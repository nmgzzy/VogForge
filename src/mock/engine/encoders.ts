import type { Capabilities, Codec, EncoderId, QualityTier, Scenario, Vendor } from "@/lib/types";

export const SOFTWARE_ENCODER: Record<Codec, EncoderId> = {
  h264: "libx264",
  hevc: "libx265",
  av1: "libsvtav1",
};

export const CODEC_LABEL: Record<Codec, string> = { h264: "H.264", hevc: "HEVC", av1: "AV1" };

export const VENDOR_LABEL: Record<Vendor, string> = {
  software: "CPU 软编",
  intel: "Intel QSV",
  nvidia: "NVIDIA NVENC",
  amd: "AMD AMF",
  apple: "Apple VideoToolbox",
};

export function encoderVendor(id: EncoderId): Vendor {
  if (id.startsWith("lib")) return "software";
  if (id.endsWith("_qsv")) return "intel";
  if (id.endsWith("_nvenc")) return "nvidia";
  if (id.endsWith("_amf")) return "amd";
  return "apple";
}

export function encoderCodec(id: EncoderId): Codec {
  if (id === "libx264" || id.startsWith("h264")) return "h264";
  if (id === "libsvtav1" || id.startsWith("av1")) return "av1";
  return "hevc";
}

export const isHardware = (id: EncoderId) => encoderVendor(id) !== "software";

/** 硬件优先级：Windows 上 NVENC 画质最好，其次 QSV、AMF */
const HW_ORDER: Record<Capabilities["platform"], Vendor[]> = {
  windows: ["nvidia", "intel", "amd"],
  macos: ["apple"],
  linux: ["nvidia", "intel"],
};

export interface EncoderPick {
  encoder: EncoderId;
  reason: string;
}

/** 该编码格式的软件编码器在当前 ffmpeg 里可用 */
export function softwareUsable(codec: Codec, caps: Capabilities): boolean {
  return caps.encoders.some((e) => e.id === SOFTWARE_ENCODER[codec] && e.usable);
}

/** 该编码格式有任何一个可用的编码器（软编或硬编） */
export function codecAvailable(codec: Codec, caps: Capabilities): boolean {
  return caps.encoders.some((e) => e.codec === codec && e.usable);
}

export function pickEncoder(
  codec: Codec,
  opts: { preferHw: boolean; need10bit: boolean; needDv: boolean; needHdr10?: boolean },
  caps: Capabilities,
): EncoderPick {
  const sw = SOFTWARE_ENCODER[codec];
  if (opts.needDv) {
    return { encoder: sw, reason: "杜比视界的动态元数据只能由软件编码器写入，硬件编码器无法输出杜比视界" };
  }
  if (!softwareUsable(codec, caps)) {
    // 软件编码器没有编译进当前 ffmpeg（例如 essentials 构建没有 libsvtav1）：只能用硬件编码器
    const hw = caps.encoders.filter((e) => e.codec === codec && e.usable && e.vendor !== "software");
    const best = hw.find((e) => !opts.need10bit || e.tenBit) ?? hw[0];
    return best
      ? { encoder: best.id, reason: `当前 ffmpeg 没有 ${sw}，改用 ${VENDOR_LABEL[best.vendor]} 硬件编码` }
      : { encoder: sw, reason: `当前 ffmpeg 没有任何可用的 ${CODEC_LABEL[codec]} 编码器` };
  }
  if (!opts.preferHw) {
    return { encoder: sw, reason: "软件编码在同等体积下画质最好，适合长期保存" };
  }
  let skippedForHdr10 = false;
  for (const vendor of HW_ORDER[caps.platform]) {
    const hit = caps.encoders.find(
      (e) => e.vendor === vendor && e.codec === codec && e.usable && (!opts.need10bit || e.tenBit),
    );
    if (!hit) continue;
    // 要保留 HDR10 时，跳过不写 MDCV/CLL 的编码器（VideoToolbox），否则"修正"后仍然冲突
    if (opts.needHdr10 && !writesHdr10(hit.id)) {
      skippedForHdr10 = true;
      continue;
    }
    return {
      encoder: hit.id,
      reason: `使用 ${VENDOR_LABEL[vendor]} 硬件编码，速度约为软编的 5–10 倍，适合非收藏用途`,
    };
  }
  return {
    encoder: sw,
    reason: skippedForHdr10
      ? "可用的硬件编码器不会写入 HDR10 元数据，为保留 HDR10 改用软件编码"
      : `没有可用的 ${CODEC_LABEL[codec]}${opts.need10bit ? " 10bit" : ""} 硬件编码器，已改用软件编码`,
  };
}

// ───────────── 质量档位到原生数值的映射（跨编码器不等价） ─────────────

type Family = "x265" | "x264" | "svtav1" | "qsv" | "nvenc" | "amf" | "vt";

function family(id: EncoderId): Family {
  if (id === "libx265") return "x265";
  if (id === "libx264") return "x264";
  if (id === "libsvtav1") return "svtav1";
  const v = encoderVendor(id);
  return v === "intel" ? "qsv" : v === "nvidia" ? "nvenc" : v === "amd" ? "amf" : "vt";
}

const QUALITY: Record<Family, Record<QualityTier, number>> = {
  x265: { lossless: 16, high: 20, standard: 23, small: 27 },
  x264: { lossless: 15, high: 18, standard: 21, small: 25 },
  svtav1: { lossless: 20, high: 26, standard: 32, small: 38 },
  qsv: { lossless: 18, high: 21, standard: 24, small: 28 },
  nvenc: { lossless: 19, high: 23, standard: 26, small: 30 },
  amf: { lossless: 18, high: 21, standard: 24, small: 28 },
  // VideoToolbox 的 -q:v 是数值越大越好
  vt: { lossless: 80, high: 68, standard: 58, small: 48 },
};

export function qualityValue(id: EncoderId, tier: QualityTier): number {
  return QUALITY[family(id)][tier];
}

export interface QualityMeta {
  param: string;
  min: number;
  max: number;
  lowerIsBetter: boolean;
}

export function qualityMeta(id: EncoderId): QualityMeta {
  switch (family(id)) {
    case "x265":
    case "x264":
      return { param: "CRF", min: 0, max: 51, lowerIsBetter: true };
    case "svtav1":
      return { param: "CRF", min: 0, max: 63, lowerIsBetter: true };
    case "qsv":
      return { param: "global_quality", min: 1, max: 51, lowerIsBetter: true };
    case "nvenc":
      return { param: "CQ", min: 0, max: 51, lowerIsBetter: true };
    case "amf":
      return { param: "QP", min: 0, max: 51, lowerIsBetter: true };
    case "vt":
      return { param: "q:v", min: 1, max: 100, lowerIsBetter: false };
  }
}

export function presetOptions(id: EncoderId): string[] {
  switch (family(id)) {
    case "x265":
    case "x264":
      return ["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"];
    case "svtav1":
      return ["2", "3", "4", "5", "6", "7", "8", "10", "12"];
    case "qsv":
      return ["veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"];
    case "nvenc":
      return ["p1", "p2", "p3", "p4", "p5", "p6", "p7"];
    case "amf":
      return ["speed", "balanced", "quality"];
    case "vt":
      return ["default"];
  }
}

export function defaultPreset(id: EncoderId, scenario: Scenario): string {
  const slow = scenario === "collection" || scenario === "archive";
  switch (family(id)) {
    case "x265":
      return scenario === "collection" ? "slow" : slow ? "medium" : "fast";
    case "x264":
      return slow ? "slow" : "medium";
    case "svtav1":
      return scenario === "collection" ? "4" : "6";
    case "qsv":
      return slow ? "slow" : "medium";
    case "nvenc":
      return slow ? "p6" : "p5";
    case "amf":
      return slow ? "quality" : "balanced";
    case "vt":
      return "default";
  }
}

export function encoderSupports10bit(id: EncoderId, caps: Capabilities): boolean {
  if (!isHardware(id)) return true;
  return caps.encoders.find((e) => e.id === id)?.tenBit ?? false;
}

/** HDR10 静态元数据会被写入码流的编码器（见 ffmpeg-facts 第 7.3 节） */
export function writesHdr10(id: EncoderId): boolean {
  return (
    id === "libx265" ||
    id === "libx264" ||
    id === "libsvtav1" ||
    id === "hevc_qsv" ||
    id === "hevc_nvenc" ||
    id === "av1_nvenc" ||
    id === "hevc_amf" ||
    id === "av1_amf"
  );
}
