//! "为什么这么选"（需求 F-2.3）。依据当前计划推导，而不是记录推荐时的理由，用户改了参数后理由会同步更新。
//! 文字按界面语言生成（需求 F-9.1）。

use crate::i18n::{Lang, pick};
use crate::model::{
    Capabilities, Codec, Container, Decision, DoviAction, Estimate, FpsInsight, FpsPolicy, HdrAction, HdrKind,
    MediaInfo, QualityTier, RateControl, Scenario, Severity, StreamAction, ToneMapPipeline, TrackRole, TranscodePlan,
};
use crate::tr;

use super::args::{ExtraArgsIssue, display_size, downmix_is_tuned, extra_args_issue, split_args, target_dimensions};
use super::encoders::{Family, codec_available, family, quality_meta, software_usable};
use super::estimate::source_video_bps;
use super::fps::{is_extreme_vfr, recommend_cfr_target};
use super::strategy::{MAX_KBPS, MIN_KBPS, max_target_kbps, prefer_hw_for, scenario_codec};
use super::text::{format_bitrate, format_fps, format_percent, plain, thousands};

pub fn tier_label(t: QualityTier, lang: Lang) -> &'static str {
    match t {
        QualityTier::Lossless => pick(lang, "视觉无损", "Visually lossless"),
        QualityTier::High => pick(lang, "高画质", "High"),
        QualityTier::Standard => pick(lang, "标准", "Standard"),
        QualityTier::Small => pick(lang, "小体积", "Small"),
    }
}

fn tonemap_label(p: ToneMapPipeline, lang: Lang) -> &'static str {
    match p {
        ToneMapPipeline::Libplacebo => "libplacebo",
        ToneMapPipeline::TonemapOpencl => "OpenCL",
        ToneMapPipeline::Zscale => pick(lang, "zscale（CPU）", "zscale (CPU)"),
        ToneMapPipeline::ScaleVt => "VideoToolbox",
    }
}

struct Out {
    lang: Lang,
    items: Vec<Decision>,
}

impl Out {
    /// `field` 是 (中文, 英文) 两个名字
    fn add(&mut self, field: (&str, &str), value: impl Into<String>, reason: impl Into<String>) {
        self.push(field, value, reason, Severity::Info);
    }
    fn push(&mut self, field: (&str, &str), value: impl Into<String>, reason: impl Into<String>, severity: Severity) {
        let field = pick(self.lang, field.0, field.1).to_string();
        self.items.push(Decision { field, value: value.into(), reason: reason.into(), severity });
    }
}

const VIDEO: (&str, &str) = ("视频", "Video");
const CONTAINER: (&str, &str) = ("容器", "Container");
const ENCODER: (&str, &str) = ("编码器", "Encoder");
const CODEC: (&str, &str) = ("编码格式", "Format");
const QUALITY: (&str, &str) = ("画质", "Quality");
const BITRATE: (&str, &str) = ("码率", "Bitrate");
const DEPTH: (&str, &str) = ("位深", "Bit depth");
const HDR: (&str, &str) = ("HDR", "HDR");
const DV: (&str, &str) = ("杜比视界", "Dolby Vision");
const RESOLUTION: (&str, &str) = ("分辨率", "Resolution");
const FPS: (&str, &str) = ("帧率", "Frame rate");
const GOP: (&str, &str) = ("关键帧", "Keyframes");
const TIME: (&str, &str) = ("耗时", "Time");
const AUDIO: (&str, &str) = ("音频", "Audio");
const LOUDNESS: (&str, &str) = ("响度", "Loudness");
const DOWNMIX: (&str, &str) = ("降混", "Downmix");
const EXTRA: (&str, &str) = ("附加参数", "Extra arguments");
const COVER: (&str, &str) = ("封面", "Cover art");

/// 源文件带封面图时的提醒：现在的命令不带封面（封面是一条特殊的视频流），完成后的校验会标出
fn cover_note(out: &mut Out, media: &MediaInfo, lang: Lang) {
    if let Some(n) = media.covers.filter(|n| *n > 0) {
        let reason = tr!(
            lang,
            "源文件带 {} 张封面图，输出不会保留：封面图是一条特殊的视频流，这一版还没有处理。完成后的校验会标出这一项",
            "The source has {} cover image(s) that the output will not keep: cover art is a special video stream this version does not handle yet. Verification will flag it",
            n
        );
        out.push(COVER, pick(lang, "不保留", "Not kept"), reason, Severity::Warn);
    }
}

pub fn explain(
    media: &MediaInfo,
    plan: &TranscodePlan,
    caps: &Capabilities,
    fps: Option<&FpsInsight>,
    est: Option<&Estimate>,
    lang: Lang,
) -> Vec<Decision> {
    let l = |zh: &'static str, en: &'static str| pick(lang, zh, en);
    let mut out = Out { lang, items: Vec::new() };
    let Some(v) = media.video.first() else { return out.items };
    let vp = &plan.video;
    let mkv_reason = l(
        "能完整容纳杜比视界、无损音轨、图形字幕与章节",
        "Holds Dolby Vision, lossless audio, image subtitles and chapters without loss",
    );

    if vp.action == StreamAction::Copy {
        out.add(
            VIDEO,
            l("原样复制", "Copied as-is"),
            l(
                "不重新编码，画质零损失，速度只受磁盘读写限制",
                "No re-encoding: zero quality loss, limited only by disk speed",
            ),
        );
        out.add(CONTAINER, plan.container.ext().to_uppercase(), mkv_reason);
        cover_note(&mut out, media, lang);
        return out.items;
    }

    // ── 编码器 ──
    let vendor = vp.encoder.vendor();
    let encoder = vp.encoder.name();
    let encoder_ok = caps.encoder_usable(vp.encoder);
    if caps.status == crate::model::EnvStatus::Ready && !encoder_ok {
        let reason = tr!(
            lang,
            "当前 ffmpeg 没有任何可用的 {} 编码器，这个计划无法执行",
            "This ffmpeg has no usable {} encoder, so the plan cannot run",
            vp.codec.label()
        );
        out.push(ENCODER, encoder, reason, Severity::Warn);
    } else if !vp.encoder_auto {
        out.add(
            ENCODER,
            encoder,
            l("你手动指定了编码器，自动选择已关闭", "You picked the encoder manually; automatic selection is off"),
        );
    } else if vp.encoder.is_hardware() && !software_usable(vp.codec, caps) {
        let reason = tr!(
            lang,
            "当前 ffmpeg 没有 {} 软件编码器，改用 {}；同画质下体积会大 15–30%",
            "This ffmpeg has no {} software encoder, so {} is used; files are 15–30% larger at the same quality",
            vp.codec.label(),
            vendor.label()
        );
        out.push(ENCODER, encoder, reason, Severity::Warn);
    } else if vp.dovi == DoviAction::Preserve {
        out.add(
            ENCODER,
            encoder,
            l(
                "杜比视界的逐帧元数据只能由软件编码器写入，因此本次不使用 GPU 编码",
                "Only software encoders can write per-frame Dolby Vision metadata, so the GPU is not used",
            ),
        );
    } else if vp.encoder.is_hardware() {
        let reason = tr!(
            lang,
            "使用 {}（启动时已真实试编码验证可用），速度约为软编的 5–10 倍；同画质下体积会大 15–30%",
            "Uses {} (verified by a real test encode at startup), about 5–10× faster than software; files are 15–30% larger at the same quality",
            vendor.label()
        );
        out.add(ENCODER, encoder, reason);
    } else if prefer_hw_for(plan.scenario) {
        let reason = tr!(
            lang,
            "没有可用的 {} 硬件编码器，已改用软件编码",
            "No usable {} hardware encoder; using software encoding",
            vp.codec.label()
        );
        out.push(ENCODER, encoder, reason, Severity::Warn);
    } else if plan.scenario == Scenario::Editing {
        out.add(
            ENCODER,
            encoder,
            l(
                "剪辑素材要经得起调色与二次导出，用软件编码保证画质；硬件编码的码率控制不够稳定",
                "Editing footage must survive grading and re-export, so software encoding keeps the quality; hardware rate control is less stable",
            ),
        );
    } else {
        out.add(
            ENCODER,
            encoder,
            l(
                "软件编码在同体积下画质最好，适合长期保存；硬件编码更快但同画质体积更大",
                "Software encoding gives the best quality per size, ideal for keeping; hardware is faster but larger at the same quality",
            ),
        );
    }

    // ── 编码格式 ──
    let wanted = scenario_codec(plan.scenario, media);
    if vp.codec != wanted && !codec_available(wanted, caps) {
        let reason = tr!(
            lang,
            "当前 ffmpeg 没有可用的 {} 编码器，已改用 {}",
            "This ffmpeg has no usable {} encoder; using {} instead",
            wanted.label(),
            vp.codec.label()
        );
        out.push(CODEC, vp.codec.label(), reason, Severity::Warn);
    } else if plan.scenario == Scenario::Editing {
        let reason = if vp.codec == Codec::H264 {
            l(
                "H.264 在各剪辑软件中解码最流畅，时间线拖动不卡",
                "H.264 decodes most smoothly in editors, so scrubbing stays fluid",
            )
        } else {
            l(
                "源为 HDR，用 HEVC 10bit 才能保留 HDR；达芬奇与 Final Cut 均支持",
                "The source is HDR, which needs HEVC 10-bit; DaVinci Resolve and Final Cut both support it",
            )
        };
        out.add(CODEC, vp.codec.label(), reason);
    } else if vp.codec == Codec::Hevc && v.codec == "h264" {
        out.add(
            CODEC,
            "HEVC",
            l(
                "同画质下 HEVC 比 H.264 体积小约 40–50%，2016 年后的设备普遍能硬解播放",
                "HEVC is about 40–50% smaller than H.264 at the same quality; most devices since 2016 decode it in hardware",
            ),
        );
    } else if vp.codec == Codec::Av1 {
        out.push(
            CODEC,
            "AV1",
            l(
                "AV1 比 HEVC 再省约 20–30%，但 2020 年前的设备多数无法硬解播放",
                "AV1 saves another 20–30% over HEVC, but most devices from before 2020 cannot decode it in hardware",
            ),
            Severity::Tip,
        );
    } else if vp.codec == Codec::H264 {
        out.add(
            CODEC,
            "H.264",
            l(
                "H.264 兼容性最好，几乎所有设备和平台都能直接播放",
                "H.264 is the most compatible; nearly every device and platform plays it",
            ),
        );
    }

    // ── 质量与码率 ──
    let meta = quality_meta(vp.encoder);
    let quality = format!("{} · {} {}", tier_label(vp.quality, lang), meta.param, vp.quality_value);
    let mbps = |kbps: u32| format_bitrate(f64::from(kbps) * 1000.0);
    // 目标码率被源视频码率封顶时补一句原因（不升档）
    let top = max_target_kbps(media);
    let source_note = |kbps: u32, reason: &str| -> Option<String> {
        (kbps >= top && top > MIN_KBPS && top < MAX_KBPS).then(|| {
            tr!(
                lang,
                "{}。目标码率不高于源视频码率（{}）：再高只会多占空间，画质不会更好",
                "{}. The target never exceeds the source video bitrate ({}): a higher bitrate only takes more space without improving quality",
                reason,
                format_bitrate(source_video_bps(media))
            )
        })
    };
    match vp.rate_control {
        RateControl::Quality => {
            let reason = if meta.lower_is_better {
                tr!(
                    lang,
                    "{} 越小画质越好。不同编码器的数值刻度不等价，档位已按编码器分别换算",
                    "Lower {} means better quality. Scales differ between encoders, so each tier is mapped per encoder",
                    meta.param
                )
            } else {
                tr!(
                    lang,
                    "{} 越大画质越好。不同编码器的数值刻度不等价，档位已按编码器分别换算",
                    "Higher {} means better quality. Scales differ between encoders, so each tier is mapped per encoder",
                    meta.param
                )
            };
            out.add(QUALITY, quality, reason);
        }
        RateControl::Capped { kbps } => {
            let value = tr!(lang, "{}，峰值 ≤ {}", "{}, peak ≤ {}", quality, mbps(kbps));
            let reason = if family(vp.encoder) == Family::Qsv {
                tr!(
                    lang,
                    "QSV 用 QVBR 实现：按质量编码，平均码率约 {}，峰值不超过上限",
                    "QSV implements this with QVBR: quality-based encoding averaging about {}, with peaks under the cap",
                    mbps(kbps * 2 / 3)
                )
            } else {
                l(
                    "按质量编码，同时限制峰值码率，网络串流时不易卡顿；复杂画面会略降画质以守住上限",
                    "Encodes by quality while capping the peak bitrate, so streaming stalls less; complex scenes lose a little quality to stay under the cap",
                )
                .to_string()
            };
            out.add(QUALITY, value, reason);
        }
        RateControl::Bitrate { kbps } => {
            let value = tr!(lang, "平均 {}", "Average {}", mbps(kbps));
            let reason = l(
                "按目标码率编码，体积可预测；画面复杂的片段画质会下降。追求画质稳定用恒定质量",
                "Encodes to a target bitrate so the size is predictable; complex scenes lose quality. Use constant quality for consistent quality",
            );
            match source_note(kbps, reason) {
                Some(note) => out.push(BITRATE, value, note, Severity::Tip),
                None => out.add(BITRATE, value, reason),
            }
        }
        RateControl::TwoPass { kbps } => {
            let value = tr!(lang, "两遍 · 平均 {}", "Two-pass · average {}", mbps(kbps));
            let reason = l(
                "第一遍分析全片复杂度，第二遍按目标码率分配，体积准确、画质比单遍按码率编码更均匀；耗时约 1.7 倍",
                "The first pass analyzes the whole video and the second distributes the bitrate, giving an accurate size and more even quality than single-pass bitrate mode; takes about 1.7× as long",
            );
            match source_note(kbps, reason) {
                Some(note) => out.push(BITRATE, value, note, Severity::Tip),
                None => out.add(BITRATE, value, reason),
            }
        }
    }

    // ── 位深 ──
    if vp.bit_depth == 10 {
        if v.color.hdr_kind != HdrKind::None && vp.hdr_action == HdrAction::Keep {
            out.add(
                DEPTH,
                "10bit",
                l(
                    "HDR 必须 10bit，8bit 会在天空与暗部出现明显色带",
                    "HDR needs 10-bit; 8-bit shows visible banding in skies and shadows",
                ),
            );
        } else if v.bit_depth == 8 {
            out.add(
                DEPTH,
                "10bit",
                l(
                    "8bit 源用 10bit 编码能减少渐变处的色带，体积几乎不变",
                    "Encoding an 8-bit source in 10-bit reduces banding in gradients at almost no size cost",
                ),
            );
        }
    }

    // ── HDR ──
    if v.color.hdr_kind != HdrKind::None {
        let kind = if v.color.hdr_kind == HdrKind::Hlg { "HLG" } else { "HDR10" };
        let keep = tr!(lang, "保留 {}", "Keep {}", kind);
        let wants_sdr = matches!(plan.scenario, Scenario::Mobile | Scenario::Social);
        if vp.hdr_action == HdrAction::Keep && wants_sdr && caps.pick_tonemap().is_none() {
            out.push(
                HDR,
                tr!(lang, "无法转为 SDR，保留 {}", "Cannot convert to SDR; keeping {}", kind),
                l(
                    "当前 ffmpeg 没有任何可用的色调映射滤镜（libplacebo / OpenCL / zscale）。在普通屏幕上可能发灰，建议换用带 libplacebo 或 zscale 的构建",
                    "This ffmpeg has no usable tone mapping filter (libplacebo / OpenCL / zscale). The video may look washed out on SDR screens; use a build with libplacebo or zscale",
                ),
                Severity::Warn,
            );
        } else if vp.hdr_action == HdrAction::Tonemap {
            let pipe = tonemap_label(vp.tonemap.unwrap_or(ToneMapPipeline::Libplacebo), lang);
            let reason = if v.dolby_vision.is_some() && vp.tonemap == Some(ToneMapPipeline::Libplacebo) {
                tr!(
                    lang,
                    "源为 {}，目标多为 SDR 屏幕。使用 {} 做色调映射，并利用杜比视界元数据提升映射准确度，避免画面发灰",
                    "The source is {} and most target screens are SDR. {} tone maps it, using the Dolby Vision metadata for accuracy, so it does not look washed out",
                    kind,
                    pipe
                )
            } else {
                tr!(
                    lang,
                    "源为 {}，目标多为 SDR 屏幕。使用 {} 做色调映射，避免画面发灰",
                    "The source is {} and most target screens are SDR. {} tone maps it so it does not look washed out",
                    kind,
                    pipe
                )
            };
            out.add(HDR, l("色调映射为 SDR", "Tone map to SDR"), reason);
        } else if vp.encoder.is_hardware() && kind == "HDR10" {
            let reason = tr!(
                lang,
                "{} 会把 HDR10 元数据写入码流，已在本机实测验证",
                "{} writes HDR10 metadata into the stream (verified on real hardware)",
                vendor.label()
            );
            out.add(HDR, keep, reason);
        } else {
            let reason = if kind == "HLG" {
                l(
                    "保留 HLG 色彩标记，HDR 电视与手机可直接识别",
                    "Keeps the HLG color tags, which HDR TVs and phones recognize directly",
                )
            } else {
                l(
                    "母版显示与 MaxCLL 元数据由 ffmpeg 自动透传",
                    "ffmpeg passes the mastering display and MaxCLL metadata through automatically",
                )
            };
            out.add(HDR, keep, reason);
        }
    }

    // ── 杜比视界 ──
    if let Some(dv) = &v.dolby_vision {
        if dv.has_enhancement_layer {
            let reason = tr!(
                lang,
                "源为 Profile {} 双层。ffmpeg 无法编码增强层，重编码后只剩 HDR10 基础层。要完整保留，请改为「原样封装」",
                "The source is dual-layer Profile {}. ffmpeg cannot encode the enhancement layer, so re-encoding keeps only the HDR10 base layer. Use Remux to keep everything",
                dv.profile
            );
            out.push(DV, l("仅保留基础层", "Base layer only"), reason, Severity::Warn);
        } else if vp.dovi == DoviAction::Preserve {
            if dv.profile == 5 {
                out.push(
                    DV,
                    tr!(lang, "保留 Profile 5", "Keep Profile 5"),
                    l(
                        "Profile 5 没有 HDR10 回退层，不支持杜比视界的设备会显示偏绿或偏紫",
                        "Profile 5 has no HDR10 fallback layer; devices without Dolby Vision show green or purple tints",
                    ),
                    Severity::Warn,
                );
            } else {
                out.add(
                    DV,
                    tr!(lang, "保留 Profile {}.{}", "Keep Profile {}.{}", dv.profile, dv.bl_compat_id),
                    l(
                        "传入 -dolbyvision 1，保留失败时会明确报错而不是静默丢弃",
                        "Passes -dolbyvision 1 so a failure is reported instead of silently dropping it",
                    ),
                );
            }
        } else {
            let base = if v.color.hdr_kind == HdrKind::Hlg { "HLG" } else { "HDR10" };
            let reason = tr!(
                lang,
                "已显式传入 -dolbyvision 0（ffmpeg 默认会自动开启），输出将以 {} 播放",
                "Passes -dolbyvision 0 explicitly (ffmpeg enables it automatically by default); the output plays as {}",
                base
            );
            out.add(DV, l("不保留", "Not kept"), reason);
        }
    }

    // ── 分辨率 ──
    let (sw, sh) = display_size(v);
    if let Some(d) = target_dimensions(v, vp.resolution) {
        let reason = tr!(
            lang,
            "从 {}×{} 缩小，使用 lanczos 保留细节",
            "Downscaled from {}×{} with lanczos to keep detail",
            sw,
            sh
        );
        out.add(RESOLUTION, format!("{}×{}", d.w, d.h), reason);
    } else if vp.resolution != crate::model::ResolutionPreset::Source {
        let reason = tr!(
            lang,
            "目标分辨率不低于源（{}×{}），不做放大",
            "The target is not below the source ({}×{}), so nothing is upscaled",
            sw,
            sh
        );
        out.push(RESOLUTION, l("保持原始", "Unchanged"), reason, Severity::Tip);
    }

    // ── 帧率 ──
    match (vp.fps, fps) {
        (FpsPolicy::Cfr { .. }, Some(fps)) => {
            let target = format_fps(fps.target_fps);
            let value = tr!(lang, "{} fps 固定", "Constant {} fps", target);
            let source = recommend_cfr_target(v);
            if fps.dropped > 0 && fps.target_fps < source * (1.0 - 1e-9) {
                let reason = tr!(
                    lang,
                    "从源的 {} fps 降到 {} fps，约丢弃 {} 帧：体积更小、编码更快，但运动画面不如原来流畅",
                    "Lowered from the source's {} fps to {} fps, dropping about {} frames: smaller and faster to encode, but motion is less smooth",
                    format_fps(source),
                    target,
                    thousands(fps.dropped)
                );
                out.add(FPS, value, reason);
            } else if v.is_vfr && is_extreme_vfr(v) {
                let pct = format_percent(fps.duplicated as f64 / fps.target_frames.max(1) as f64);
                let reason = tr!(
                    lang,
                    "源平均仅 {} fps，会复制 {} 帧（占 {}）。重复帧几乎不占体积，但编码更慢；剪辑也可降到 30 fps",
                    "The source averages only {} fps, so {} frames are duplicated ({}). Duplicates take almost no space but slow encoding; 30 fps is also fine for editing",
                    format_fps(v.fps_avg),
                    thousands(fps.duplicated),
                    pct
                );
                out.push(FPS, value, reason, Severity::Warn);
            } else if v.is_vfr {
                let reason = tr!(
                    lang,
                    "导入剪辑软件不会逐渐音画错位；复制约 {} 帧补齐时间轴",
                    "Audio stays in sync in editors; about {} frames are duplicated to fill the timeline",
                    thousands(fps.duplicated)
                );
                out.add(FPS, value, reason);
            } else {
                out.add(
                    FPS,
                    value,
                    l(
                        "源本身已是固定帧率，输出保持一致",
                        "The source is already constant frame rate; the output matches",
                    ),
                );
            }
        }
        (FpsPolicy::Keep, _) if v.is_vfr => {
            out.add(
                FPS,
                l("保持可变帧率", "Keep variable"),
                l(
                    "保持原始时间戳，播放没有问题；之后要剪辑的话，在帧率里打开“转为固定帧率”",
                    "Keeps the original timestamps, which play fine; turn on \"Convert to constant frame rate\" if you plan to edit",
                ),
            );
        }
        _ => {}
    }

    if let Some(gop) = vp.gop {
        out.add(
            GOP,
            tr!(lang, "每 {} 帧", "Every {} frames", gop),
            l(
                "关键帧间隔约 0.5 秒，剪辑软件拖动时间线更流畅，代价是体积略增",
                "A keyframe about every 0.5 s makes scrubbing smoother in editors at a slightly larger size",
            ),
        );
    }

    // ── 附加参数 ──
    if let Some(issue) = vp.extra_args.as_deref().and_then(|s| extra_args_issue(&split_args(s))) {
        let reason = match issue {
            ExtraArgsIssue::Output(arg) => tr!(
                lang,
                "「{}」不属于任何选项，ffmpeg 会把它当成又一个输出文件，并直接覆盖同名文件。这段附加参数没有使用；值里有空格时请加引号",
                "'{}' belongs to no option, so ffmpeg would treat it as another output file and overwrite any file with that name. The extra arguments are not used; quote values that contain spaces",
                arg
            ),
            ExtraArgsIssue::Managed(arg) => tr!(
                lang,
                "{} 由队列管理（输入、覆盖确认与进度），不能写在附加参数里。这段附加参数没有使用",
                "{} is managed by the queue (input, overwrite and progress) and cannot go in the extra arguments. The extra arguments are not used",
                arg
            ),
        };
        out.push(EXTRA, l("未使用", "Not used"), reason, Severity::Warn);
    }

    // ── 耗时 ──
    const LONG_ENCODE_SEC: f64 = 3.0 * 3600.0;
    if let Some(est) = est.filter(|e| !vp.encoder.is_hardware() && e.time_max_sec > LONG_ENCODE_SEC) {
        let slow = matches!(vp.preset.as_str(), "slow" | "slower" | "veryslow");
        let hours = plain((est.time_max_sec / 3600.0).round());
        let reason = if slow {
            tr!(
                lang,
                "CPU 以 {} 速度编码 {}×{} 很慢。不在意极致压缩率的话，可在“更多参数”里改为 medium，速度约快 2 倍，体积仅增加 5% 左右",
                "Encoding {1}×{2} on the CPU at preset {0} is slow. If you do not need maximum compression, switch to medium under More options: about 2× faster for roughly 5% more size",
                vp.preset,
                v.width,
                v.height
            )
        } else {
            l(
                "CPU 编码高分辨率长片耗时较长，可以放在队列里夜间运行",
                "CPU encoding of long high-resolution videos takes a while; let the queue run overnight",
            )
            .to_string()
        };
        out.push(TIME, tr!(lang, "可能超过 {} 小时", "May take over {} h", hours), reason, Severity::Tip);
    }

    // ── 音频 ──
    let hq = media.audio.iter().find(|a| a.atmos || a.lossless);
    let hq_copied =
        hq.is_some_and(|h| plan.audio.iter().any(|t| t.source_index == h.index && t.action == StreamAction::Copy));
    let hq_encoded =
        hq.is_some_and(|h| plan.audio.iter().any(|t| t.source_index == h.index && t.action == StreamAction::Encode));
    let has_compat = plan.audio.iter().any(|t| t.role == TrackRole::Compat);
    if hq.is_some_and(|h| h.atmos) && hq_copied {
        let reason = if has_compat {
            l(
                "全景声无法重新编码（需要杜比商业授权），只能原样复制；已额外生成兼容轨供手机和耳机使用",
                "Atmos cannot be re-encoded (it needs a Dolby license), so it is copied as-is; a compatible track is added for phones and headphones",
            )
        } else {
            l(
                "全景声无法重新编码（需要杜比商业授权），只能原样复制",
                "Atmos cannot be re-encoded (it needs a Dolby license), so it is copied as-is",
            )
        };
        out.add(AUDIO, l("Atmos 原样保留", "Atmos kept as-is"), reason);
    } else if hq.is_some_and(|h| h.atmos) && hq_encoded && !hq_copied {
        out.push(
            AUDIO,
            l("Atmos 转为兼容格式", "Atmos converted"),
            l(
                "全景声元数据会丢失，只保留声道混音。若要保留，请在保真度里勾选「全景声与无损音轨」",
                "The Atmos metadata is lost and only the channel mix remains. To keep it, check \"Atmos & lossless audio\" under Fidelity",
            ),
            Severity::Warn,
        );
    }
    if plan.loudnorm {
        let n = plan.audio.iter().filter(|t| t.action == StreamAction::Encode).count();
        if n == 0 {
            out.push(
                LOUDNESS,
                l("无法标准化", "Cannot normalize"),
                l(
                    "音轨都是原样复制，无法调整响度。要标准化，把音频改为“转为兼容格式”或追加兼容轨",
                    "All audio tracks are copied, so the loudness cannot change. To normalize, convert the audio or add a compatible track",
                ),
                Severity::Warn,
            );
        } else {
            let reason = tr!(
                lang,
                "先测量整段响度再线性调整，避免单遍处理开头几秒音量爬升；作用于 {} 条重新编码的音轨，原样复制的音轨不变",
                "Measures the whole program first and then adjusts linearly, avoiding the volume ramp of single-pass processing; applies to {} re-encoded track(s), copied tracks are untouched",
                n
            );
            let target = plain(super::loudness::TARGET_I);
            out.add(LOUDNESS, tr!(lang, "{} LUFS（两遍）", "{} LUFS (two-pass)", target), reason);
        }
    }
    let downmixed = plan
        .audio
        .iter()
        .filter(|t| t.action == StreamAction::Encode && t.channels == Some(2))
        .find_map(|t| media.audio.iter().find(|a| a.index == t.source_index && a.channels > 2));
    if let Some(src) = downmixed {
        if downmix_is_tuned(src) {
            out.add(
                DOWNMIX,
                l("中置 +3dB", "Center +3 dB"),
                l(
                    "多声道降为立体声时提升中置声道，对白更清楚，并用限幅器防止削波",
                    "Boosts the center channel when downmixing to stereo so dialogue is clearer, with a limiter to prevent clipping",
                ),
            );
        } else {
            let layout =
                if src.channel_layout.is_empty() { format!("{}ch", src.channels) } else { src.channel_layout.clone() };
            let reason = tr!(
                lang,
                "{} 声道布局没有专门调过的降混系数，用 ffmpeg 的默认矩阵，每个声道都会混进立体声",
                "The {} layout has no tuned downmix, so ffmpeg's default matrix is used and every channel is mixed into stereo",
                layout
            );
            out.add(DOWNMIX, l("默认矩阵", "Default matrix"), reason);
        }
    }

    cover_note(&mut out, media, lang);

    // ── 容器 ──
    match plan.container {
        Container::Mkv => out.add(CONTAINER, "MKV", mkv_reason),
        Container::Mp4 => out.add(
            CONTAINER,
            "MP4",
            l(
                "兼容性最好；已加 faststart 便于边下边播，HEVC 标记为 hvc1 以兼容苹果设备",
                "The most compatible; faststart allows playback while downloading and HEVC is tagged hvc1 for Apple devices",
            ),
        ),
        Container::Mov => out.add(CONTAINER, "MOV", l("剪辑软件最友好的容器", "The container editors like best")),
    }
    out.items
}
