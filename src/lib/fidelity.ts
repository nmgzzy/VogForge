import type { FidelityKind } from "./types";

/** 保真度勾选项的悬停说明；判定与标题来自引擎 */
export const FIDELITY_HINT: Record<FidelityKind, string> = {
  dolbyVision: "逐帧动态元数据，支持的电视能呈现更准确的 HDR",
  hdr10: "高动态范围与广色域信息",
  hdr10plus: "另一种逐帧动态元数据",
  lossless: "TrueHD、Atmos、DTS-HD MA、PCM",
  allAudio: "多语言与评论音轨",
  allSubtitles: "包括蓝光图形字幕（PGS）",
  chapters: "片内章节跳转点",
  tenBit: "减少天空、渐变处的色带",
};
