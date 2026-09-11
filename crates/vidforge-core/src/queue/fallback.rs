//! 运行时失败的处理（设计文档 4.2）：先按 stderr 分类，再决定降级重试、换编码器还是放弃。
//!
//! 纯函数：输入失败分类、当前计划与能力，输出下一步。调度器负责执行并把原因写进任务日志。

use crate::i18n::Lang;
use crate::model::{
    Capabilities, Codec, EncoderId, FailureKind, HdrAction, HdrKind, MediaInfo, Platform, StreamAction, TranscodePlan,
    Vendor,
};
use crate::pipeline::encoders::{supports_10bit, writes_hdr10};
use crate::pipeline::strategy::switch_encoder;
use crate::tr;

/// 运行这么久之后才失败，输出可能已写入大量数据：不再逐个试硬件编码器，直接软编从头重跑
pub const LATE_FAILURE_SEC: f64 = 10.0;
/// 资源不足时最多重试的次数（间隔 1、2、4 秒）
pub const RESOURCE_RETRIES: u32 = 3;

/// 同一任务已经做过的降级，避免反复尝试同一种办法
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tried {
    pub to_8bit: bool,
    pub without_extra_args: bool,
    pub resource_retries: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Recovery {
    Retry {
        plan: TranscodePlan,
        message: String,
        /// 设备缺失：本次运行停用这个厂商
        disable_vendor: Option<Vendor>,
        /// 重试前等待（资源不足时退避）
        delay_ms: u64,
        /// 资源不足：之后 GPU 任务逐个运行
        serialize_gpu: bool,
    },
    GiveUp {
        message: String,
    },
}

/// 回退链（设计文档 4.2）：Windows 上 NVENC → QSV → AMF → 软编，macOS 上 VideoToolbox → 软编
pub fn fallback_chain(codec: Codec, platform: Platform) -> Vec<EncoderId> {
    let vendors: &[Vendor] = match platform {
        Platform::Windows => &[Vendor::Nvidia, Vendor::Intel, Vendor::Amd],
        Platform::Macos => &[Vendor::Apple],
        Platform::Linux => &[Vendor::Nvidia, Vendor::Intel],
    };
    vendors.iter().filter_map(|&v| EncoderId::for_vendor(v, codec)).chain([EncoderId::software_for(codec)]).collect()
}

/// 回退链上当前编码器之后第一个能用、且满足计划要求（10bit、HDR10 元数据）的编码器
fn next_encoder(
    current: EncoderId,
    plan: &TranscodePlan,
    media: &MediaInfo,
    caps: &Capabilities,
    skip: &[Vendor],
) -> Option<EncoderId> {
    let chain = fallback_chain(plan.video.codec, caps.platform);
    let start = chain.iter().position(|&e| e == current).map_or(0, |i| i + 1);
    let need_hdr10 = plan.video.hdr_action == HdrAction::Keep
        && media.video.first().is_some_and(|v| v.color.hdr_kind == HdrKind::Hdr10);
    chain[start..].iter().copied().find(|&e| {
        caps.encoder_usable(e)
            && !skip.contains(&e.vendor())
            && (plan.video.bit_depth != 10 || supports_10bit(e, caps))
            && (!need_hdr10 || writes_hdr10(e))
    })
}

fn retry(plan: TranscodePlan, message: String) -> Recovery {
    Recovery::Retry { plan, message, disable_vendor: None, delay_ms: 0, serialize_gpu: false }
}

/// 决定失败后的下一步（只处理硬件编码器；软件编码器与原样封装失败由调用方直接报错）。
/// `elapsed_sec` 是这次运行了多久；给用户的说明按 `lang` 生成，ffmpeg 原文由调用方另外附上
pub fn recover(
    kind: FailureKind,
    plan: &TranscodePlan,
    media: &MediaInfo,
    caps: &Capabilities,
    tried: &mut Tried,
    elapsed_sec: f64,
    lang: Lang,
) -> Recovery {
    let enc = plan.video.encoder;
    let name = enc.name();
    if plan.video.action == StreamAction::Copy || !enc.is_hardware() {
        return Recovery::GiveUp { message: tr!(lang, "{} 失败", "{} failed", name) };
    }
    let software = EncoderId::software_for(plan.video.codec);
    let to_software = |why: String| {
        if caps.encoder_usable(software) {
            let message =
                tr!(lang, "{}，回退到软件编码 {}", "{}; falling back to software encoder {}", why, software.name());
            retry(switch_encoder(plan.clone(), software, media, caps), message)
        } else {
            let message = tr!(
                lang,
                "{}，当前 ffmpeg 也没有可用的 {}",
                "{}, and this ffmpeg has no usable {} either",
                why,
                software.name()
            );
            Recovery::GiveUp { message }
        }
    };

    if elapsed_sec > LATE_FAILURE_SEC {
        // 不静默回退：已写入的部分输出由调度器删除，这里明确说明从头重跑
        let secs = elapsed_sec.round() as u64;
        return to_software(tr!(
            lang,
            "{} 运行 {} 秒后失败，已删除部分输出，从头重跑",
            "{} failed after {} s; the partial output was deleted and the job starts over",
            name,
            secs
        ));
    }

    let switch_to = |next: EncoderId, why: String, disable: Option<Vendor>| Recovery::Retry {
        plan: switch_encoder(plan.clone(), next, media, caps),
        message: tr!(lang, "{}，回退到 {}", "{}; falling back to {}", why, next.name()),
        disable_vendor: disable,
        delay_ms: 0,
        serialize_gpu: false,
    };
    let next_or_give_up = |why: String, disable: Option<Vendor>| {
        let skip: Vec<Vendor> = disable.into_iter().collect();
        match next_encoder(enc, plan, media, caps, &skip) {
            Some(next) => switch_to(next, why, disable),
            None => Recovery::GiveUp {
                message: tr!(lang, "{}，没有其他可用的编码器", "{}, and no other encoder is available", why),
            },
        }
    };

    match kind {
        FailureKind::DeviceMissing => {
            let vendor = enc.vendor();
            let why = tr!(
                lang,
                "{} 的设备不可用，本次运行不再使用 {}",
                "{}: the device is unavailable, {} is disabled for this session",
                name,
                vendor.label()
            );
            next_or_give_up(why, Some(vendor))
        }
        FailureKind::Capability if plan.video.bit_depth == 10 && !tried.to_8bit => {
            tried.to_8bit = true;
            let mut p = plan.clone();
            p.video.bit_depth = 8;
            let message = tr!(
                lang,
                "{} 不支持这组 10bit 参数，降为 8bit 重试",
                "{} does not support these 10-bit settings; retrying in 8-bit",
                name
            );
            retry(p, message)
        }
        FailureKind::Capability => {
            next_or_give_up(tr!(lang, "{} 不支持这组参数", "{} does not support these settings", name), None)
        }
        FailureKind::Param if plan.video.extra_args.is_some() && !tried.without_extra_args => {
            tried.without_extra_args = true;
            let mut p = plan.clone();
            p.video.extra_args = None;
            let message = tr!(
                lang,
                "{} 不接受这组参数，去掉附加参数重试",
                "{} rejected the settings; retrying without the extra arguments",
                name
            );
            retry(p, message)
        }
        FailureKind::Param => next_or_give_up(tr!(lang, "{} 不接受这组参数", "{} rejected the settings", name), None),
        FailureKind::Resource if tried.resource_retries < RESOURCE_RETRIES => {
            let delay = 1000u64 << tried.resource_retries;
            tried.resource_retries += 1;
            Recovery::Retry {
                plan: plan.clone(),
                message: tr!(
                    lang,
                    "{} 资源不足，{} 秒后重试，GPU 任务改为逐个运行",
                    "{} ran out of resources; retrying in {} s and running GPU jobs one at a time",
                    name,
                    delay / 1000
                ),
                disable_vendor: None,
                delay_ms: delay,
                serialize_gpu: true,
            }
        }
        FailureKind::Resource => to_software(tr!(lang, "{} 多次资源不足", "{} kept running out of resources", name)),
        FailureKind::NotBuilt | FailureKind::Unknown => to_software(tr!(lang, "{} 失败", "{} failed", name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EncoderProbe, EnvStatus, Scenario};
    use crate::pipeline::recommend_plan;

    fn media() -> MediaInfo {
        serde_json::from_str(
            r#"{"id":"m","path":"/a/b.mp4","name":"b.mp4","container":"mov","durationSec":60,"sizeBytes":1000000,
            "bitrate":50000000,"video":[{"index":0,"codec":"h264","width":1920,"height":1080,"fpsAvg":30,"fpsNominal":30,
            "isVfr":false,"bitDepth":8,"pixFmt":"yuv420p","color":{"primaries":"bt709","transfer":"bt709","space":"bt709",
            "range":"tv","hdrKind":"none"},"hdr10plus":false,"rotation":0}],"audio":[],"subtitle":[],"chapters":0,
            "attachments":0,"sourceHint":"unknown"}"#,
        )
        .unwrap()
    }

    /// Windows，NVENC / QSV / AMF 与软编都可用
    fn caps() -> Capabilities {
        let mut c = Capabilities::placeholder(EnvStatus::Ready, "");
        c.platform = Platform::Windows;
        c.encoders = EncoderId::ALL
            .into_iter()
            .filter(|e| e.vendor() != Vendor::Apple)
            .map(|id| EncoderProbe {
                id,
                vendor: id.vendor(),
                codec: id.codec(),
                usable: true,
                ten_bit: id != EncoderId::H264Qsv,
                error: None,
                failure: None,
            })
            .collect();
        c
    }

    fn plan_with(enc: EncoderId) -> TranscodePlan {
        switch_encoder(recommend_plan(&media(), Scenario::Streaming, &caps()), enc, &media(), &caps())
    }

    fn run(kind: FailureKind, plan: &TranscodePlan, tried: &mut Tried, elapsed: f64) -> Recovery {
        recover(kind, plan, &media(), &caps(), tried, elapsed, Lang::ZhCn)
    }

    fn retried(r: &Recovery) -> (EncoderId, String) {
        match r {
            Recovery::Retry { plan, message, .. } => (plan.video.encoder, message.clone()),
            Recovery::GiveUp { message } => panic!("应重试，却放弃：{message}"),
        }
    }

    #[test]
    fn chains_follow_the_platform_order() {
        assert_eq!(
            fallback_chain(Codec::Hevc, Platform::Windows),
            [EncoderId::HevcNvenc, EncoderId::HevcQsv, EncoderId::HevcAmf, EncoderId::Libx265]
        );
        assert_eq!(fallback_chain(Codec::Hevc, Platform::Macos), [EncoderId::HevcVideotoolbox, EncoderId::Libx265]);
        assert_eq!(fallback_chain(Codec::Av1, Platform::Macos), [EncoderId::Libsvtav1]);
    }

    #[test]
    fn device_missing_disables_the_vendor_and_moves_down_the_chain() {
        let p = plan_with(EncoderId::HevcNvenc);
        let r = run(FailureKind::DeviceMissing, &p, &mut Tried::default(), 0.0);
        assert!(matches!(r, Recovery::Retry { disable_vendor: Some(Vendor::Nvidia), .. }));
        let (enc, msg) = retried(&r);
        assert_eq!(enc, EncoderId::HevcQsv);
        assert!(msg.contains("回退到 hevc_qsv") && msg.contains("不再使用"), "{msg}");
    }

    #[test]
    fn capability_first_drops_to_8bit_then_moves_on() {
        let mut p = plan_with(EncoderId::HevcQsv);
        p.video.bit_depth = 10;
        let mut tried = Tried::default();
        let r = run(FailureKind::Capability, &p, &mut tried, 0.0);
        let Recovery::Retry { plan, .. } = &r else { panic!() };
        assert_eq!((plan.video.encoder, plan.video.bit_depth), (EncoderId::HevcQsv, 8));
        let (enc, _) = retried(&run(FailureKind::Capability, plan, &mut tried, 0.0));
        assert_eq!(enc, EncoderId::HevcAmf);
    }

    #[test]
    fn param_first_drops_extra_args() {
        let mut p = plan_with(EncoderId::HevcQsv);
        p.video.extra_args = Some("-foo 1".into());
        let mut tried = Tried::default();
        let Recovery::Retry { plan, message, .. } = run(FailureKind::Param, &p, &mut tried, 0.0) else { panic!() };
        assert_eq!((plan.video.encoder, plan.video.extra_args.as_deref()), (EncoderId::HevcQsv, None));
        assert!(message.contains("去掉附加参数"));
        let (enc, _) = retried(&run(FailureKind::Param, &plan, &mut tried, 0.0));
        assert_eq!(enc, EncoderId::HevcAmf);
    }

    #[test]
    fn resource_backs_off_then_falls_back_to_software() {
        let p = plan_with(EncoderId::HevcQsv);
        let mut tried = Tried::default();
        let delays: Vec<u64> = (0..RESOURCE_RETRIES)
            .map(|_| match run(FailureKind::Resource, &p, &mut tried, 0.0) {
                Recovery::Retry { delay_ms, serialize_gpu: true, plan, .. } => {
                    assert_eq!(plan.video.encoder, EncoderId::HevcQsv);
                    delay_ms
                }
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(delays, [1000, 2000, 4000]);
        assert_eq!(retried(&run(FailureKind::Resource, &p, &mut tried, 0.0)).0, EncoderId::Libx265);
    }

    #[test]
    fn unknown_goes_straight_to_software() {
        let (enc, msg) =
            retried(&run(FailureKind::Unknown, &plan_with(EncoderId::HevcNvenc), &mut Tried::default(), 0.0));
        assert_eq!(enc, EncoderId::Libx265);
        assert!(msg.contains("回退到软件编码"));
    }

    #[test]
    fn late_failures_restart_on_software_and_say_so() {
        let p = plan_with(EncoderId::HevcNvenc);
        let (enc, msg) = retried(&run(FailureKind::DeviceMissing, &p, &mut Tried::default(), 42.0));
        assert_eq!(enc, EncoderId::Libx265);
        assert!(msg.contains("运行 42 秒后失败") && msg.contains("已删除部分输出"), "{msg}");
    }

    #[test]
    fn software_failures_are_final() {
        let p = plan_with(EncoderId::Libx265);
        assert!(matches!(run(FailureKind::Unknown, &p, &mut Tried::default(), 0.0), Recovery::GiveUp { .. }));
    }

    #[test]
    fn the_chain_skips_encoders_that_cannot_keep_the_plan() {
        // 10bit 计划：h264_qsv 没有 10bit，NVENC 失败后直接到 h264_amf
        let mut p = plan_with(EncoderId::H264Nvenc);
        p.video.bit_depth = 10;
        let (enc, _) = retried(&run(FailureKind::DeviceMissing, &p, &mut Tried::default(), 0.0));
        assert_eq!(enc, EncoderId::H264Amf);
    }

    #[test]
    fn messages_follow_the_interface_language() {
        let p = plan_with(EncoderId::HevcNvenc);
        let r = recover(FailureKind::DeviceMissing, &p, &media(), &caps(), &mut Tried::default(), 0.0, Lang::En);
        let (_, msg) = retried(&r);
        assert_eq!(
            msg,
            "hevc_nvenc: the device is unavailable, NVIDIA NVENC is disabled for this session; falling back to hevc_qsv"
        );
    }
}
