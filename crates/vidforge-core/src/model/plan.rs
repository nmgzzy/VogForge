//! 转码计划与引擎产出（设计文档 3.2）。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::caps::ToneMapPipeline;
use super::encoder::{Codec, EncoderId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Scenario {
    /// 相机素材归档
    Archive,
    /// NAS 高画质收藏
    Collection,
    /// 流媒体直出（Plex / Jellyfin）
    Streaming,
    /// 手机平板观看
    Mobile,
    /// 社交分享
    Social,
    /// 最小体积
    Smallest,
    /// 剪辑预处理
    Editing,
    /// 原样封装
    Remux,
}

impl Scenario {
    pub const ALL: [Scenario; 8] = [
        Scenario::Archive,
        Scenario::Collection,
        Scenario::Streaming,
        Scenario::Mobile,
        Scenario::Social,
        Scenario::Smallest,
        Scenario::Editing,
        Scenario::Remux,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum QualityTier {
    /// 视觉无损
    Lossless,
    High,
    Standard,
    /// 小体积
    Small,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Container {
    Mkv,
    Mp4,
    Mov,
}

impl Container {
    pub fn ext(self) -> &'static str {
        match self {
            Container::Mkv => "mkv",
            Container::Mp4 => "mp4",
            Container::Mov => "mov",
        }
    }

    /// `-f` 用的 muxer 名
    pub fn muxer(self) -> &'static str {
        match self {
            Container::Mkv => "matroska",
            Container::Mp4 => "mp4",
            Container::Mov => "mov",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum ResolutionPreset {
    #[serde(rename = "source")]
    Source,
    #[serde(rename = "2160")]
    P2160,
    #[serde(rename = "1440")]
    P1440,
    #[serde(rename = "1080")]
    P1080,
    #[serde(rename = "720")]
    P720,
    #[serde(rename = "480")]
    P480,
}

impl ResolutionPreset {
    /// 目标短边像素数；原始分辨率为 None
    pub fn short_side(self) -> Option<u32> {
        match self {
            ResolutionPreset::Source => None,
            ResolutionPreset::P2160 => Some(2160),
            ResolutionPreset::P1440 => Some(1440),
            ResolutionPreset::P1080 => Some(1080),
            ResolutionPreset::P720 => Some(720),
            ResolutionPreset::P480 => Some(480),
        }
    }
}

/// 帧率策略（设计文档 4.9）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum FpsPolicy {
    /// 不传帧率参数，保持原时间戳
    Keep,
    /// 转固定帧率
    Cfr { fps: f64 },
    /// 仅当源超过上限时下调
    Cap { max: f64 },
}

/// 码率控制（需求 F-3.3）。码率单位都是 kbps
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum RateControl {
    /// 恒定质量：CRF / CQ / ICQ，数值取 `quality_value`
    #[default]
    Quality,
    /// 目标平均码率，峰值不超过 1.5 倍
    Bitrate { kbps: u32 },
    /// 恒定质量，但峰值码率不超过 `kbps`（给网络串流留余量）
    Capped { kbps: u32 },
    /// 两遍编码：第一遍分析画面复杂度，第二遍按目标平均码率分配。只有软件编码器支持
    TwoPass { kbps: u32 },
}

/// 码率控制方式（不含数值），界面据此列出编码器支持的选项
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RateControlKind {
    Quality,
    Bitrate,
    Capped,
    TwoPass,
}

impl RateControlKind {
    pub const ALL: [RateControlKind; 4] =
        [RateControlKind::Quality, RateControlKind::Bitrate, RateControlKind::Capped, RateControlKind::TwoPass];
}

impl RateControl {
    /// 按码率计的模式（不看质量数值）
    pub fn target_kbps(self) -> Option<u32> {
        match self {
            RateControl::Bitrate { kbps } | RateControl::TwoPass { kbps } => Some(kbps),
            _ => None,
        }
    }

    pub fn kind(self) -> RateControlKind {
        match self {
            RateControl::Quality => RateControlKind::Quality,
            RateControl::Bitrate { .. } => RateControlKind::Bitrate,
            RateControl::Capped { .. } => RateControlKind::Capped,
            RateControl::TwoPass { .. } => RateControlKind::TwoPass,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HdrAction {
    Keep,
    Tonemap,
    Strip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DoviAction {
    Preserve,
    Disable,
    Remux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StreamAction {
    Copy,
    Encode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct VideoPlan {
    pub action: StreamAction,
    pub codec: Codec,
    pub encoder: EncoderId,
    /// 为 true 时由引擎按场景与能力选择编码器
    pub encoder_auto: bool,
    pub quality: QualityTier,
    /// 当前编码器下的原生质量数值（CRF / CQ / global_quality …）
    pub quality_value: i32,
    #[serde(default)]
    pub rate_control: RateControl,
    pub preset: String,
    #[ts(type = "8 | 10")]
    pub bit_depth: u8,
    pub resolution: ResolutionPreset,
    pub fps: FpsPolicy,
    pub hdr_action: HdrAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tonemap: Option<ToneMapPipeline>,
    pub dovi: DoviAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gop: Option<u32>,
    /// 追加到 `-x265-params` 的原始串
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_params: Option<String>,
    /// 附加 ffmpeg 参数（支持引号）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_args: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AudioCodec {
    Aac,
    Eac3,
    Ac3,
    Opus,
    Flac,
    /// 24bit PCM：MOV（剪辑用途）装不下 TrueHD / DTS 时的替代
    #[serde(rename = "pcm_s24le")]
    PcmS24le,
}

impl AudioCodec {
    pub fn name(self) -> &'static str {
        match self {
            AudioCodec::Aac => "aac",
            AudioCodec::Eac3 => "eac3",
            AudioCodec::Ac3 => "ac3",
            AudioCodec::Opus => "opus",
            AudioCodec::Flac => "flac",
            AudioCodec::PcmS24le => "pcm_s24le",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TrackRole {
    /// 源轨（原样或重编码）
    Original,
    /// 追加的兼容轨
    Compat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct AudioTrackPlan {
    /// 源音轨的流序号
    pub source_index: u32,
    pub action: StreamAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec: Option<AudioCodec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate_kbps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub role: TrackRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AudioMode {
    CopyAll,
    CompatOnly,
    OriginalPlusCompat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SubtitleMode {
    All,
    TextOnly,
    None,
}

/// 用户勾选的"尽量保留"项
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FidelityRequest {
    pub dolby_vision: bool,
    pub hdr10: bool,
    pub hdr10plus: bool,
    pub lossless: bool,
    pub all_audio: bool,
    pub all_subtitles: bool,
    pub chapters: bool,
    pub ten_bit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FidelityKind {
    DolbyVision,
    Hdr10,
    Hdr10plus,
    Lossless,
    AllAudio,
    AllSubtitles,
    Chapters,
    TenBit,
}

impl FidelityRequest {
    pub fn get(&self, kind: FidelityKind) -> bool {
        match kind {
            FidelityKind::DolbyVision => self.dolby_vision,
            FidelityKind::Hdr10 => self.hdr10,
            FidelityKind::Hdr10plus => self.hdr10plus,
            FidelityKind::Lossless => self.lossless,
            FidelityKind::AllAudio => self.all_audio,
            FidelityKind::AllSubtitles => self.all_subtitles,
            FidelityKind::Chapters => self.chapters,
            FidelityKind::TenBit => self.ten_bit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranscodePlan {
    pub scenario: Scenario,
    pub video: VideoPlan,
    pub audio: Vec<AudioTrackPlan>,
    pub audio_mode: AudioMode,
    pub subtitles: SubtitleMode,
    pub container: Container,
    pub fidelity: FidelityRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Severity {
    Info,
    Tip,
    Warn,
}

/// "为什么这么选"的一条理由
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Decision {
    pub field: String,
    pub value: String,
    pub reason: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FidelityState {
    Achievable,
    NeedsChange,
    Impossible,
    NotApplicable,
}

/// 一键修正：对 TranscodePlan 的一个补丁
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Fix {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FidelityItem {
    pub kind: FidelityKind,
    pub label: String,
    pub state: FidelityState,
    pub detail: String,
    pub fixes: Vec<Fix>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Estimate {
    pub size_min: f64,
    pub size_max: f64,
    pub time_min_sec: f64,
    pub time_max_sec: f64,
    /// 输出 / 源 的体积比，取区间中值
    pub ratio: f64,
    /// 预计的平均视频码率（bps）；原样封装时是源的视频码率。界面切到按码率编码时以它为起始值
    pub video_bps: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FpsInsight {
    pub source_frames: u64,
    pub target_frames: u64,
    pub duplicated: u64,
    pub dropped: u64,
    pub target_fps: f64,
}

/// 命令的一段，界面按段换行展示
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ArgSegment {
    pub label: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct PlanResult {
    pub plan: TranscodePlan,
    pub decisions: Vec<Decision>,
    pub fidelity: Vec<FidelityItem>,
    pub args: Vec<String>,
    /// 与 args 相同的命令，按段分组，界面按段换行展示
    pub segments: Vec<ArgSegment>,
    /// 两遍编码的第一遍命令（只分析、不输出文件）；其余模式为空
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_pass: Option<Vec<String>>,
    pub estimate: Estimate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps_insight: Option<FpsInsight>,
    /// 源码率已低于目标时的"不建议转码"提示
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_worth_it: Option<String>,
}
