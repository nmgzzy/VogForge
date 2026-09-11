/**
 * 编码器相关的界面文案与查询。数值规则（质量刻度、preset、码率控制支持）来自引擎的规则表，
 * 这里只放显示用的名字和对环境能力的简单查询。
 */
import { tr } from "@/i18n";
import type { Capabilities, Codec, EncoderId, QualityTier, Vendor } from "./types";
import { encoderMeta } from "./engine";

export const CODEC_LABEL: Record<Codec, string> = { h264: "H.264", hevc: "HEVC", av1: "AV1" };

const VENDOR_NAMES: Record<Exclude<Vendor, "software">, string> = {
  intel: "Intel QSV",
  nvidia: "NVIDIA NVENC",
  amd: "AMD AMF",
  apple: "Apple VideoToolbox",
};

export const vendorLabel = (v: Vendor): string => (v === "software" ? tr("CPU 软编", "CPU (software)") : VENDOR_NAMES[v]);

export const encoderVendor = (id: EncoderId): Vendor => encoderMeta(id).vendor;

export const isHardware = (id: EncoderId): boolean => encoderMeta(id).hardware;

/** 质量档位在该编码器上的原生数值（跨编码器不等价） */
export const qualityValue = (id: EncoderId, tier: QualityTier): number => encoderMeta(id).quality[tier];

/** 该编码格式有任何一个可用的编码器（软编或硬编） */
export const codecAvailable = (codec: Codec, caps: Capabilities): boolean =>
  caps.encoders.some((e) => e.codec === codec && e.usable);

export function encoderSupports10bit(id: EncoderId, caps: Capabilities): boolean {
  if (!isHardware(id)) return true;
  return caps.encoders.find((e) => e.id === id)?.tenBit ?? false;
}

/** 当前 ffmpeg 至少有一条可用的色调映射管线 */
export const canTonemap = (caps: Capabilities): boolean => caps.tonemap.some((t) => t.available);
