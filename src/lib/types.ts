/**
 * 前后端共享的核心类型。
 *
 * 结构与 docs/design.md 第 3 章对齐。已迁到 Rust 的类型由 ts-rs 生成到 src/bindings/
 * （改 Rust 模型后运行 cargo test 重新生成），这里只做 re-export；其余类型随各阶段逐步迁移。
 */

import type { Codec } from "@/bindings/Codec";
import type { EncoderId } from "@/bindings/EncoderId";
import type { MediaInfo } from "@/bindings/MediaInfo";
import type { ToneMapPipeline } from "@/bindings/ToneMapPipeline";

export type { AfterAction } from "@/bindings/AfterAction";
export type { AudioStream } from "@/bindings/AudioStream";
export type { ColorInfo } from "@/bindings/ColorInfo";
export type { DoviInfo } from "@/bindings/DoviInfo";
export type { Hdr10Metadata } from "@/bindings/Hdr10Metadata";
export type { HdrKind } from "@/bindings/HdrKind";
export type { ImportFailure } from "@/bindings/ImportFailure";
export type { ImportProgress } from "@/bindings/ImportProgress";
export type { ImportResult } from "@/bindings/ImportResult";
export type { MediaInfo } from "@/bindings/MediaInfo";
export type { SourceHint } from "@/bindings/SourceHint";
export type { SubtitleStream } from "@/bindings/SubtitleStream";
export type { VideoStream } from "@/bindings/VideoStream";
export type { BuildFlag } from "@/bindings/BuildFlag";
export type { Capabilities } from "@/bindings/Capabilities";
export type { Codec } from "@/bindings/Codec";
export type { ConflictPolicy } from "@/bindings/ConflictPolicy";
export type { DeviceProbe } from "@/bindings/DeviceProbe";
export type { EncoderId } from "@/bindings/EncoderId";
export type { EncoderProbe } from "@/bindings/EncoderProbe";
export type { EnvStatus } from "@/bindings/EnvStatus";
export type { ExternalTool } from "@/bindings/ExternalTool";
export type { FailureKind } from "@/bindings/FailureKind";
export type { GpuInfo } from "@/bindings/GpuInfo";
export type { Lang } from "@/bindings/Lang";
export type { LocateSource } from "@/bindings/LocateSource";
export type { Platform } from "@/bindings/Platform";
export type { ProbeProgress } from "@/bindings/ProbeProgress";
export type { Settings } from "@/bindings/Settings";
export type { ToneMapPipeline } from "@/bindings/ToneMapPipeline";
export type { TonemapProbe } from "@/bindings/TonemapProbe";
export type { Vendor } from "@/bindings/Vendor";

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
export type Container = "mkv" | "mp4" | "mov";

export type ResolutionPreset = "source" | "2160" | "1440" | "1080" | "720" | "480";

export type FpsPolicy =
  | { kind: "keep" }
  | { kind: "cfr"; fps: number }
  | { kind: "cap"; max: number };

export type HdrAction = "keep" | "tonemap" | "strip";
export type DoviAction = "preserve" | "disable" | "remux";

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
