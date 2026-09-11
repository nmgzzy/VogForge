/**
 * 浏览器预览的队列演示数据：预置任务、模拟速度与模拟的校验报告。模拟推进在 src/backend/mock-queue.ts；
 * 桌面应用的队列在 crates/vidforge-core/src/queue，进度来自 ffmpeg -progress 的块协议解析。
 */
import type { FidelityKind, Job, JobEvent, MediaInfo, ReportItem, Settings, TranscodePlan } from "@/lib/types";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { formatBytes, formatDuration } from "@/lib/format";
import { evaluate, recommendPlan, updatePlan } from "@/lib/engine";
import { MOCK_CAPABILITIES as caps } from "./capabilities";
import { MOCK_MEDIA } from "./media";

const T0 = Date.now();
const media = (id: string) => MOCK_MEDIA.find((m) => m.id === id)!;

function ev(offsetSec: number, level: JobEvent["level"], message: string): JobEvent {
  return { at: T0 + offsetSec * 1000, level, message };
}

/** 命令与输出路径按入队时的设置计算，与转码页预览的一致 */
export function makeJob(
  m: MediaInfo,
  plan: TranscodePlan,
  settings: Settings = DEFAULT_SETTINGS,
  id = `job-${Math.random().toString(36).slice(2, 9)}`,
): Job {
  const r = evaluate(m, plan, caps, settings);
  return {
    id,
    media: m,
    plan,
    args: r.args,
    firstPass: r.firstPass,
    outputPath: r.args[r.args.length - 1]!,
    status: "queued",
    progress: { percent: 0, outTimeSec: 0, speed: 0, fps: 0, sizeBytes: 0, dupFrames: 0, dropFrames: 0 },
    encoderUsed: plan.video.encoder,
    events: [{ at: Date.now(), level: "info", message: "已加入队列" }],
    log: [],
    attempts: 0,
  };
}

/** 模拟速度（实时倍速）：用预估耗时反推 */
export function simulatedSpeed(job: Job): number {
  const r = evaluate(job.media, job.plan, caps);
  const t = (r.estimate.timeMinSec + r.estimate.timeMaxSec) / 2;
  return Math.max(0.05, job.media.durationSec / Math.max(t, 1));
}

export function estimatedOutputSize(job: Job): number {
  const r = evaluate(job.media, job.plan, caps);
  return (r.estimate.sizeMin + r.estimate.sizeMax) / 2;
}

/** 取说明的第一句，报告表格里放不下整段解释 */
function firstClause(text: string): string {
  return text.split(/[。；]/)[0] ?? text;
}

/** 某一项确实保留下来时，报告里"实际"一栏写什么 */
function achievedText(kind: FidelityKind, m: MediaInfo, p: TranscodePlan): string {
  const v = m.video[0];
  switch (kind) {
    case "dolbyVision":
      return p.video.action === "copy" ? "原样复制，含增强层" : "配置记录与逐帧 RPU 均存在";
    case "hdr10":
      return v?.color.hdrKind === "hlg" ? "bt2020 / arib-std-b67" : "MDCV 与 MaxCLL 存在（按有理数求值比较）";
    case "hdr10plus":
      return "原样复制";
    case "lossless":
      return "复制轨编码与源一致";
    case "allAudio":
      return `${m.audio.length} 条均在`;
    case "allSubtitles":
      return `${m.subtitle.length} 条均在`;
    case "chapters":
      return `${m.chapters} 个`;
    case "tenBit":
      return p.video.encoder.endsWith("_qsv") || p.video.encoder.endsWith("_nvenc") ? "p010le" : "yuv420p10le";
  }
}

/**
 * 模拟转码后的 ffprobe 校验。分两部分：
 * 1. 基础完整性（时长、帧数或固定帧率、音轨数）。
 * 2. 用户勾选要保留的每一项，结果取决于计划实际会生成什么，与保真度求解器的判定一致。
 *    冲突未修正就提交的任务会在这里如实报失败，而不是一律"通过"。
 * 真实实现中第 2 部分来自对输出文件的 ffprobe，不依赖求解器（见 docs/design.md 4.8）。
 */
export function buildReport(job: Job): ReportItem[] {
  const m = job.media;
  const v = m.video[0];
  const p = job.plan;
  const items: ReportItem[] = [
    { label: "时长", expected: formatDuration(m.durationSec), actual: formatDuration(m.durationSec), ok: true },
  ];
  if (v && p.video.action === "encode" && p.video.fps.kind === "cfr") {
    const fps = p.video.fps.fps.toFixed(3).replace(/\.?0+$/, "");
    items.push({ label: "固定帧率", expected: "r_frame_rate = avg_frame_rate", actual: `${fps} / ${fps}`, ok: true });
    items.push({ label: "音画对齐", expected: "差值 < 1 帧", actual: "0.000 秒", ok: true });
  } else if (v) {
    const n = v.frameCount ?? Math.round(v.fpsAvg * m.durationSec);
    items.push({ label: "帧数", expected: n.toLocaleString(), actual: n.toLocaleString(), ok: true });
  }
  items.push({ label: "音轨数", expected: `${p.audio.length} 条`, actual: `${p.audio.length} 条`, ok: true });

  for (const f of evaluate(m, p, caps).fidelity) {
    if (!p.fidelity[f.kind] || f.state === "not_applicable") continue;
    const ok = f.state === "achievable";
    items.push({
      label: f.label,
      expected: "保留",
      actual: ok ? achievedText(f.kind, m, p) : `未保留：${firstClause(f.detail)}`,
      ok,
    });
  }
  return items;
}

const SAMPLE_LOG = [
  "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'D:\\素材\\2026-08 京都\\IMG_4521.MOV':",
  "  Duration: 00:02:14.60, start: 0.000000, bitrate: 78302 kb/s",
  "  Stream #0:0[0x1](und): Video: hevc (Main 10) (hvc1), yuv420p10le(tv, bt2020nc/bt2020/arib-std-b67), 3840x2160, 77904 kb/s, 29.41 fps, 30 tbr",
  "      Side data: DOVI configuration record: version: 1.0, profile: 8, level: 6, rpu flag: 1, el flag: 0, bl flag: 1, compatibility id: 4",
  "  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a), 48000 Hz, stereo, fltp, 256 kb/s",
  "x265 [info]: HEVC encoder version 4.3",
  "x265 [info]: build info [Windows][GCC 16.1.0][64 bit] 10bit",
  "x265 [info]: using cpu capabilities: MMX2 SSE2Fast LZCNT SSSE3 SSE4.2 AVX FMA3 BMI2 AVX2",
  "x265 [info]: Main 10 profile, Level-5.1 (Main tier)",
  "x265 [info]: Dolby Vision RPU enabled, profile 8.4",
  "x265 [info]: Thread pool created using 16 threads",
  "Output #0, matroska, to 'D:\\转码输出\\IMG_4521_2160p_hevc.mkv.vidforge-part':",
  "  Stream #0:0: Video: hevc (Main 10), yuv420p10le(tv, bt2020nc/bt2020/arib-std-b67), 3840x2160, q=2-31, 29.41 fps",
  "  Stream #0:1: Audio: aac (LC), 48000 Hz, stereo, fltp, 256 kb/s",
  "x265 [info]: frame I:     34, Avg QP:17.12  kb/s: 61244.18",
  "x265 [info]: frame P:    991, Avg QP:19.85  kb/s: 21830.44",
  "x265 [info]: frame B:   2934, Avg QP:23.41  kb/s: 9102.63",
  "encoded 3959 frames in 441.20s (8.97 fps), 14460.42 kb/s, Avg QP:22.36",
];

export function seedJobs(): Job[] {
  // 1. 已完成：iPhone 素材归档
  const iphone = media("m-iphone");
  const done = makeJob(iphone, recommendPlan(iphone, "archive", caps), DEFAULT_SETTINGS, "job-done");
  const doneSize = 243_600_000;
  Object.assign(done, {
    status: "done",
    attempts: 1,
    progress: { ...done.progress, percent: 100, outTimeSec: iphone.durationSec, speed: 0.31, fps: 8.97, sizeBytes: doneSize },
    outputSize: doneSize,
    startedAt: T0 - 460_000,
    finishedAt: T0 - 18_000,
    log: SAMPLE_LOG,
    events: [
      ev(-470, "info", "已加入队列"),
      ev(-462, "info", "预检通过：libx265 以当前参数试编码 3 帧成功"),
      ev(-460, "info", "开始转码"),
      ev(-18, "info", `完成，校验通过：${formatBytes(iphone.sizeBytes)} → ${formatBytes(doneSize)}`),
    ],
  } satisfies Partial<Job>);
  done.report = buildReport(done);

  // 2. 运行中（CPU 票）：无人机素材归档
  const drone = media("m-drone");
  const running = makeJob(drone, recommendPlan(drone, "archive", caps), DEFAULT_SETTINGS, "job-run-cpu");
  Object.assign(running, {
    status: "running",
    attempts: 1,
    startedAt: T0 - 95_000,
    progress: { percent: 37, outTimeSec: drone.durationSec * 0.37, speed: 0.34, fps: 20.4, sizeBytes: 402_000_000, dupFrames: 0, dropFrames: 0 },
    events: [
      ev(-100, "info", "已加入队列"),
      ev(-96, "info", "预检通过：libx265 以当前参数试编码 3 帧成功"),
      ev(-95, "info", "开始转码"),
    ],
  } satisfies Partial<Job>);

  // 3. 运行中（GPU 票）：用户手选了 NVENC，预检失败后回退到 QSV
  const camera = media("m-camera");
  const camPlan = recommendPlan(camera, "streaming", caps);
  camPlan.video.encoder = "hevc_nvenc";
  camPlan.video.encoderAuto = false;
  const gpu = makeJob(camera, camPlan, DEFAULT_SETTINGS, "job-run-gpu");
  const fallbackPlan = updatePlan({ ...camPlan, video: { ...camPlan.video, encoder: "hevc_qsv", preset: "medium", qualityValue: 24 } }, camera, caps);
  Object.assign(gpu, {
    status: "running",
    attempts: 2,
    plan: fallbackPlan,
    encoderUsed: "hevc_qsv",
    args: evaluate(camera, fallbackPlan, caps).args,
    startedAt: T0 - 41_000,
    progress: { percent: 58, outTimeSec: camera.durationSec * 0.58, speed: 2.9, fps: 72.5, sizeBytes: 139_000_000, dupFrames: 0, dropFrames: 0 },
    events: [
      ev(-44, "info", "已加入队列"),
      ev(-43, "warn", "hevc_nvenc 的设备不可用（[hevc_nvenc @ 000001f2e4327240] Cannot load nvcuda.dll），本次运行不再使用 NVIDIA NVENC，回退到 hevc_qsv"),
      ev(-42, "info", "预检通过：hevc_qsv 以当前参数试编码 3 帧成功"),
      ev(-41, "info", "开始转码"),
    ],
  } satisfies Partial<Job>);

  // 4、5. 排队中
  const bluray = media("m-bluray");
  const q1 = makeJob(bluray, recommendPlan(bluray, "collection", caps), DEFAULT_SETTINGS, "job-q-bluray");
  const screen = media("m-screen");
  const q2 = makeJob(screen, recommendPlan(screen, "editing", caps), DEFAULT_SETTINGS, "job-q-screen");

  // 6. 失败：文件损坏，错误信息翻译成可行动的中文
  const broken: MediaInfo = {
    ...iphone,
    id: "m-broken",
    name: "IMG_3310.MOV",
    path: "D:\\素材\\2026-08 京都\\IMG_3310.MOV",
    sizeBytes: 402_000_000,
  };
  const failed = makeJob(broken, recommendPlan(broken, "archive", caps), DEFAULT_SETTINGS, "job-failed");
  Object.assign(failed, {
    status: "failed",
    attempts: 1,
    startedAt: T0 - 300_000,
    finishedAt: T0 - 299_000,
    log: [
      "[mov,mp4,m4a,3gp,3g2,mj2 @ 000001f2a8c4e0c0] moov atom not found",
      "[in#0 @ 000001f2a8c3d940] Error opening input: Invalid data found when processing input",
      "Error opening input file D:\\素材\\2026-08 京都\\IMG_3310.MOV.",
      "Error opening input files: Invalid data found when processing input",
    ],
    events: [
      ev(-301, "info", "已加入队列"),
      ev(-300, "error", "文件不完整或已损坏：缺少 moov 索引。常见于录制中途断电、手机存储已满或拷贝未完成。可尝试用原设备重新导出，或使用 untrunc 等工具修复"),
    ],
  } satisfies Partial<Job>);

  return [running, gpu, q1, q2, done, failed];
}
