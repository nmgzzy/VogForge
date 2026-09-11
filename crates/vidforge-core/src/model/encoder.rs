//! 编码格式、厂商与编码器标识。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Codec {
    H264,
    Hevc,
    Av1,
}

impl Codec {
    pub fn label(self) -> &'static str {
        match self {
            Codec::H264 => "H.264",
            Codec::Hevc => "HEVC",
            Codec::Av1 => "AV1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Vendor {
    Software,
    Intel,
    Nvidia,
    Amd,
    Apple,
}

impl Vendor {
    pub fn label(self) -> &'static str {
        match self {
            Vendor::Software => "CPU 软件编码",
            Vendor::Intel => "Intel QSV",
            Vendor::Nvidia => "NVIDIA NVENC",
            Vendor::Amd => "AMD AMF",
            Vendor::Apple => "Apple VideoToolbox",
        }
    }
}

/// VidForge 认识的全部视频编码器。序列化值就是 ffmpeg 的编码器名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum EncoderId {
    #[serde(rename = "libx264")]
    Libx264,
    #[serde(rename = "libx265")]
    Libx265,
    #[serde(rename = "libsvtav1")]
    Libsvtav1,
    #[serde(rename = "h264_qsv")]
    H264Qsv,
    #[serde(rename = "hevc_qsv")]
    HevcQsv,
    #[serde(rename = "av1_qsv")]
    Av1Qsv,
    #[serde(rename = "h264_nvenc")]
    H264Nvenc,
    #[serde(rename = "hevc_nvenc")]
    HevcNvenc,
    #[serde(rename = "av1_nvenc")]
    Av1Nvenc,
    #[serde(rename = "h264_amf")]
    H264Amf,
    #[serde(rename = "hevc_amf")]
    HevcAmf,
    #[serde(rename = "av1_amf")]
    Av1Amf,
    #[serde(rename = "h264_videotoolbox")]
    H264Videotoolbox,
    #[serde(rename = "hevc_videotoolbox")]
    HevcVideotoolbox,
}

impl EncoderId {
    pub const ALL: [EncoderId; 14] = [
        EncoderId::Libx264,
        EncoderId::Libx265,
        EncoderId::Libsvtav1,
        EncoderId::H264Qsv,
        EncoderId::HevcQsv,
        EncoderId::Av1Qsv,
        EncoderId::H264Nvenc,
        EncoderId::HevcNvenc,
        EncoderId::Av1Nvenc,
        EncoderId::H264Amf,
        EncoderId::HevcAmf,
        EncoderId::Av1Amf,
        EncoderId::H264Videotoolbox,
        EncoderId::HevcVideotoolbox,
    ];

    /// ffmpeg 里的编码器名
    pub fn name(self) -> &'static str {
        match self {
            EncoderId::Libx264 => "libx264",
            EncoderId::Libx265 => "libx265",
            EncoderId::Libsvtav1 => "libsvtav1",
            EncoderId::H264Qsv => "h264_qsv",
            EncoderId::HevcQsv => "hevc_qsv",
            EncoderId::Av1Qsv => "av1_qsv",
            EncoderId::H264Nvenc => "h264_nvenc",
            EncoderId::HevcNvenc => "hevc_nvenc",
            EncoderId::Av1Nvenc => "av1_nvenc",
            EncoderId::H264Amf => "h264_amf",
            EncoderId::HevcAmf => "hevc_amf",
            EncoderId::Av1Amf => "av1_amf",
            EncoderId::H264Videotoolbox => "h264_videotoolbox",
            EncoderId::HevcVideotoolbox => "hevc_videotoolbox",
        }
    }

    pub fn from_name(name: &str) -> Option<EncoderId> {
        EncoderId::ALL.into_iter().find(|e| e.name() == name)
    }

    pub fn codec(self) -> Codec {
        use EncoderId::*;
        match self {
            Libx264 | H264Qsv | H264Nvenc | H264Amf | H264Videotoolbox => Codec::H264,
            Libx265 | HevcQsv | HevcNvenc | HevcAmf | HevcVideotoolbox => Codec::Hevc,
            Libsvtav1 | Av1Qsv | Av1Nvenc | Av1Amf => Codec::Av1,
        }
    }

    pub fn vendor(self) -> Vendor {
        use EncoderId::*;
        match self {
            Libx264 | Libx265 | Libsvtav1 => Vendor::Software,
            H264Qsv | HevcQsv | Av1Qsv => Vendor::Intel,
            H264Nvenc | HevcNvenc | Av1Nvenc => Vendor::Nvidia,
            H264Amf | HevcAmf | Av1Amf => Vendor::Amd,
            H264Videotoolbox | HevcVideotoolbox => Vendor::Apple,
        }
    }

    pub fn is_hardware(self) -> bool {
        self.vendor() != Vendor::Software
    }

    /// 某编码格式的软件编码器
    pub fn software_for(codec: Codec) -> EncoderId {
        match codec {
            Codec::H264 => EncoderId::Libx264,
            Codec::Hevc => EncoderId::Libx265,
            Codec::Av1 => EncoderId::Libsvtav1,
        }
    }

    /// 某厂商、某编码格式的编码器（不存在时返回 None，例如 VideoToolbox 没有 AV1）
    pub fn for_vendor(vendor: Vendor, codec: Codec) -> Option<EncoderId> {
        EncoderId::ALL.into_iter().find(|e| e.vendor() == vendor && e.codec() == codec)
    }
}

/// 硬件编码失败的分类，决定回退动作（设计文档 4.2）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FailureKind {
    /// 设备或驱动缺失：本次会话禁用该厂商
    DeviceMissing,
    /// 硬件不支持该格式：降级参数重试
    Capability,
    /// 参数不被接受：去掉可选参数重试
    Param,
    /// 资源不足：降并发、退避重试
    Resource,
    /// 当前 ffmpeg 没有编译这个编码器
    NotBuilt,
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_names_match_ffmpeg_names() {
        for e in EncoderId::ALL {
            let json = serde_json::to_string(&e).unwrap();
            assert_eq!(json, format!("\"{}\"", e.name()));
            assert_eq!(EncoderId::from_name(e.name()), Some(e));
        }
    }

    #[test]
    fn every_codec_has_a_software_encoder() {
        for c in [Codec::H264, Codec::Hevc, Codec::Av1] {
            let e = EncoderId::software_for(c);
            assert_eq!(e.codec(), c);
            assert!(!e.is_hardware());
        }
    }

    #[test]
    fn videotoolbox_has_no_av1() {
        assert_eq!(EncoderId::for_vendor(Vendor::Apple, Codec::Av1), None);
        assert_eq!(EncoderId::for_vendor(Vendor::Intel, Codec::Av1), Some(EncoderId::Av1Qsv));
    }
}
