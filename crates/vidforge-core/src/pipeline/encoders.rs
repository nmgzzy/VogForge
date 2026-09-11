//! 编码器相关的静态规则：质量档位映射、preset、10bit 与 HDR10 支持、自动选择。

use crate::model::{Capabilities, Codec, EncoderId, Platform, QualityTier, Scenario, Vendor};

/// 质量刻度按编码器家族区分，跨家族不等价（设计文档 6.2）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    X265,
    X264,
    SvtAv1,
    Qsv,
    Nvenc,
    Amf,
    Vt,
}

pub fn family(id: EncoderId) -> Family {
    match id {
        EncoderId::Libx265 => Family::X265,
        EncoderId::Libx264 => Family::X264,
        EncoderId::Libsvtav1 => Family::SvtAv1,
        _ => match id.vendor() {
            Vendor::Intel => Family::Qsv,
            Vendor::Nvidia => Family::Nvenc,
            Vendor::Amd => Family::Amf,
            _ => Family::Vt,
        },
    }
}

/// 质量档位到原生数值（CRF / global_quality / CQ / QP / q:v）
pub fn quality_value(id: EncoderId, tier: QualityTier) -> i32 {
    use QualityTier::*;
    let t = |lossless, high, standard, small| match tier {
        Lossless => lossless,
        High => high,
        Standard => standard,
        Small => small,
    };
    match family(id) {
        Family::X265 => t(16, 20, 23, 27),
        Family::X264 => t(15, 18, 21, 25),
        Family::SvtAv1 => t(20, 26, 32, 38),
        Family::Qsv => t(18, 21, 24, 28),
        Family::Nvenc => t(19, 23, 26, 30),
        Family::Amf => t(18, 21, 24, 28),
        // VideoToolbox 的 -q:v 数值越大越好
        Family::Vt => t(80, 68, 58, 48),
    }
}

pub struct QualityMeta {
    pub param: &'static str,
    pub min: i32,
    pub max: i32,
    pub lower_is_better: bool,
}

pub fn quality_meta(id: EncoderId) -> QualityMeta {
    let m = |param, min, max, lower_is_better| QualityMeta { param, min, max, lower_is_better };
    match family(id) {
        Family::X265 | Family::X264 => m("CRF", 0, 51, true),
        Family::SvtAv1 => m("CRF", 0, 63, true),
        Family::Qsv => m("global_quality", 1, 51, true),
        Family::Nvenc => m("CQ", 0, 51, true),
        Family::Amf => m("QP", 0, 51, true),
        Family::Vt => m("q:v", 1, 100, false),
    }
}

pub fn preset_options(id: EncoderId) -> &'static [&'static str] {
    match family(id) {
        Family::X265 | Family::X264 => {
            &["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"]
        }
        Family::SvtAv1 => &["2", "3", "4", "5", "6", "7", "8", "10", "12"],
        Family::Qsv => &["veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"],
        Family::Nvenc => &["p1", "p2", "p3", "p4", "p5", "p6", "p7"],
        Family::Amf => &["speed", "balanced", "quality"],
        Family::Vt => &["default"],
    }
}

pub fn default_preset(id: EncoderId, scenario: Scenario) -> &'static str {
    let slow = matches!(scenario, Scenario::Collection | Scenario::Archive);
    match family(id) {
        Family::X265 if scenario == Scenario::Collection => "slow",
        Family::X265 if slow => "medium",
        Family::X265 => "fast",
        Family::X264 if slow => "slow",
        Family::X264 => "medium",
        Family::SvtAv1 if scenario == Scenario::Collection => "4",
        Family::SvtAv1 => "6",
        Family::Qsv if slow => "slow",
        Family::Qsv => "medium",
        Family::Nvenc if slow => "p6",
        Family::Nvenc => "p5",
        Family::Amf if slow => "quality",
        Family::Amf => "balanced",
        Family::Vt => "default",
    }
}

pub fn supports_10bit(id: EncoderId, caps: &Capabilities) -> bool {
    !id.is_hardware() || caps.encoder(id).is_some_and(|e| e.ten_bit)
}

/// 会把 HDR10 静态元数据写进码流的编码器（技术事实文档 7.3 节）
pub fn writes_hdr10(id: EncoderId) -> bool {
    use EncoderId::*;
    matches!(id, Libx265 | Libx264 | Libsvtav1 | HevcQsv | HevcNvenc | Av1Nvenc | HevcAmf | Av1Amf)
}

/// 该格式的软件编码器在当前 ffmpeg 里可用
pub fn software_usable(codec: Codec, caps: &Capabilities) -> bool {
    caps.encoder_usable(EncoderId::software_for(codec))
}

/// 该格式有任何一个可用的编码器
pub fn codec_available(codec: Codec, caps: &Capabilities) -> bool {
    caps.encoders.iter().any(|e| e.codec == codec && e.usable)
}

/// 硬件厂商的优先级：Windows 上 NVENC 画质最好，其次 QSV、AMF
fn hw_order(platform: Platform) -> &'static [Vendor] {
    match platform {
        Platform::Windows => &[Vendor::Nvidia, Vendor::Intel, Vendor::Amd],
        Platform::Macos => &[Vendor::Apple],
        Platform::Linux => &[Vendor::Nvidia, Vendor::Intel],
    }
}

pub struct EncoderNeeds {
    pub prefer_hw: bool,
    pub need_10bit: bool,
    pub need_dv: bool,
    pub need_hdr10: bool,
}

pub struct EncoderPick {
    pub encoder: EncoderId,
    pub reason: String,
}

/// 按场景偏好与当前能力自动选编码器（设计文档 4.5）
pub fn pick_encoder(codec: Codec, needs: &EncoderNeeds, caps: &Capabilities) -> EncoderPick {
    let sw = EncoderId::software_for(codec);
    let pick = |encoder, reason: String| EncoderPick { encoder, reason };
    if needs.need_dv {
        return pick(sw, "杜比视界的动态元数据只能由软件编码器写入，硬件编码器无法输出杜比视界".into());
    }
    if !software_usable(codec, caps) {
        // 软件编码器没编译进当前 ffmpeg（例如 essentials 构建没有 libsvtav1）：只能用硬件编码器
        let hw: Vec<_> = caps.encoders.iter().filter(|e| e.codec == codec && e.usable && e.id.is_hardware()).collect();
        let best = hw.iter().find(|e| !needs.need_10bit || e.ten_bit).or(hw.first());
        return match best {
            Some(e) => pick(e.id, format!("当前 ffmpeg 没有 {}，改用 {} 硬件编码", sw.name(), e.vendor.label())),
            None => pick(sw, format!("当前 ffmpeg 没有任何可用的 {} 编码器", codec.label())),
        };
    }
    if !needs.prefer_hw {
        return pick(sw, "软件编码在同等体积下画质最好，适合长期保存".into());
    }
    let mut skipped_for_hdr10 = false;
    for &vendor in hw_order(caps.platform) {
        let Some(hit) = caps
            .encoders
            .iter()
            .find(|e| e.vendor == vendor && e.codec == codec && e.usable && (!needs.need_10bit || e.ten_bit))
        else {
            continue;
        };
        // 要保留 HDR10 时跳过不写 MDCV/CLL 的编码器（VideoToolbox），否则"修正"后仍然冲突
        if needs.need_hdr10 && !writes_hdr10(hit.id) {
            skipped_for_hdr10 = true;
            continue;
        }
        return pick(hit.id, format!("使用 {} 硬件编码，速度约为软编的 5–10 倍，适合非收藏用途", vendor.label()));
    }
    let reason = if skipped_for_hdr10 {
        "可用的硬件编码器不会写入 HDR10 元数据，为保留 HDR10 改用软件编码".to_string()
    } else {
        format!(
            "没有可用的 {}{} 硬件编码器，已改用软件编码",
            codec.label(),
            if needs.need_10bit { " 10bit" } else { "" }
        )
    };
    pick(sw, reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EnvStatus;

    #[test]
    fn quality_scales_differ_by_family() {
        assert_eq!(quality_value(EncoderId::Libx265, QualityTier::High), 20);
        assert_eq!(quality_value(EncoderId::HevcNvenc, QualityTier::High), 23);
        assert_eq!(quality_value(EncoderId::HevcQsv, QualityTier::High), 21);
        assert!(!quality_meta(EncoderId::HevcVideotoolbox).lower_is_better);
    }

    #[test]
    fn dv_always_goes_to_software() {
        let caps = Capabilities::placeholder(EnvStatus::Probing, "");
        let p = pick_encoder(
            Codec::Hevc,
            &EncoderNeeds { prefer_hw: true, need_10bit: true, need_dv: true, need_hdr10: true },
            &caps,
        );
        assert_eq!(p.encoder, EncoderId::Libx265);
    }
}
