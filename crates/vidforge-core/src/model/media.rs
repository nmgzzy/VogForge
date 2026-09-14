//! ffprobe 分析结果（设计文档 3.1）。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HdrKind {
    None,
    Hdr10,
    Hlg,
    /// 标了 PQ 但没有静态元数据
    PqNoMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MasteringPrimaries {
    P3,
    Bt2020,
    Unknown,
}

/// HDR10 静态元数据。数值一律是已求值的浮点（nits / CIE xy），不保留有理数字符串：
/// HEVC 与 AV1 的定点分母不同（技术事实文档 2.2 节），字符串比对必然误判。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct Hdr10Metadata {
    pub max_luminance: f64,
    pub min_luminance: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_cll: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_fall: Option<f64>,
    pub mastering_primaries: MasteringPrimaries,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum ElType {
    /// 最小增强层：丢掉只损失很少
    #[serde(rename = "MEL")]
    Mel,
    /// 完整增强层：带亮度/色度映射，丢掉会损失明显
    #[serde(rename = "FEL")]
    Fel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct DoviInfo {
    /// 5 / 7 / 8 / 10
    pub profile: u8,
    /// 8.1 → 1，8.4 → 4，P5 → 0
    pub bl_compat_id: u8,
    pub has_enhancement_layer: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub el_type: Option<ElType>,
    /// 首帧带逐帧 RPU（`Dolby Vision RPU Data` 或解析后的 `Dolby Vision Metadata`）。
    /// 只有配置记录、没有 RPU 的流在播放器上不会按杜比视界播放
    #[serde(default)]
    pub rpu: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ColorRange {
    Tv,
    Pc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ColorInfo {
    pub primaries: String,
    pub transfer: String,
    pub space: String,
    pub range: ColorRange,
    pub hdr_kind: HdrKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct VideoStream {
    pub index: u32,
    pub codec: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub width: u32,
    pub height: u32,
    /// 实际平均帧率
    pub fps_avg: f64,
    /// 名义帧率（r_frame_rate）
    pub fps_nominal: f64,
    pub is_vfr: bool,
    #[ts(type = "8 | 10 | 12")]
    pub bit_depth: u8,
    pub pix_fmt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u64>,
    pub color: ColorInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hdr10: Option<Hdr10Metadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dolby_vision: Option<DoviInfo>,
    pub hdr10plus: bool,
    /// 显示旋转角度（度），来自 Display Matrix；宽高是编码尺寸
    pub rotation: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_count: Option<u64>,
    /// 这条流自己的时长（秒）。与音轨时长对比，转固定帧率时补齐尾部
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct AudioStream {
    pub index: u32,
    pub codec: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub channels: u32,
    pub channel_layout: String,
    pub sample_rate: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub is_default: bool,
    pub lossless: bool,
    pub atmos: bool,
    pub dts_x: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct SubtitleStream {
    pub index: u32,
    pub codec: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// PGS / VobSub / DVB 等图形字幕：MP4 装不下
    pub image_based: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SourceHint {
    Iphone,
    Android,
    Gopro,
    Dji,
    Camera,
    Screen,
    Bluray,
    Streaming,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct MediaInfo {
    /// 由规范化路径派生，同一文件重复导入得到同一个 id
    pub id: String,
    pub path: String,
    pub name: String,
    /// 简短容器名：mp4 / mov / mkv / m2ts …
    pub container: String,
    pub duration_sec: f64,
    pub size_bytes: u64,
    /// 总码率 bps
    pub bitrate: u64,
    pub video: Vec<VideoStream>,
    pub audio: Vec<AudioStream>,
    pub subtitle: Vec<SubtitleStream>,
    pub chapters: u32,
    pub attachments: u32,
    /// MP4 / MOV 的封面图（attached_pic 视频流）张数，没有时为空。现在的命令不带封面，推荐理由与校验会标出
    #[serde(skip_serializing_if = "Option::is_none")]
    pub covers: Option<u32>,
    pub source_hint: SourceHint,
    /// 拍摄设备，如 "Apple iPhone 16 Pro"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// 通过文件夹导入时的根目录，"保留源目录结构"据此计算相对路径
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import_root: Option<String>,
}

/// 导入失败的文件
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct ImportFailure {
    pub path: String,
    /// 给用户看的原因（按界面语言）
    pub reason: String,
    /// ffprobe 原文，界面上可展开查看
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportResult {
    pub media: Vec<MediaInfo>,
    pub failures: Vec<ImportFailure>,
    /// 文件夹里扩展名不像视频、被跳过的文件数
    pub skipped: u32,
    /// 被跳过的文件的扩展名（小写、带点；没有扩展名时为空串），界面据此说明跳过了什么
    #[serde(default)]
    pub skipped_exts: Vec<String>,
}

/// 导入进度事件
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportProgress {
    pub done: u32,
    pub total: u32,
    /// 刚分析完的文件名
    pub current: String,
}
