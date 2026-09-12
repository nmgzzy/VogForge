//! 体积与耗时的范围预估（需求 F-2.6）。经验模型：每像素每帧比特数 × 运动量 × 各种修正。

use crate::model::{
    Codec, EncoderId, Estimate, FpsPolicy, HdrAction, HdrKind, MediaInfo, QualityTier, RateControl, Scenario,
    SourceHint, StreamAction, ToneMapPipeline, TranscodePlan, Vendor,
};

use super::args::target_dimensions;
use super::fps::fps_insight;

/// 每像素每帧的比特数（bpp），按编码格式与质量档位的经验值
fn bpp(codec: Codec, tier: QualityTier) -> f64 {
    use QualityTier::*;
    let t = |lossless, high, standard, small| match tier {
        Lossless => lossless,
        High => high,
        Standard => standard,
        Small => small,
    };
    match codec {
        Codec::Hevc => t(0.11, 0.065, 0.042, 0.026),
        Codec::H264 => t(0.18, 0.11, 0.07, 0.045),
        Codec::Av1 => t(0.085, 0.05, 0.032, 0.019),
    }
}

/// 画面运动量对码率的影响
fn motion(hint: SourceHint) -> f64 {
    match hint {
        SourceHint::Gopro => 1.35,
        SourceHint::Dji => 1.3,
        SourceHint::Camera | SourceHint::Bluray => 1.1,
        SourceHint::Screen => 0.45,
        _ => 1.0,
    }
}

/// 1080p30 下的实时倍速
fn base_speed(encoder: EncoderId, preset: &str) -> f64 {
    match encoder {
        EncoderId::Libx265 => match preset {
            "ultrafast" => 6.0,
            "superfast" => 5.0,
            "veryfast" => 4.0,
            "faster" => 3.0,
            "fast" => 2.4,
            "medium" => 1.5,
            "slow" => 0.65,
            "slower" => 0.28,
            "veryslow" => 0.14,
            _ => 1.5,
        },
        EncoderId::Libx264 => match preset {
            "ultrafast" => 20.0,
            "veryfast" => 12.0,
            "fast" => 8.0,
            "medium" => 5.5,
            "slow" => 3.0,
            "slower" => 1.6,
            "veryslow" => 0.9,
            _ => 5.0,
        },
        EncoderId::Libsvtav1 => {
            let p: f64 = preset.parse().unwrap_or(f64::NAN);
            if p >= 12.0 {
                9.0
            } else if p >= 10.0 {
                5.5
            } else if p >= 8.0 {
                3.2
            } else if p >= 6.0 {
                1.6
            } else if p >= 4.0 {
                0.55
            } else {
                0.18
            }
        }
        _ => match encoder.vendor() {
            Vendor::Intel => 11.0,
            Vendor::Nvidia => 16.0,
            Vendor::Amd => 12.0,
            _ => 9.0,
        },
    }
}

/// 源视频码率（bps）。流上没有记录时用整体码率：含音频、略偏高，只用作比较基准与上限
pub fn source_video_bps(media: &MediaInfo) -> f64 {
    media.video.first().and_then(|v| v.bitrate).map_or(media.bitrate as f64, |b| b as f64)
}

pub struct EstimateResult {
    pub estimate: Estimate,
    pub video_bps: f64,
    pub source_video_bps: f64,
}

pub fn estimate(media: &MediaInfo, plan: &TranscodePlan) -> EstimateResult {
    let v = media.video.first();
    let dur = media.duration_sec;
    let vp = &plan.video;
    let audio_bps = plan.audio.iter().fold(0.0, |sum, t| {
        if t.action == StreamAction::Copy {
            sum + media.audio.iter().find(|a| a.index == t.source_index).and_then(|a| a.bitrate).unwrap_or(192_000)
                as f64
        } else {
            sum + f64::from(t.bitrate_kbps.unwrap_or(192)) * 1000.0
        }
    });
    let source_video_bps = source_video_bps(media);

    let Some(v) = v.filter(|_| vp.action == StreamAction::Encode) else {
        let size = media.size_bytes as f64;
        return EstimateResult {
            estimate: Estimate {
                size_min: size * 0.98,
                size_max: size,
                time_min_sec: dur / 80.0,
                time_max_sec: dur / 30.0,
                ratio: 1.0,
                video_bps: source_video_bps,
            },
            video_bps: source_video_bps,
            source_video_bps,
        };
    };

    let dims = target_dimensions(v, vp.resolution);
    let (w, h) = dims.map_or((f64::from(v.width), f64::from(v.height)), |d| (f64::from(d.w), f64::from(d.h)));

    // 转 CFR 复制出的帧几乎零残差，按 3% 计入码率，而不是按帧数线性增长
    let mut eff_fps = v.fps_avg;
    if let FpsPolicy::Cfr { fps } = vp.fps {
        let ins = fps_insight(v, dur, fps);
        eff_fps = (ins.source_frames as f64 - ins.dropped as f64 + ins.duplicated as f64 * 0.03) / dur;
    }

    let mut bpp = bpp(vp.codec, vp.quality) * motion(media.source_hint);
    if vp.encoder.is_hardware() {
        bpp *= 1.3;
    }
    if vp.hdr_action == HdrAction::Keep && v.color.hdr_kind != HdrKind::None {
        bpp *= 1.1;
    }
    if vp.gop.is_some_and(|g| g < 30) {
        bpp *= 1.25;
    }

    let mut video_bps = bpp * w * h * eff_fps;
    // 重编码不会比源更大（CRF 模式下编码器会自然收敛），剪辑预处理除外
    if plan.scenario != Scenario::Editing {
        video_bps = video_bps.min(source_video_bps * 1.02);
    }
    // 按码率编码时体积由码率决定；限峰值时平均码率不会超过峰值。范围随之收窄
    let (mut lo, mut hi) = (0.72, 1.32);
    match vp.rate_control {
        RateControl::Quality => {}
        RateControl::Capped { kbps } => video_bps = video_bps.min(f64::from(kbps) * 1000.0),
        RateControl::Bitrate { kbps } => {
            video_bps = f64::from(kbps) * 1000.0;
            (lo, hi) = (0.88, 1.12);
        }
        RateControl::TwoPass { kbps } => {
            video_bps = f64::from(kbps) * 1000.0;
            (lo, hi) = (0.95, 1.05);
        }
    }

    let total = (video_bps + audio_bps) * dur / 8.0 * 1.01;
    let pixel_scale = (1920.0 * 1080.0 * 30.0) / (w * h * eff_fps.max(1.0));
    let mut speed = base_speed(vp.encoder, &vp.preset) * pixel_scale;
    if vp.hdr_action == HdrAction::Tonemap {
        speed *= if vp.tonemap == Some(ToneMapPipeline::Zscale) { 0.35 } else { 0.8 };
    }
    let mut t = dur / speed.max(0.01);
    // 第一遍只做分析、用快速设置，耗时约为第二遍的 0.7
    if matches!(vp.rate_control, RateControl::TwoPass { .. }) && !vp.encoder.is_hardware() {
        t *= 1.7;
    }

    EstimateResult {
        estimate: Estimate {
            size_min: total * lo,
            size_max: total * hi,
            time_min_sec: t * 0.75,
            time_max_sec: t * 1.4,
            ratio: total / media.size_bytes as f64,
            video_bps,
        },
        video_bps,
        source_video_bps,
    }
}
