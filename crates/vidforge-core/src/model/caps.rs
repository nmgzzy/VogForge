//! 环境能力快照（设计文档 3.3）。所有决策模块只读它，不自行探测环境。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::encoder::{Codec, EncoderId, FailureKind, Vendor};
use crate::i18n::{Lang, pick};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Platform {
    Windows,
    Macos,
    Linux,
}

impl Platform {
    pub fn current() -> Platform {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::Macos
        } else {
            Platform::Linux
        }
    }
}

/// ffmpeg 的整体状态，决定界面是否能开始转码
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EnvStatus {
    /// 可用
    Ready,
    /// 尚未探测完成（界面先按软编提供选项）
    Probing,
    /// 没找到 ffmpeg / ffprobe
    Missing,
    /// 找到了但版本低于最低要求
    TooOld,
    /// 找到了但无法运行
    Broken,
}

/// 在哪一级找到的 ffmpeg（设计文档 5.1）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LocateSource {
    /// 用户在设置里指定
    User,
    /// 应用目录（引导下载后存放处）
    Bundled,
    /// 当前进程的 PATH
    Path,
    /// 注册表里的 PATH（进程环境尚未刷新时）
    Registry,
    /// 平台常见安装位置
    Common,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ToneMapPipeline {
    Libplacebo,
    TonemapOpencl,
    Zscale,
    ScaleVt,
}

impl ToneMapPipeline {
    /// 按质量排序（设计文档 5.3）
    pub const ORDER: [ToneMapPipeline; 4] = [
        ToneMapPipeline::Libplacebo,
        ToneMapPipeline::TonemapOpencl,
        ToneMapPipeline::Zscale,
        ToneMapPipeline::ScaleVt,
    ];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BuildFlag {
    pub name: String,
    pub present: bool,
    /// 缺失时影响的功能
    pub affects: String,
}

/// 第 3 层真实试编码的结果
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct EncoderProbe {
    pub id: EncoderId,
    pub vendor: Vendor,
    pub codec: Codec,
    pub usable: bool,
    pub ten_bit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<FailureKind>,
}

/// 第 2 层硬件设备初始化的结果
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct DeviceProbe {
    /// ffmpeg 的 hwdevice 类型名：qsv / cuda / d3d11va / vulkan / opencl / videotoolbox …
    pub id: String,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 色调映射管线不可用的原因（界面文字由它按语言生成）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum TonemapBlock {
    /// 构建里没有需要的库或滤镜（libplacebo / OpenCL / libzimg / scale_vt）
    NotBuilt { what: String },
    /// 依赖的硬件设备初始化失败
    Device { device: String, error: String },
    /// scale_vt 只在 macOS 上有
    MacOnly,
    /// 试运行失败，保存 ffmpeg 原文
    TrialFailed { error: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct TonemapProbe {
    pub id: ToneMapPipeline,
    pub available: bool,
    /// 可用时是这条管线的特点，不可用时是原因
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<TonemapBlock>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct ExternalTool {
    pub name: String,
    pub found: bool,
    pub purpose: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GpuInfo {
    pub name: String,
    pub driver: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct Capabilities {
    pub status: EnvStatus,
    /// 状态的中文说明；就绪时为空
    pub status_detail: String,
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locate_source: Option<LocateSource>,
    /// 没找到时列出查找过的目录，便于用户排查
    pub searched: Vec<String>,
    /// 不影响使用的提示，如 ffprobe 与 ffmpeg 版本不一致
    pub notes: Vec<String>,
    /// ffmpeg 自报的版本串，如 `9.0.1-full_build-www.gyan.dev`
    pub version: String,
    /// 推断出的发行版本号，如 `9.0.1`；git 构建按库版本推断
    pub version_number: String,
    pub build_source: String,
    pub build_flags: Vec<BuildFlag>,
    pub encoders: Vec<EncoderProbe>,
    pub hwaccels: Vec<String>,
    pub devices: Vec<DeviceProbe>,
    pub tonemap: Vec<TonemapProbe>,
    /// libx265 支持 `-dolbyvision` 且版本满足要求
    pub dolby_vision_encode: bool,
    pub dovi_split: bool,
    pub external: Vec<ExternalTool>,
    pub gpus: Vec<GpuInfo>,
    pub platform: Platform,
    pub probed_at: String,
    /// 缓存 key：路径 + 修改时间 + 版本 + GPU + 驱动
    pub fingerprint: String,
}

impl Capabilities {
    /// 空白快照：尚未探测或找不到 ffmpeg 时使用。软件编码器按"可能可用"处理，
    /// 让界面在探测完成前也能给出软编方案。
    pub fn placeholder(status: EnvStatus, detail: impl Into<String>) -> Capabilities {
        let assume = status == EnvStatus::Probing;
        Capabilities {
            status,
            status_detail: detail.into(),
            ffmpeg_path: String::new(),
            ffprobe_path: String::new(),
            locate_source: None,
            searched: Vec::new(),
            notes: Vec::new(),
            version: String::new(),
            version_number: String::new(),
            build_source: String::new(),
            build_flags: Vec::new(),
            encoders: [EncoderId::Libx264, EncoderId::Libx265, EncoderId::Libsvtav1]
                .into_iter()
                .map(|id| EncoderProbe {
                    id,
                    vendor: id.vendor(),
                    codec: id.codec(),
                    usable: assume,
                    ten_bit: assume,
                    error: None,
                    failure: None,
                })
                .collect(),
            hwaccels: Vec::new(),
            devices: Vec::new(),
            tonemap: ToneMapPipeline::ORDER
                .into_iter()
                .map(|id| TonemapProbe { id, available: false, note: String::new(), block: None })
                .collect(),
            dolby_vision_encode: false,
            dovi_split: false,
            external: Vec::new(),
            gpus: Vec::new(),
            platform: Platform::current(),
            probed_at: String::new(),
            fingerprint: String::new(),
        }
    }

    pub fn encoder(&self, id: EncoderId) -> Option<&EncoderProbe> {
        self.encoders.iter().find(|e| e.id == id)
    }

    pub fn encoder_usable(&self, id: EncoderId) -> bool {
        self.encoder(id).is_some_and(|e| e.usable)
    }

    pub fn tonemap_available(&self, id: ToneMapPipeline) -> bool {
        self.tonemap.iter().any(|t| t.id == id && t.available)
    }

    /// 按质量顺序取第一个可用的色调映射管线
    pub fn pick_tonemap(&self) -> Option<ToneMapPipeline> {
        ToneMapPipeline::ORDER.into_iter().find(|&id| self.tonemap_available(id))
    }

    pub fn device_available(&self, id: &str) -> bool {
        self.devices.iter().any(|d| d.id == id && d.available)
    }

    pub fn can_transcode(&self) -> bool {
        self.status == EnvStatus::Ready
    }

    /// 决策引擎实际使用的能力（需求 F-5.6、设计文档 4.2）：设置里关了硬件编码时硬件编码器一律不可用，
    /// 关了硬件解码时不列硬解方式；本次会话因设备缺失而禁用的厂商同样不可用。环境页展示的仍是原始探测结果
    pub fn restricted(&self, hw_encode: bool, hw_decode: bool, disabled: &[Vendor], lang: Lang) -> Capabilities {
        let mut c = self.clone();
        for e in c.encoders.iter_mut().filter(|e| e.id.is_hardware() && e.usable) {
            if !hw_encode {
                e.usable = false;
                e.error =
                    Some(pick(lang, "设置里关闭了硬件编码", "Hardware encoding is turned off in Settings").into());
            } else if disabled.contains(&e.vendor) {
                e.usable = false;
                let msg = pick(
                    lang,
                    "本次运行中该厂商的设备不可用，已停用",
                    "This vendor's device failed during this session and is disabled",
                );
                e.error = Some(msg.into());
            }
        }
        if !hw_decode {
            c.hwaccels.clear();
        }
        c
    }
}

/// 探测进度事件
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProbeProgress {
    /// 当前阶段的中文说明
    pub stage: String,
    pub done: u32,
    pub total: u32,
}
