import type { FpsInsight, VideoStream } from "@/lib/types";

/**
 * 标准帧率档。NTSC 帧率用精确分数，传给 ffmpeg 时也用分数形式，
 * 避免 29.97 这种近似值累积出时间轴误差。
 */
export const STANDARD_FPS = [
  { value: 24000 / 1001, arg: "24000/1001", label: "23.976" },
  { value: 24, arg: "24", label: "24" },
  { value: 25, arg: "25", label: "25" },
  { value: 30000 / 1001, arg: "30000/1001", label: "29.97" },
  { value: 30, arg: "30", label: "30" },
  { value: 50, arg: "50", label: "50" },
  { value: 60000 / 1001, arg: "60000/1001", label: "59.94" },
  { value: 60, arg: "60", label: "60" },
  { value: 120, arg: "120", label: "120" },
] as const;

const SNAP_TOLERANCE = 0.02;
/** 误差小于它才认为源本来就是这个档，典型是真正的 NTSC 源 */
const EXACT_TOLERANCE = 0.001;

/**
 * 吸附到标准帧率档（容差 2%），吸附不上时四舍五入到整数。
 *
 * 29.97 与 30、59.94 与 60 只差 0.1%，"取最近"会被噪声左右。可变帧率的实际平均值
 * 因掉帧总是偏低（如 29.41、29.8），按最近档会被误判成 NTSC。所以：只有误差小于
 * 0.1%（明确就是该档）才采纳 NTSC 档，其余模糊情况优先整数档。
 */
export function snapFps(fps: number): number {
  const within = STANDARD_FPS.map((s) => ({ v: s.value, err: Math.abs(fps - s.value) / s.value }))
    .filter((c) => c.err <= SNAP_TOLERANCE)
    .sort((a, b) => a.err - b.err);
  const nearest = within[0];
  if (!nearest) return Math.round(fps);
  if (nearest.err < EXACT_TOLERANCE) return nearest.v;
  return within.find((c) => Number.isInteger(c.v))?.v ?? nearest.v;
}

export function fpsArg(fps: number): string {
  const hit = STANDARD_FPS.find((s) => Math.abs(s.value - fps) < 1e-6);
  return hit ? hit.arg : String(Math.round(fps * 1000) / 1000);
}

/**
 * 推荐的 CFR 目标帧率：优先取名义帧率（只复制帧、不丢帧），
 * 名义帧率异常（录屏常见 1000/1）时回退到平均帧率。
 */
export function recommendCfrTarget(v: VideoStream): number {
  const nominalSane = v.fpsNominal >= 10 && v.fpsNominal <= 240;
  return snapFps(nominalSane ? v.fpsNominal : v.fpsAvg);
}

/** 帧率波动剧烈：平均帧率不到名义帧率的 60%，典型如录屏 */
export function isExtremeVfr(v: VideoStream): boolean {
  return v.isVfr && v.fpsAvg / v.fpsNominal < 0.6;
}

export function computeFpsInsight(v: VideoStream, durationSec: number, targetFps: number): FpsInsight {
  const sourceFrames = v.frameCount ?? Math.round(v.fpsAvg * durationSec);
  const targetFrames = Math.round(targetFps * durationSec);
  return {
    sourceFrames,
    targetFrames,
    duplicated: Math.max(0, targetFrames - sourceFrames),
    dropped: Math.max(0, sourceFrames - targetFrames),
    targetFps,
  };
}
