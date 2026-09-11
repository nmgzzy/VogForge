import { tr } from "@/i18n";
import type { FidelityKind } from "./types";

/** 保真度勾选项的悬停说明；判定与标题来自引擎 */
export function fidelityHint(kind: FidelityKind): string {
  switch (kind) {
    case "dolbyVision":
      return tr("逐帧动态元数据，支持的电视能呈现更准确的 HDR", "Per-frame dynamic metadata for more accurate HDR on supporting TVs");
    case "hdr10":
      return tr("高动态范围与广色域信息", "High dynamic range and wide color gamut information");
    case "hdr10plus":
      return tr("另一种逐帧动态元数据", "Another kind of per-frame dynamic metadata");
    case "lossless":
      return "TrueHD, Atmos, DTS-HD MA, PCM";
    case "allAudio":
      return tr("多语言与评论音轨", "Other languages and commentary tracks");
    case "allSubtitles":
      return tr("包括蓝光图形字幕（PGS）", "Including Blu-ray image subtitles (PGS)");
    case "chapters":
      return tr("片内章节跳转点", "Chapter markers inside the video");
    case "tenBit":
      return tr("减少天空、渐变处的色带", "Less banding in skies and gradients");
  }
}
