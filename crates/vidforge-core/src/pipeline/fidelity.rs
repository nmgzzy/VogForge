//! 保真度约束求解（设计文档 4.4）：用户勾选的每一项能不能保留、为什么、怎么修。

use crate::model::{
    AudioMode, Capabilities, Codec, Container, DoviAction, FidelityItem, FidelityKind, FidelityState, Fix, HdrAction,
    HdrKind, MediaInfo, Scenario, StreamAction, SubtitleMode, TrackRole, TranscodePlan,
};

use super::container::audio_fits;
use super::encoders::{supports_10bit, writes_hdr10};

pub struct FidelityMeta {
    pub kind: FidelityKind,
    pub label: &'static str,
    pub hint: &'static str,
}

/// 展示顺序与文案
pub const FIDELITY_META: [FidelityMeta; 8] = [
    FidelityMeta {
        kind: FidelityKind::DolbyVision,
        label: "杜比视界",
        hint: "逐帧动态元数据，支持的电视能呈现更准确的 HDR",
    },
    FidelityMeta {
        kind: FidelityKind::Hdr10, label: "HDR（HDR10 / HLG）", hint: "高动态范围与广色域信息"
    },
    FidelityMeta { kind: FidelityKind::Hdr10plus, label: "HDR10+", hint: "另一种逐帧动态元数据" },
    FidelityMeta {
        kind: FidelityKind::Lossless, label: "全景声与无损音轨", hint: "TrueHD、Atmos、DTS-HD MA、PCM"
    },
    FidelityMeta { kind: FidelityKind::AllAudio, label: "全部音轨", hint: "多语言与评论音轨" },
    FidelityMeta { kind: FidelityKind::AllSubtitles, label: "全部字幕", hint: "包括蓝光图形字幕（PGS）" },
    FidelityMeta { kind: FidelityKind::Chapters, label: "章节", hint: "片内章节跳转点" },
    FidelityMeta { kind: FidelityKind::TenBit, label: "10bit 色深", hint: "减少天空、渐变处的色带" },
];

fn label_of(kind: FidelityKind) -> &'static str {
    FIDELITY_META.iter().find(|m| m.kind == kind).map(|m| m.label).unwrap_or("")
}

fn item(kind: FidelityKind, state: FidelityState, detail: impl Into<String>, fixes: Vec<Fix>) -> FidelityItem {
    FidelityItem { kind, label: label_of(kind).to_string(), state, detail: detail.into(), fixes }
}

fn fix(id: &str, label: impl Into<String>) -> Fix {
    Fix { id: id.to_string(), label: label.into() }
}

fn remux_fix() -> Fix {
    fix("remux", "改为原样封装")
}

fn dv_label(profile: u8, bl_compat_id: u8) -> String {
    if profile == 8 { format!("8.{bl_compat_id}") } else { profile.to_string() }
}

fn upper(s: &str) -> String {
    s.to_uppercase()
}

pub fn resolve_fidelity(m: &MediaInfo, p: &TranscodePlan, caps: &Capabilities) -> Vec<FidelityItem> {
    FIDELITY_META
        .iter()
        .map(|meta| match meta.kind {
            FidelityKind::DolbyVision => dolby_vision(m, p, caps),
            FidelityKind::Hdr10 => hdr10(m, p),
            FidelityKind::Hdr10plus => hdr10plus(m, p),
            FidelityKind::Lossless => lossless(m, p),
            FidelityKind::AllAudio => all_audio(m, p),
            FidelityKind::AllSubtitles => all_subtitles(m, p),
            FidelityKind::Chapters => chapters(m, p),
            FidelityKind::TenBit => ten_bit(m, p, caps),
        })
        .collect()
}

fn dolby_vision(m: &MediaInfo, p: &TranscodePlan, caps: &Capabilities) -> FidelityItem {
    use FidelityState::*;
    let k = FidelityKind::DolbyVision;
    let Some(dv) = m.video.first().and_then(|v| v.dolby_vision.as_ref()) else {
        return item(k, NotApplicable, "源文件不含杜比视界", vec![]);
    };
    let name = format!("Profile {}", dv_label(dv.profile, dv.bl_compat_id));
    let vp = &p.video;
    if vp.action == StreamAction::Copy {
        return item(k, Achievable, format!("原样封装完整保留 {name} 的全部数据，包括增强层"), vec![]);
    }
    if dv.has_enhancement_layer {
        let el = match dv.el_type {
            Some(crate::model::ElType::Mel) => "MEL",
            Some(crate::model::ElType::Fel) => "FEL",
            None => "EL",
        };
        return item(
            k,
            Impossible,
            format!(
                "源为 {name} 双层（{el}）。ffmpeg 无法编码增强层，重编码只能降级为 8.1 并丢失增强层的亮度与色度映射。要完整保留，请改为原样封装。"
            ),
            vec![remux_fix()],
        );
    }
    if !caps.dolby_vision_encode {
        return item(k, Impossible, "当前 ffmpeg 低于 7.1，无法写入杜比视界", vec![remux_fix()]);
    }
    let mut blockers = Vec::new();
    let mut changes = Vec::new();
    if vp.dovi != DoviAction::Preserve {
        blockers.push("当前设置为不保留");
    }
    if vp.encoder.is_hardware() {
        blockers.push("硬件编码器无法输出杜比视界");
        changes.push("改用 CPU 编码");
    }
    if vp.codec == Codec::H264 {
        blockers.push("H.264 的杜比视界几乎没有播放器支持");
        changes.push("改用 HEVC");
    }
    if vp.hdr_action == HdrAction::Tonemap {
        blockers.push("当前会色调映射为 SDR");
        changes.push("保留 HDR");
    }
    if vp.bit_depth != 10 {
        blockers.push("杜比视界要求 10bit");
        changes.push("10bit");
    }
    if !blockers.is_empty() {
        let label = if changes.is_empty() {
            "开启保留".to_string()
        } else {
            format!("开启保留（{}）", changes.join(" · "))
        };
        return item(k, NeedsChange, blockers.join("；"), vec![fix("dovi_preserve", label)]);
    }
    let mut detail = format!("保留 {name}。ffmpeg 解析源的 RPU 后重新生成，逐帧动态元数据完整保留。");
    if p.container != Container::Mkv {
        detail.push_str(" MP4/MOV 输出会自动添加 hvc1 标记与 -strict unofficial。");
    }
    if dv.profile == 5 {
        detail.push_str(" 注意：Profile 5 没有 HDR10 回退层，不支持杜比视界的设备会显示偏色。");
    }
    item(k, Achievable, detail, vec![])
}

fn hdr10(m: &MediaInfo, p: &TranscodePlan) -> FidelityItem {
    use FidelityState::*;
    let k = FidelityKind::Hdr10;
    let Some(v) = m.video.first().filter(|v| v.color.hdr_kind != HdrKind::None) else {
        return item(k, NotApplicable, "源为 SDR 视频", vec![]);
    };
    let is_hlg = v.color.hdr_kind == HdrKind::Hlg;
    let vp = &p.video;
    if vp.action == StreamAction::Copy {
        return item(k, Achievable, "原样封装完整保留 HDR 信息", vec![]);
    }
    let mut blockers = Vec::new();
    let mut changes = Vec::new();
    if vp.hdr_action == HdrAction::Tonemap {
        blockers.push("当前会色调映射为 SDR".to_string());
        changes.push("保留 HDR");
    }
    if vp.bit_depth != 10 {
        blockers.push("8bit 输出 HDR 会产生明显色带".to_string());
        changes.push("10bit");
    }
    if !is_hlg && !writes_hdr10(vp.encoder) {
        blockers.push(format!("{} 不会把 HDR10 元数据写入码流", vp.encoder.name()));
        changes.push(if vp.codec == Codec::H264 { "改用 HEVC" } else { "改用 CPU 编码" });
    }
    if !blockers.is_empty() {
        return item(
            k,
            NeedsChange,
            blockers.join("；"),
            vec![fix("keep_hdr", format!("保留 HDR（{}）", changes.join(" · ")))],
        );
    }
    if is_hlg {
        return item(k, Achievable, "保留 HLG 色彩标记。HLG 不依赖额外元数据，电视与手机可直接识别。", vec![]);
    }
    let how = if vp.encoder.is_hardware() {
        format!("{} 会把母版显示与 MaxCLL 写入码流（已在本机实测验证）", vp.encoder.vendor().label())
    } else {
        "软件编码器会自动透传母版显示与 MaxCLL 元数据".to_string()
    };
    item(k, Achievable, format!("保留 HDR10。{how}。"), vec![])
}

fn hdr10plus(m: &MediaInfo, p: &TranscodePlan) -> FidelityItem {
    use FidelityState::*;
    let k = FidelityKind::Hdr10plus;
    if !m.video.first().is_some_and(|v| v.hdr10plus) {
        return item(k, NotApplicable, "源文件不含 HDR10+", vec![]);
    }
    if p.video.action == StreamAction::Copy {
        return item(k, Achievable, "原样封装完整保留", vec![]);
    }
    item(
        k,
        Impossible,
        "ffmpeg 无法把 HDR10+ 动态元数据透传给编码器，需借助 x265 命令行或事后注入（v2 支持）。重编码后仍会保留 HDR10 基础层。",
        vec![remux_fix()],
    )
}

fn audio_name(a: &crate::model::AudioStream) -> String {
    a.title.clone().unwrap_or_else(|| upper(&a.codec))
}

fn lossless(m: &MediaInfo, p: &TranscodePlan) -> FidelityItem {
    use FidelityState::*;
    let k = FidelityKind::Lossless;
    let hq: Vec<_> = m.audio.iter().filter(|a| a.lossless || a.atmos).collect();
    if hq.is_empty() {
        return item(k, NotApplicable, "源文件没有无损或全景声音轨", vec![]);
    }
    let mut blockers = Vec::new();
    let mut need_mkv = false;
    for a in &hq {
        let name = audio_name(a);
        let copied = p.audio.iter().any(|t| t.source_index == a.index && t.action == StreamAction::Copy);
        if !copied {
            blockers.push(if a.atmos {
                format!("「{name}」会被重新编码，全景声元数据将丢失（Atmos 无法重新编码）")
            } else {
                format!("「{name}」会被有损压缩")
            });
        } else if !audio_fits(&a.codec, p.container) {
            blockers.push(format!("{} 不支持 {}", upper(p.container.ext()), upper(&a.codec)));
            need_mkv = true;
        }
    }
    if !blockers.is_empty() {
        let label = if need_mkv || p.audio_mode == AudioMode::CompatOnly {
            "原样保留（切换到 MKV · 追加兼容轨）"
        } else {
            "原样保留"
        };
        return item(k, NeedsChange, blockers.join("；"), vec![fix("keep_lossless", label)]);
    }
    let compat = p.audio.iter().any(|t| t.role == TrackRole::Compat);
    let names = hq.iter().map(|a| audio_name(a)).collect::<Vec<_>>().join("、");
    item(
        k,
        Achievable,
        format!("原样复制 {names}{}", if compat { "，并额外生成兼容轨，手机与耳机也能播放" } else { "" }),
        vec![],
    )
}

fn all_audio(m: &MediaInfo, p: &TranscodePlan) -> FidelityItem {
    use FidelityState::*;
    let k = FidelityKind::AllAudio;
    if m.audio.len() <= 1 {
        let why = if m.audio.is_empty() { "源文件没有音轨" } else { "源文件只有一条音轨" };
        return item(k, NotApplicable, why, vec![]);
    }
    let missing: Vec<_> = m.audio.iter().filter(|a| !p.audio.iter().any(|t| t.source_index == a.index)).collect();
    let unfit = p
        .audio
        .iter()
        .filter(|t| {
            t.action == StreamAction::Copy
                && m.audio
                    .iter()
                    .find(|a| a.index == t.source_index)
                    .is_some_and(|a| !audio_fits(&a.codec, p.container))
        })
        .count();
    if missing.is_empty() && unfit == 0 {
        return item(k, Achievable, format!("保留全部 {} 条音轨", m.audio.len()), vec![]);
    }
    let mut parts = Vec::new();
    if !missing.is_empty() {
        let names = missing.iter().map(|a| a.title.clone().unwrap_or_else(|| a.codec.clone())).collect::<Vec<_>>();
        parts.push(format!("将丢弃 {} 条：{}", missing.len(), names.join("、")));
    }
    if unfit > 0 {
        parts.push(format!("{unfit} 条音轨的编码不被 {} 支持", upper(p.container.ext())));
    }
    let label = if unfit > 0 || p.container == Container::Mp4 {
        "复制全部（切换到 MKV）"
    } else {
        "复制全部音轨"
    };
    item(k, NeedsChange, parts.join("；"), vec![fix("audio_copy_all", label)])
}

fn all_subtitles(m: &MediaInfo, p: &TranscodePlan) -> FidelityItem {
    use FidelityState::*;
    let k = FidelityKind::AllSubtitles;
    if m.subtitle.is_empty() {
        return item(k, NotApplicable, "源文件没有字幕", vec![]);
    }
    let image = m.subtitle.iter().filter(|s| s.image_based).count();
    let blocker = match p.subtitles {
        SubtitleMode::None => Some("当前设置不保留字幕".to_string()),
        SubtitleMode::TextOnly if image > 0 => Some(format!("将丢弃 {image} 条图形字幕（PGS）")),
        _ if p.container != Container::Mkv && image > 0 => {
            Some(format!("{} 不支持 PGS 图形字幕", upper(p.container.ext())))
        }
        _ => None,
    };
    if let Some(b) = blocker {
        let label = if image > 0 { "保留全部（切换到 MKV）" } else { "保留全部字幕" };
        return item(k, NeedsChange, b, vec![fix("subs_all", label)]);
    }
    item(k, Achievable, format!("保留全部 {} 条字幕", m.subtitle.len()), vec![])
}

fn chapters(m: &MediaInfo, p: &TranscodePlan) -> FidelityItem {
    let k = FidelityKind::Chapters;
    if m.chapters == 0 {
        return item(k, FidelityState::NotApplicable, "源文件没有章节", vec![]);
    }
    let note = if p.container == Container::Mkv { "" } else { "（MP4/MOV 的章节在部分播放器中不显示）" };
    item(k, FidelityState::Achievable, format!("保留 {} 个章节{note}", m.chapters), vec![])
}

fn ten_bit(m: &MediaInfo, p: &TranscodePlan, caps: &Capabilities) -> FidelityItem {
    use FidelityState::*;
    let k = FidelityKind::TenBit;
    let Some(v) = m.video.first().filter(|v| v.bit_depth >= 10) else {
        return item(k, NotApplicable, "源为 8bit 视频", vec![]);
    };
    let _ = v;
    let vp = &p.video;
    if vp.action == StreamAction::Copy {
        return item(k, Achievable, "原样封装保持原始位深", vec![]);
    }
    let supported = supports_10bit(vp.encoder, caps);
    if vp.bit_depth == 10 && supported {
        return item(k, Achievable, "输出 10bit", vec![]);
    }
    let why =
        if supported { "当前输出 8bit".to_string() } else { format!("{} 不支持 10bit", vp.encoder.name()) };
    item(k, NeedsChange, why, vec![fix("ten_bit", "改为 10bit")])
}

/// 把一键修正应用到计划上。调用方随后应再做一次 normalize
pub fn apply_fix(mut plan: TranscodePlan, fix_id: &str, media: &MediaInfo) -> TranscodePlan {
    let has_image_subs = media.subtitle.iter().any(|s| s.image_based);
    let vp = &mut plan.video;
    match fix_id {
        "remux" => {
            plan.scenario = Scenario::Remux;
            vp.action = StreamAction::Copy;
            vp.dovi = DoviAction::Remux;
            vp.hdr_action = HdrAction::Keep;
            plan.audio_mode = AudioMode::CopyAll;
            plan.subtitles = SubtitleMode::All;
            plan.container = Container::Mkv;
        }
        "dovi_preserve" => {
            vp.dovi = DoviAction::Preserve;
            vp.encoder_auto = true;
            vp.hdr_action = HdrAction::Keep;
            vp.bit_depth = 10;
            if vp.codec == Codec::H264 {
                vp.codec = Codec::Hevc;
            }
            plan.fidelity.dolby_vision = true;
        }
        "keep_hdr" => {
            vp.hdr_action = HdrAction::Keep;
            vp.bit_depth = 10;
            if vp.codec == Codec::H264 {
                vp.codec = Codec::Hevc;
            }
            if !writes_hdr10(vp.encoder) {
                vp.encoder_auto = true;
            }
            plan.fidelity.hdr10 = true;
        }
        "keep_lossless" => {
            if plan.audio_mode == AudioMode::CompatOnly {
                plan.audio_mode = AudioMode::OriginalPlusCompat;
            }
            plan.container = Container::Mkv;
            plan.fidelity.lossless = true;
        }
        "audio_copy_all" => {
            plan.audio_mode = AudioMode::CopyAll;
            plan.container = Container::Mkv;
            plan.fidelity.all_audio = true;
        }
        "subs_all" => {
            plan.subtitles = SubtitleMode::All;
            if has_image_subs {
                plan.container = Container::Mkv;
            }
            plan.fidelity.all_subtitles = true;
        }
        "ten_bit" => {
            vp.bit_depth = 10;
            vp.encoder_auto = true;
            plan.fidelity.ten_bit = true;
        }
        _ => {}
    }
    plan
}
