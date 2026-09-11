//! 界面需要的静态规则表：各编码器的质量刻度、preset、支持的码率控制，标准帧率档，码率输入范围。
//!
//! 前端只读这张表（经 WASM 取得），不再自己维护第二份规则。

use serde::Serialize;
use ts_rs::TS;

use crate::model::{Codec, EncoderId, MediaInfo, QualityTier, RateControl, RateControlKind, Vendor};

use super::encoders::{preset_options, quality_meta, quality_value, supports_rate_control, writes_hdr10};
use super::fps::{STANDARD_FPS, is_extreme_vfr, recommend_cfr_target};
use super::strategy::{MAX_KBPS, MIN_KBPS};

/// 四个质量档位在某个编码器上的原生数值
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct QualityValues {
    pub lossless: i32,
    pub high: i32,
    pub standard: i32,
    pub small: i32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EncoderMeta {
    pub id: EncoderId,
    pub vendor: Vendor,
    pub codec: Codec,
    pub hardware: bool,
    /// 质量参数名：CRF / global_quality / CQ / QP / q:v
    pub param: String,
    pub min: i32,
    pub max: i32,
    pub lower_is_better: bool,
    pub presets: Vec<String>,
    pub quality: QualityValues,
    pub rate_controls: Vec<RateControlKind>,
    /// 会把 HDR10 静态元数据写进码流
    pub writes_hdr10: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct StandardFpsMeta {
    pub value: f64,
    pub arg: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineMeta {
    pub encoders: Vec<EncoderMeta>,
    pub standard_fps: Vec<StandardFpsMeta>,
    pub min_kbps: u32,
    pub max_kbps: u32,
}

/// 帧率控件需要的、依赖素材的建议
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VideoHints {
    /// 打开"转为固定帧率"时的推荐目标
    pub recommended_fps: f64,
    /// 帧率波动剧烈（录屏类），转 CFR 会复制大量帧
    pub extreme_vfr: bool,
}

fn sample_of(kind: RateControlKind) -> RateControl {
    match kind {
        RateControlKind::Quality => RateControl::Quality,
        RateControlKind::Bitrate => RateControl::Bitrate { kbps: MIN_KBPS },
        RateControlKind::Capped => RateControl::Capped { kbps: MIN_KBPS },
        RateControlKind::TwoPass => RateControl::TwoPass { kbps: MIN_KBPS },
    }
}

pub fn engine_meta() -> EngineMeta {
    let encoders = EncoderId::ALL
        .into_iter()
        .map(|id| {
            let q = quality_meta(id);
            let v = |tier| quality_value(id, tier);
            EncoderMeta {
                id,
                vendor: id.vendor(),
                codec: id.codec(),
                hardware: id.is_hardware(),
                param: q.param.to_string(),
                min: q.min,
                max: q.max,
                lower_is_better: q.lower_is_better,
                presets: preset_options(id).iter().map(|s| s.to_string()).collect(),
                quality: QualityValues {
                    lossless: v(QualityTier::Lossless),
                    high: v(QualityTier::High),
                    standard: v(QualityTier::Standard),
                    small: v(QualityTier::Small),
                },
                rate_controls: RateControlKind::ALL
                    .into_iter()
                    .filter(|&k| supports_rate_control(id, sample_of(k)))
                    .collect(),
                writes_hdr10: writes_hdr10(id),
            }
        })
        .collect();
    let standard_fps = STANDARD_FPS
        .iter()
        .map(|s| StandardFpsMeta { value: s.value, arg: s.arg.to_string(), label: s.label.to_string() })
        .collect();
    EngineMeta { encoders, standard_fps, min_kbps: MIN_KBPS, max_kbps: MAX_KBPS }
}

pub fn video_hints(media: &MediaInfo) -> Option<VideoHints> {
    let v = media.video.first()?;
    Some(VideoHints { recommended_fps: recommend_cfr_target(v), extreme_vfr: is_extreme_vfr(v) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_covers_every_encoder_and_matches_the_rules() {
        let m = engine_meta();
        assert_eq!(m.encoders.len(), EncoderId::ALL.len());
        let x265 = m.encoders.iter().find(|e| e.id == EncoderId::Libx265).unwrap();
        assert_eq!((x265.param.as_str(), x265.quality.high), ("CRF", 20));
        assert_eq!(x265.rate_controls.len(), 4);
        let vt = m.encoders.iter().find(|e| e.id == EncoderId::HevcVideotoolbox).unwrap();
        assert_eq!(vt.rate_controls, [RateControlKind::Quality, RateControlKind::Bitrate]);
        assert!(!vt.lower_is_better && !vt.writes_hdr10);
        assert_eq!(m.standard_fps[3].arg, "30000/1001");
    }
}
