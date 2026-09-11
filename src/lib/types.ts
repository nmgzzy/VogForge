/**
 * 前后端共享的核心类型。
 *
 * 结构与 docs/design.md 第 3 章对齐。全部由 ts-rs 从 Rust 模型生成到 src/bindings/
 * （改 Rust 模型后运行 `pnpm bindings` 重新生成），这里只做 re-export。
 */

// ───────────────────────── 环境与设置 ─────────────────────────
export type { AfterAction } from "@/bindings/AfterAction";
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

// ───────────────────────── 媒体分析与导入 ─────────────────────────
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

// ───────────────────────── 转码计划与引擎产出 ─────────────────────────
export type { ArgSegment } from "@/bindings/ArgSegment";
export type { AudioCodec } from "@/bindings/AudioCodec";
export type { AudioMode } from "@/bindings/AudioMode";
export type { AudioTrackPlan } from "@/bindings/AudioTrackPlan";
export type { Container } from "@/bindings/Container";
export type { Decision } from "@/bindings/Decision";
export type { DoviAction } from "@/bindings/DoviAction";
export type { Estimate } from "@/bindings/Estimate";
export type { FidelityItem } from "@/bindings/FidelityItem";
export type { FidelityKind } from "@/bindings/FidelityKind";
export type { FidelityRequest } from "@/bindings/FidelityRequest";
export type { FidelityState } from "@/bindings/FidelityState";
export type { Fix } from "@/bindings/Fix";
export type { FpsInsight } from "@/bindings/FpsInsight";
export type { FpsPolicy } from "@/bindings/FpsPolicy";
export type { HdrAction } from "@/bindings/HdrAction";
export type { PlanResult } from "@/bindings/PlanResult";
export type { QualityTier } from "@/bindings/QualityTier";
export type { RateControl } from "@/bindings/RateControl";
export type { RateControlKind } from "@/bindings/RateControlKind";
export type { ResolutionPreset } from "@/bindings/ResolutionPreset";
export type { Scenario } from "@/bindings/Scenario";
export type { Severity } from "@/bindings/Severity";
export type { StreamAction } from "@/bindings/StreamAction";
export type { SubtitleMode } from "@/bindings/SubtitleMode";
export type { TrackRole } from "@/bindings/TrackRole";
export type { TranscodePlan } from "@/bindings/TranscodePlan";
export type { VideoPlan } from "@/bindings/VideoPlan";

// 引擎的静态规则表
export type { EncoderMeta } from "@/bindings/EncoderMeta";
export type { EngineMeta } from "@/bindings/EngineMeta";
export type { QualityValues } from "@/bindings/QualityValues";
export type { StandardFpsMeta } from "@/bindings/StandardFpsMeta";
export type { VideoHints } from "@/bindings/VideoHints";

// ───────────────────────── 队列 ─────────────────────────
export type { EventLevel } from "@/bindings/EventLevel";
export type { Job } from "@/bindings/Job";
export type { JobEvent } from "@/bindings/JobEvent";
export type { JobProgress } from "@/bindings/JobProgress";
export type { JobProgressEvent } from "@/bindings/JobProgressEvent";
export type { JobStatus } from "@/bindings/JobStatus";
export type { QueueItem } from "@/bindings/QueueItem";
export type { QueueOp } from "@/bindings/QueueOp";
export type { QueueSnapshot } from "@/bindings/QueueSnapshot";
export type { ReportItem } from "@/bindings/ReportItem";
