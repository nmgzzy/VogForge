import type { Codec, Estimate, EncoderId, MediaInfo, QualityTier, SourceHint, TranscodePlan } from "@/lib/types";
import { encoderVendor, isHardware } from "./encoders";
import { computeFpsInsight } from "./fps";
import { targetDimensions } from "./args";

/** 每像素每帧的比特数（bpp），按编码格式与质量档位的经验值 */
const BPP: Record<Codec, Record<QualityTier, number>> = {
  hevc: { lossless: 0.11, high: 0.065, standard: 0.042, small: 0.026 },
  h264: { lossless: 0.18, high: 0.11, standard: 0.07, small: 0.045 },
  av1: { lossless: 0.085, high: 0.05, standard: 0.032, small: 0.019 },
};

/** 画面运动量对码率的影响 */
const MOTION: Record<SourceHint, number> = {
  iphone: 1, android: 1, gopro: 1.35, dji: 1.3, camera: 1.1, screen: 0.45, bluray: 1.1, streaming: 1, unknown: 1,
};

/** 1080p30 下的实时倍速，用于耗时估计 */
function baseSpeed(encoder: EncoderId, preset: string): number {
  if (encoder === "libx265") {
    const t: Record<string, number> = {
      ultrafast: 6, superfast: 5, veryfast: 4, faster: 3, fast: 2.4, medium: 1.5, slow: 0.65, slower: 0.28, veryslow: 0.14,
    };
    return t[preset] ?? 1.5;
  }
  if (encoder === "libx264") {
    const t: Record<string, number> = { ultrafast: 20, veryfast: 12, fast: 8, medium: 5.5, slow: 3, slower: 1.6, veryslow: 0.9 };
    return t[preset] ?? 5;
  }
  if (encoder === "libsvtav1") {
    const p = Number(preset);
    return p >= 12 ? 9 : p >= 10 ? 5.5 : p >= 8 ? 3.2 : p >= 6 ? 1.6 : p >= 4 ? 0.55 : 0.18;
  }
  switch (encoderVendor(encoder)) {
    case "intel":
      return 11;
    case "nvidia":
      return 16;
    case "amd":
      return 12;
    default:
      return 9;
  }
}

export interface EstimateResult {
  estimate: Estimate;
  videoBps: number;
  sourceVideoBps: number;
}

export function estimate(media: MediaInfo, plan: TranscodePlan): EstimateResult {
  const v = media.video[0];
  const dur = media.durationSec;
  const vp = plan.video;
  const audioBps = plan.audio.reduce((sum, t) => {
    if (t.action === "copy") return sum + (media.audio.find((a) => a.index === t.sourceIndex)?.bitrate ?? 192_000);
    return sum + (t.bitrateKbps ?? 192) * 1000;
  }, 0);
  const sourceVideoBps = v?.bitrate ?? media.bitrate;

  if (!v || vp.action === "copy") {
    const size = media.sizeBytes;
    return {
      estimate: { sizeMin: size * 0.98, sizeMax: size, timeMinSec: dur / 80, timeMaxSec: dur / 30, ratio: 1 },
      videoBps: sourceVideoBps,
      sourceVideoBps,
    };
  }

  const dims = targetDimensions(v, vp.resolution);
  const w = dims?.w ?? v.width;
  const h = dims?.h ?? v.height;

  // 转 CFR 复制出的帧几乎零残差，按 3% 计入码率，而不是按帧数线性增长
  let effFps = v.fpsAvg;
  if (vp.fps.kind === "cfr") {
    const ins = computeFpsInsight(v, dur, vp.fps.fps);
    effFps = (ins.sourceFrames - ins.dropped + ins.duplicated * 0.03) / dur;
  }

  let bpp = BPP[vp.codec][vp.quality] * MOTION[media.sourceHint];
  if (isHardware(vp.encoder)) bpp *= 1.3;
  if (vp.hdrAction === "keep" && v.color.hdrKind !== "none") bpp *= 1.1;
  if (vp.gop && vp.gop < 30) bpp *= 1.25;

  let videoBps = bpp * w * h * effFps;
  // 重编码不会比源更大（CRF 模式下编码器会自然收敛），剪辑预处理除外
  if (plan.scenario !== "editing") videoBps = Math.min(videoBps, sourceVideoBps * 1.02);

  const total = ((videoBps + audioBps) * dur) / 8 * 1.01;
  const pixelScale = (1920 * 1080 * 30) / (w * h * Math.max(effFps, 1));
  let speed = baseSpeed(vp.encoder, vp.preset) * pixelScale;
  if (vp.hdrAction === "tonemap") speed *= vp.tonemap === "zscale" ? 0.35 : 0.8;
  const t = dur / Math.max(speed, 0.01);

  return {
    estimate: {
      sizeMin: total * 0.72,
      sizeMax: total * 1.32,
      timeMinSec: t * 0.75,
      timeMaxSec: t * 1.4,
      ratio: total / media.sizeBytes,
    },
    videoBps,
    sourceVideoBps,
  };
}
