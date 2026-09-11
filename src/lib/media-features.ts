import type { MediaInfo } from "./types";

export type FeatureTone = "hdr" | "dv" | "atmos" | "lossless" | "vfr" | "neutral";

export interface Feature {
  key: string;
  label: string;
  tone: FeatureTone;
  title: string;
}

const LOSSLESS_NAME: Record<string, string> = {
  truehd: "TrueHD",
  dts: "DTS-HD MA",
  flac: "FLAC",
  pcm_s16le: "PCM",
  pcm_s24le: "PCM",
  alac: "ALAC",
};

/** 从媒体信息中提取需要在列表与详情里突出显示的高价值特征 */
export function mediaFeatures(m: MediaInfo): Feature[] {
  const out: Feature[] = [];
  const v = m.video[0];

  if (v?.dolbyVision) {
    const dv = v.dolbyVision;
    const p = dv.profile === 8 ? `8.${dv.blCompatId}` : String(dv.profile);
    out.push({
      key: "dv",
      label: `杜比视界 ${p}${dv.hasEnhancementLayer ? ` ${dv.elType ?? "双层"}` : ""}`,
      tone: "dv",
      title: dv.hasEnhancementLayer
        ? `Profile ${dv.profile} 双层（${dv.elType ?? "EL"}），重编码无法保留增强层`
        : `Profile ${p} 单层，可在 CPU 编码时保留`,
    });
  }
  if (v?.color.hdrKind === "hdr10") {
    const nits = v.hdr10?.maxLuminance;
    out.push({ key: "hdr10", label: "HDR10", tone: "hdr", title: nits ? `母版峰值 ${nits} nits` : "HDR10" });
  } else if (v?.color.hdrKind === "hlg") {
    out.push({ key: "hlg", label: "HLG", tone: "hdr", title: "混合对数伽马 HDR" });
  } else if (v?.color.hdrKind === "pq_no_meta") {
    out.push({ key: "pq", label: "PQ", tone: "hdr", title: "标记为 PQ 但缺少 HDR10 元数据" });
  }
  if (v?.hdr10plus) out.push({ key: "hdr10plus", label: "HDR10+", tone: "hdr", title: "含 HDR10+ 动态元数据" });

  const atmos = m.audio.find((a) => a.atmos);
  if (atmos) out.push({ key: "atmos", label: "全景声", tone: "atmos", title: atmos.title ?? "Dolby Atmos" });

  const lossless = m.audio.find((a) => a.lossless && !a.atmos);
  const losslessAny = m.audio.find((a) => a.lossless);
  if (lossless || (losslessAny && !atmos)) {
    const a = (lossless ?? losslessAny)!;
    out.push({ key: "lossless", label: LOSSLESS_NAME[a.codec] ?? "无损", tone: "lossless", title: "无损音轨" });
  } else if (atmos?.lossless) {
    out.push({ key: "lossless", label: LOSSLESS_NAME[atmos.codec] ?? "无损", tone: "lossless", title: "无损音轨" });
  }

  if (v?.isVfr) {
    out.push({
      key: "vfr",
      label: "可变帧率",
      tone: "vfr",
      title: `平均 ${v.fpsAvg.toFixed(2)} fps，名义 ${v.fpsNominal} fps。导入剪辑软件前建议转为固定帧率`,
    });
  }
  if (v && v.bitDepth >= 10 && v.color.hdrKind === "none") {
    out.push({ key: "10bit", label: `${v.bitDepth}bit`, tone: "neutral", title: `${v.bitDepth}bit 色深` });
  }
  const pgs = m.subtitle.filter((s) => s.imageBased).length;
  if (pgs) out.push({ key: "pgs", label: `PGS ×${pgs}`, tone: "neutral", title: "蓝光图形字幕，MP4 无法容纳" });
  if (m.audio.length > 1) {
    out.push({ key: "tracks", label: `${m.audio.length} 音轨`, tone: "neutral", title: "多条音轨" });
  }
  return out;
}
