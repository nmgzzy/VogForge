/**
 * 前后端共享的核心类型。
 *
 * 结构与 docs/design.md 第 3 章对齐。阶段 4 起由 Rust 端通过 ts-rs 生成到
 * src/bindings/，届时本文件改为从 bindings re-export，不再手写。
 */

// ───────────────────────── 媒体分析 ─────────────────────────

export type HdrKind = "none" | "hdr10" | "hlg" | "pq_no_meta";

/** HDR10 静态元数据。数值一律为已求值的浮点（nits），不保留有理数字符串。 */
export interface Hdr10Metadata {
  maxLuminance: number;
  minLuminance: number;
  maxCll?: number;
  maxFall?: number;
  masteringPrimaries: "p3" | "bt2020" | "unknown";
}

export interface DoviInfo {
  /** 5 / 7 / 8 / 10 */
  profile: number;
  /** 8.1 → 1，8.4 → 4，P5 → 0 */
  blCompatId: number;
  hasEnhancementLayer: boolean;
  elType?: "MEL" | "FEL";
}

export interface ColorInfo {
  primaries: string;
  transfer: string;
  space: string;
  range: "tv" | "pc";
  hdrKind: HdrKind;
}

export interface VideoStream {
  index: number;
  codec: string;
  profile?: string;
  width: number;
  height: number;
  /** 实际平均帧率 */
  fpsAvg: number;
  /** 名义帧率（r_frame_rate） */
  fpsNominal: number;
  isVfr: boolean;
  bitDepth: 8 | 10 | 12;
  pixFmt: string;
  bitrate?: number;
  color: ColorInfo;
  hdr10?: Hdr10Metadata;
  dolbyVision?: DoviInfo;
  hdr10plus: boolean;
  rotation: number;
  frameCount?: number;
}

export interface AudioStream {
  index: number;
  codec: string;
  profile?: string;
  channels: number;
  channelLayout: string;
  sampleRate: number;
  bitrate?: number;
  language?: string;
  title?: string;
  isDefault: boolean;
  lossless: boolean;
  atmos: boolean;
  dtsX: boolean;
}

export interface SubtitleStream {
  index: number;
  codec: string;
  language?: string;
  title?: string;
  imageBased: boolean;
}

export type SourceHint =
  | "iphone"
  | "android"
  | "gopro"
  | "dji"
  | "camera"
  | "screen"
  | "bluray"
  | "streaming"
  | "unknown";

export interface MediaInfo {
  id: string;
  path: string;
  name: string;
  container: string;
  durationSec: number;
  sizeBytes: number;
  /** 总码率 bps */
  bitrate: number;
  video: VideoStream[];
  audio: AudioStream[];
  subtitle: SubtitleStream[];
  chapters: number;
  attachments: number;
  sourceHint: SourceHint;
  device?: string;
}

// ───────────────────────── 转码计划 ─────────────────────────

export type Scenario =
  | "archive"
  | "collection"
  | "streaming"
  | "mobile"
  | "social"
  | "smallest"
  | "editing"
  | "remux";

export type QualityTier = "lossless" | "high" | "standard" | "small";
export type Codec = "h264" | "hevc" | "av1";
export type Container = "mkv" | "mp4" | "mov";
export type Vendor = "software" | "intel" | "nvidia" | "amd" | "apple";

export type EncoderId =
  | "libx264"
  | "libx265"
  | "libsvtav1"
  | "h264_qsv"
  | "hevc_qsv"
  | "av1_qsv"
  | "h264_nvenc"
  | "hevc_nvenc"
  | "av1_nvenc"
  | "h264_amf"
  | "hevc_amf"
  | "av1_amf"
  | "h264_videotoolbox"
  | "hevc_videotoolbox";

export type ResolutionPreset = "source" | "2160" | "1440" | "1080" | "720" | "480";

export type FpsPolicy =
  | { kind: "keep" }
  | { kind: "cfr"; fps: number }
  | { kind: "cap"; max: number };

export type HdrAction = "keep" | "tonemap" | "strip";
export type DoviAction = "preserve" | "disable" | "remux";
export type ToneMapPipeline = "libplacebo" | "tonemap_opencl" | "zscale" | "scale_vt";

export interface VideoPlan {
  action: "copy" | "encode";
  codec: Codec;
  /** null 表示"自动选择"，由引擎决定 */
  encoder: EncoderId;
  encoderAuto: boolean;
  quality: QualityTier;
  /** 当前编码器下的原生质量数值（CRF / CQ / global_quality …） */
  qualityValue: number;
  preset: string;
  bitDepth: 8 | 10;
  resolution: ResolutionPreset;
  fps: FpsPolicy;
  hdrAction: HdrAction;
  tonemap?: ToneMapPipeline;
  dovi: DoviAction;
  gop?: number;
  extraParams?: string;
  extraArgs?: string;
}

export type AudioCodec = "aac" | "eac3" | "ac3" | "opus" | "flac";

export interface AudioTrackPlan {
  sourceIndex: number;
  action: "copy" | "encode";
  codec?: AudioCodec;
  bitrateKbps?: number;
  channels?: number;
  title?: string;
  role: "original" | "compat";
}

export interface FidelityRequest {
  dolbyVision: boolean;
  hdr10: boolean;
  hdr10plus: boolean;
  lossless: boolean;
  allAudio: boolean;
  allSubtitles: boolean;
  chapters: boolean;
  tenBit: boolean;
}

export type FidelityKind = keyof FidelityRequest;

export interface TranscodePlan {
  scenario: Scenario;
  video: VideoPlan;
  audio: AudioTrackPlan[];
  audioMode: "copy_all" | "compat_only" | "original_plus_compat";
  subtitles: "all" | "text_only" | "none";
  container: Container;
  fidelity: FidelityRequest;
}

// ───────────────────────── 引擎产出 ─────────────────────────

export interface Decision {
  field: string;
  value: string;
  reason: string;
  severity: "info" | "tip" | "warn";
}

export type FidelityState = "achievable" | "needs_change" | "impossible" | "not_applicable";

/** 一键修正：对 TranscodePlan 的一个补丁 */
export interface Fix {
  id: string;
  label: string;
}

export interface FidelityItem {
  kind: FidelityKind;
  label: string;
  state: FidelityState;
  detail: string;
  fixes: Fix[];
}

export interface Estimate {
  sizeMin: number;
  sizeMax: number;
  timeMinSec: number;
  timeMaxSec: number;
  /** 输出 / 源 的体积比，取区间中值 */
  ratio: number;
}

export interface FpsInsight {
  sourceFrames: number;
  targetFrames: number;
  duplicated: number;
  dropped: number;
  targetFps: number;
}

export interface PlanResult {
  plan: TranscodePlan;
  decisions: Decision[];
  fidelity: FidelityItem[];
  args: string[];
  estimate: Estimate;
  fpsInsight?: FpsInsight;
  /** 源码率已低于目标时的"不建议转码"提示 */
  notWorthIt?: string;
}

// ───────────────────────── 环境能力 ─────────────────────────

export type FailureKind = "device_missing" | "capability" | "param" | "resource" | "unknown";

export interface EncoderProbe {
  id: EncoderId;
  vendor: Vendor;
  codec: Codec;
  usable: boolean;
  tenBit: boolean;
  error?: string;
  failure?: FailureKind;
}

export interface BuildFlag {
  name: string;
  present: boolean;
  affects: string;
}

export interface Capabilities {
  ffmpegPath: string;
  ffprobePath: string;
  version: string;
  buildSource: string;
  buildFlags: BuildFlag[];
  encoders: EncoderProbe[];
  hwaccels: string[];
  tonemap: { id: ToneMapPipeline; available: boolean; note: string }[];
  dolbyVisionEncode: boolean;
  doviSplit: boolean;
  external: { name: string; found: boolean; purpose: string }[];
  gpus: { name: string; driver: string }[];
  platform: "windows" | "macos";
  probedAt: string;
}

// ───────────────────────── 队列 ─────────────────────────

export type JobStatus = "queued" | "running" | "paused" | "done" | "failed" | "cancelled";

export interface JobProgress {
  percent: number;
  outTimeSec: number;
  speed: number;
  fps: number;
  sizeBytes: number;
  etaSec?: number;
  dupFrames: number;
  dropFrames: number;
}

export interface JobEvent {
  at: number;
  level: "info" | "warn" | "error";
  message: string;
}

export interface ReportItem {
  label: string;
  expected: string;
  actual: string;
  ok: boolean;
}

export interface Job {
  id: string;
  media: MediaInfo;
  plan: TranscodePlan;
  args: string[];
  outputPath: string;
  status: JobStatus;
  progress: JobProgress;
  encoderUsed: EncoderId;
  events: JobEvent[];
  log: string[];
  report?: ReportItem[];
  startedAt?: number;
  finishedAt?: number;
  outputSize?: number;
}
