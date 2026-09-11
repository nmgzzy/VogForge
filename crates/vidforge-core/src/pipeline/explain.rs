//! "为什么这么选"（需求 F-2.3）。依据当前计划推导，而不是记录推荐时的理由，用户改了参数后理由会同步更新。

use crate::model::{
    Capabilities, Codec, Container, Decision, DoviAction, Estimate, FpsInsight, FpsPolicy, HdrAction, HdrKind,
    MediaInfo, QualityTier, RateControl, Scenario, Severity, StreamAction, ToneMapPipeline, TrackRole, TranscodePlan,
};

use super::args::{display_size, target_dimensions};
use super::encoders::{Family, codec_available, family, quality_meta, software_usable};
use super::fps::is_extreme_vfr;
use super::strategy::{prefer_hw_for, scenario_codec};
use super::text::{format_bitrate, format_fps, format_percent, thousands};

fn tier_label(t: QualityTier) -> &'static str {
    match t {
        QualityTier::Lossless => "视觉无损",
        QualityTier::High => "高画质",
        QualityTier::Standard => "标准",
        QualityTier::Small => "小体积",
    }
}

fn tonemap_label(p: ToneMapPipeline) -> &'static str {
    match p {
        ToneMapPipeline::Libplacebo => "libplacebo",
        ToneMapPipeline::TonemapOpencl => "OpenCL",
        ToneMapPipeline::Zscale => "zscale（CPU）",
        ToneMapPipeline::ScaleVt => "VideoToolbox",
    }
}

struct Out(Vec<Decision>);

impl Out {
    fn add(&mut self, field: &str, value: impl Into<String>, reason: impl Into<String>) {
        self.push(field, value, reason, Severity::Info);
    }
    fn push(&mut self, field: &str, value: impl Into<String>, reason: impl Into<String>, severity: Severity) {
        self.0.push(Decision { field: field.into(), value: value.into(), reason: reason.into(), severity });
    }
}

pub fn explain(
    media: &MediaInfo,
    plan: &TranscodePlan,
    caps: &Capabilities,
    fps: Option<&FpsInsight>,
    est: Option<&Estimate>,
) -> Vec<Decision> {
    let mut out = Out(Vec::new());
    let Some(v) = media.video.first() else { return out.0 };
    let vp = &plan.video;

    if vp.action == StreamAction::Copy {
        out.add("视频", "原样复制", "不重新编码，画质零损失，速度只受磁盘读写限制");
        out.add("容器", plan.container.ext().to_uppercase(), "MKV 能完整容纳杜比视界、无损音轨、图形字幕与章节");
        return out.0;
    }

    // ── 编码器 ──
    let vendor = vp.encoder.vendor();
    let encoder = vp.encoder.name();
    let encoder_ok = caps.encoder_usable(vp.encoder);
    if caps.status == crate::model::EnvStatus::Ready && !encoder_ok {
        out.push(
            "编码器",
            encoder,
            format!("当前 ffmpeg 没有任何可用的 {} 编码器，这个计划无法执行", vp.codec.label()),
            Severity::Warn,
        );
    } else if !vp.encoder_auto {
        out.add("编码器", encoder, "你手动指定了编码器，自动选择已关闭");
    } else if vp.encoder.is_hardware() && !software_usable(vp.codec, caps) {
        out.push(
            "编码器",
            encoder,
            format!(
                "当前 ffmpeg 没有 {} 软件编码器，改用 {}；同画质下体积会大 15–30%",
                vp.codec.label(),
                vendor.label()
            ),
            Severity::Warn,
        );
    } else if vp.dovi == DoviAction::Preserve {
        out.add("编码器", encoder, "杜比视界的逐帧元数据只能由软件编码器写入，因此本次不使用 GPU 编码");
    } else if vp.encoder.is_hardware() {
        out.add(
            "编码器",
            encoder,
            format!(
                "使用 {}（启动时已真实试编码验证可用），速度约为软编的 5–10 倍；同画质下体积会大 15–30%",
                vendor.label()
            ),
        );
    } else if prefer_hw_for(plan.scenario) {
        out.push(
            "编码器",
            encoder,
            format!("没有可用的 {} 硬件编码器，已改用软件编码", vp.codec.label()),
            Severity::Warn,
        );
    } else if plan.scenario == Scenario::Editing {
        out.add("编码器", encoder, "剪辑素材要经得起调色与二次导出，用软件编码保证画质；硬件编码的码率控制不够稳定");
    } else {
        out.add("编码器", encoder, "软件编码在同体积下画质最好，适合长期保存；硬件编码更快但同画质体积更大");
    }

    // ── 编码格式 ──
    let wanted = scenario_codec(plan.scenario, media);
    if vp.codec != wanted && !codec_available(wanted, caps) {
        out.push(
            "编码格式",
            vp.codec.label(),
            format!("当前 ffmpeg 没有可用的 {} 编码器，已改用 {}", wanted.label(), vp.codec.label()),
            Severity::Warn,
        );
    } else if plan.scenario == Scenario::Editing {
        out.add(
            "编码格式",
            vp.codec.label(),
            if vp.codec == Codec::H264 {
                "H.264 在各剪辑软件中解码最流畅，时间线拖动不卡"
            } else {
                "源为 HDR，用 HEVC 10bit 才能保留 HDR；达芬奇与 Final Cut 均支持"
            },
        );
    } else if vp.codec == Codec::Hevc && v.codec == "h264" {
        out.add("编码格式", "HEVC", "同画质下 HEVC 比 H.264 体积小约 40–50%，2016 年后的设备普遍能硬解播放");
    } else if vp.codec == Codec::Av1 {
        out.push("编码格式", "AV1", "AV1 比 HEVC 再省约 20–30%，但 2020 年前的设备多数无法硬解播放", Severity::Tip);
    } else if vp.codec == Codec::H264 {
        out.add("编码格式", "H.264", "H.264 兼容性最好，几乎所有设备和平台都能直接播放");
    }

    // ── 质量与码率 ──
    let meta = quality_meta(vp.encoder);
    let quality = format!("{} · {} {}", tier_label(vp.quality), meta.param, vp.quality_value);
    let mbps = |kbps: u32| format_bitrate(f64::from(kbps) * 1000.0);
    match vp.rate_control {
        RateControl::Quality => out.add(
            "画质",
            quality,
            format!(
                "{} {}画质越好。不同编码器的数值刻度不等价，档位已按编码器分别换算",
                meta.param,
                if meta.lower_is_better { "越小" } else { "越大" }
            ),
        ),
        RateControl::Capped { kbps } => out.add(
            "画质",
            format!("{quality}，峰值 ≤ {}", mbps(kbps)),
            if family(vp.encoder) == Family::Qsv {
                format!("QSV 用 QVBR 实现：按质量编码，平均码率约 {}，峰值不超过上限", mbps(kbps * 2 / 3))
            } else {
                "按质量编码，同时限制峰值码率，网络串流时不易卡顿；复杂画面会略降画质以守住上限".to_string()
            },
        ),
        RateControl::Bitrate { kbps } => out.add(
            "码率",
            format!("平均 {}", mbps(kbps)),
            "按目标码率编码，体积可预测；画面复杂的片段画质会下降。追求画质稳定用恒定质量",
        ),
        RateControl::TwoPass { kbps } => out.add(
            "码率",
            format!("两遍 · 平均 {}", mbps(kbps)),
            "第一遍分析全片复杂度，第二遍按目标码率分配，体积准确、画质比单遍按码率编码更均匀；耗时约 1.7 倍",
        ),
    }

    // ── 位深 ──
    if vp.bit_depth == 10 {
        if v.color.hdr_kind != HdrKind::None && vp.hdr_action == HdrAction::Keep {
            out.add("位深", "10bit", "HDR 必须 10bit，8bit 会在天空与暗部出现明显色带");
        } else if v.bit_depth == 8 {
            out.add("位深", "10bit", "8bit 源用 10bit 编码能减少渐变处的色带，体积几乎不变");
        }
    }

    // ── HDR ──
    if v.color.hdr_kind != HdrKind::None {
        let kind = if v.color.hdr_kind == HdrKind::Hlg { "HLG" } else { "HDR10" };
        let wants_sdr = matches!(plan.scenario, Scenario::Mobile | Scenario::Social);
        if vp.hdr_action == HdrAction::Keep && wants_sdr && caps.pick_tonemap().is_none() {
            out.push(
                "HDR",
                format!("无法转为 SDR，保留 {kind}"),
                "当前 ffmpeg 没有任何可用的色调映射滤镜（libplacebo / OpenCL / zscale）。在普通屏幕上可能发灰，建议换用带 libplacebo 或 zscale 的构建",
                Severity::Warn,
            );
        } else if vp.hdr_action == HdrAction::Tonemap {
            let pipe = tonemap_label(vp.tonemap.unwrap_or(ToneMapPipeline::Libplacebo));
            let dv_note = if v.dolby_vision.is_some() && vp.tonemap == Some(ToneMapPipeline::Libplacebo) {
                "，并利用杜比视界元数据提升映射准确度"
            } else {
                ""
            };
            out.add(
                "HDR",
                "色调映射为 SDR",
                format!("源为 {kind}，目标多为 SDR 屏幕。使用 {pipe} 做色调映射{dv_note}，避免画面发灰"),
            );
        } else if vp.encoder.is_hardware() && kind == "HDR10" {
            out.add(
                "HDR",
                format!("保留 {kind}"),
                format!("{} 会把 HDR10 元数据写入码流，已在本机实测验证", vendor.label()),
            );
        } else {
            out.add(
                "HDR",
                format!("保留 {kind}"),
                if kind == "HLG" {
                    "保留 HLG 色彩标记，HDR 电视与手机可直接识别"
                } else {
                    "母版显示与 MaxCLL 元数据由 ffmpeg 自动透传"
                },
            );
        }
    }

    // ── 杜比视界 ──
    if let Some(dv) = &v.dolby_vision {
        if dv.has_enhancement_layer {
            out.push(
                "杜比视界",
                "仅保留基础层",
                format!(
                    "源为 Profile {} 双层。ffmpeg 无法编码增强层，重编码后只剩 HDR10 基础层。要完整保留，请改为\"原样封装\"",
                    dv.profile
                ),
                Severity::Warn,
            );
        } else if vp.dovi == DoviAction::Preserve {
            if dv.profile == 5 {
                out.push(
                    "杜比视界",
                    "保留 Profile 5",
                    "Profile 5 没有 HDR10 回退层，不支持杜比视界的设备会显示偏绿或偏紫",
                    Severity::Warn,
                );
            } else {
                out.add(
                    "杜比视界",
                    format!("保留 Profile {}.{}", dv.profile, dv.bl_compat_id),
                    "传入 -dolbyvision 1，保留失败时会明确报错而不是静默丢弃",
                );
            }
        } else {
            out.add(
                "杜比视界",
                "不保留",
                format!(
                    "已显式传入 -dolbyvision 0（ffmpeg 默认会自动开启），输出将以 {} 播放",
                    if v.color.hdr_kind == HdrKind::Hlg { "HLG" } else { "HDR10" }
                ),
            );
        }
    }

    // ── 分辨率 ──
    let (sw, sh) = display_size(v);
    if let Some(d) = target_dimensions(v, vp.resolution) {
        out.add("分辨率", format!("{}×{}", d.w, d.h), format!("从 {sw}×{sh} 缩小，使用 lanczos 保留细节"));
    } else if vp.resolution != crate::model::ResolutionPreset::Source {
        out.push("分辨率", "保持原始", format!("目标分辨率不低于源（{sw}×{sh}），不做放大"), Severity::Tip);
    }

    // ── 帧率 ──
    match (vp.fps, fps) {
        (FpsPolicy::Cfr { .. }, Some(fps)) => {
            let target = format_fps(fps.target_fps);
            if v.is_vfr && is_extreme_vfr(v) {
                let pct = format_percent(fps.duplicated as f64 / fps.target_frames.max(1) as f64);
                out.push(
                    "帧率",
                    format!("{target} fps 固定"),
                    format!(
                        "源平均仅 {} fps，会复制 {} 帧（占 {pct}）。重复帧几乎不占体积，但编码更慢；剪辑也可降到 30 fps",
                        format_fps(v.fps_avg),
                        thousands(fps.duplicated)
                    ),
                    Severity::Warn,
                );
            } else if v.is_vfr {
                out.add(
                    "帧率",
                    format!("{target} fps 固定"),
                    format!("导入剪辑软件不会逐渐音画错位；复制约 {} 帧补齐时间轴", thousands(fps.duplicated)),
                );
            } else {
                out.add("帧率", format!("{target} fps 固定"), "源本身已是固定帧率，输出保持一致");
            }
        }
        (FpsPolicy::Keep, _) if v.is_vfr => {
            out.add("帧率", "保持可变帧率", "保持原始时间戳，播放没有问题；之后要剪辑的话，在帧率里打开“转为固定帧率”");
        }
        _ => {}
    }

    if let Some(gop) = vp.gop {
        out.add("关键帧", format!("每 {gop} 帧"), "关键帧间隔约 0.5 秒，剪辑软件拖动时间线更流畅，代价是体积略增");
    }

    // ── 耗时 ──
    const LONG_ENCODE_SEC: f64 = 3.0 * 3600.0;
    if let Some(est) = est.filter(|e| !vp.encoder.is_hardware() && e.time_max_sec > LONG_ENCODE_SEC) {
        let slow = matches!(vp.preset.as_str(), "slow" | "slower" | "veryslow");
        out.push(
            "耗时",
            format!("可能超过 {} 小时", super::text::plain((est.time_max_sec / 3600.0).round())),
            if slow {
                format!(
                    "CPU 以 {} 速度编码 {}×{} 很慢。不在意极致压缩率的话，可在“更多参数”里改为 medium，速度约快 2 倍，体积仅增加 5% 左右",
                    vp.preset, v.width, v.height
                )
            } else {
                "CPU 编码高分辨率长片耗时较长，可以放在队列里夜间运行".to_string()
            },
            Severity::Tip,
        );
    }

    // ── 音频 ──
    let hq = media.audio.iter().find(|a| a.atmos || a.lossless);
    let hq_copied =
        hq.is_some_and(|h| plan.audio.iter().any(|t| t.source_index == h.index && t.action == StreamAction::Copy));
    let hq_encoded =
        hq.is_some_and(|h| plan.audio.iter().any(|t| t.source_index == h.index && t.action == StreamAction::Encode));
    let has_compat = plan.audio.iter().any(|t| t.role == TrackRole::Compat);
    if hq.is_some_and(|h| h.atmos) && hq_copied {
        out.add(
            "音频",
            "Atmos 原样保留",
            format!(
                "全景声无法重新编码（需要杜比商业授权），只能原样复制{}",
                if has_compat { "；已额外生成兼容轨供手机和耳机使用" } else { "" }
            ),
        );
    } else if hq.is_some_and(|h| h.atmos) && hq_encoded && !hq_copied {
        out.push(
            "音频",
            "Atmos 转为兼容格式",
            "全景声元数据会丢失，只保留声道混音。若要保留，请在保真度里勾选「全景声与无损音轨」",
            Severity::Warn,
        );
    }
    if plan.loudnorm {
        let n = plan.audio.iter().filter(|t| t.action == StreamAction::Encode).count();
        if n == 0 {
            out.push(
                "响度",
                "无法标准化",
                "音轨都是原样复制，无法调整响度。要标准化，把音频改为“转为兼容格式”或追加兼容轨",
                Severity::Warn,
            );
        } else {
            out.add(
                "响度",
                format!("{} LUFS（两遍）", super::text::plain(super::loudness::TARGET_I)),
                format!(
                    "先测量整段响度再线性调整，避免单遍处理开头几秒音量爬升；作用于 {n} 条重新编码的音轨，原样复制的音轨不变"
                ),
            );
        }
    }
    if plan.audio.iter().any(|t| t.role == TrackRole::Compat && t.channels == Some(2))
        && media.audio.iter().any(|a| a.channels > 2)
    {
        out.add("降混", "中置 +3dB", "多声道降为立体声时提升中置声道，对白更清楚，并用限幅器防止削波");
    }

    // ── 容器 ──
    match plan.container {
        Container::Mkv => out.add("容器", "MKV", "能完整容纳杜比视界、无损音轨、图形字幕与章节"),
        Container::Mp4 => {
            out.add("容器", "MP4", "兼容性最好；已加 faststart 便于边下边播，HEVC 标记为 hvc1 以兼容苹果设备")
        }
        Container::Mov => out.add("容器", "MOV", "剪辑软件最友好的容器"),
    }
    out.0
}
