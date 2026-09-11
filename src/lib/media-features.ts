import { tr } from "@/i18n";
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
      label: `${tr("杜比视界", "Dolby Vision")} ${p}${dv.hasEnhancementLayer ? ` ${dv.elType ?? tr("双层", "dual layer")}` : ""}`,
      tone: "dv",
      title: dv.hasEnhancementLayer
        ? tr(
            `Profile ${dv.profile} 双层（${dv.elType ?? "EL"}），重编码无法保留增强层`,
            `Dual-layer Profile ${dv.profile} (${dv.elType ?? "EL"}); re-encoding cannot keep the enhancement layer`,
          )
        : tr(`Profile ${p} 单层，可在 CPU 编码时保留`, `Single-layer Profile ${p}; kept when encoding on the CPU`),
    });
  }
  if (v?.color.hdrKind === "hdr10") {
    const nits = v.hdr10?.maxLuminance;
    const title = nits ? tr(`母版峰值 ${nits} nits`, `Mastering peak ${nits} nits`) : "HDR10";
    out.push({ key: "hdr10", label: "HDR10", tone: "hdr", title });
  } else if (v?.color.hdrKind === "hlg") {
    out.push({ key: "hlg", label: "HLG", tone: "hdr", title: tr("混合对数伽马 HDR", "Hybrid Log-Gamma HDR") });
  } else if (v?.color.hdrKind === "pq_no_meta") {
    const title = tr("标记为 PQ 但缺少 HDR10 元数据", "Tagged as PQ but missing HDR10 metadata");
    out.push({ key: "pq", label: "PQ", tone: "hdr", title });
  }
  if (v?.hdr10plus) {
    out.push({ key: "hdr10plus", label: "HDR10+", tone: "hdr", title: tr("含 HDR10+ 动态元数据", "Has HDR10+ dynamic metadata") });
  }

  const atmos = m.audio.find((a) => a.atmos);
  if (atmos) out.push({ key: "atmos", label: tr("全景声", "Atmos"), tone: "atmos", title: atmos.title ?? "Dolby Atmos" });

  const lossless = m.audio.find((a) => a.lossless && !a.atmos);
  const losslessAny = m.audio.find((a) => a.lossless);
  if (lossless || (losslessAny && !atmos)) {
    const a = (lossless ?? losslessAny)!;
    const label = LOSSLESS_NAME[a.codec] ?? tr("无损", "Lossless");
    out.push({ key: "lossless", label, tone: "lossless", title: tr("无损音轨", "Lossless audio") });
  } else if (atmos?.lossless) {
    const label = LOSSLESS_NAME[atmos.codec] ?? tr("无损", "Lossless");
    out.push({ key: "lossless", label, tone: "lossless", title: tr("无损音轨", "Lossless audio") });
  }

  if (v?.isVfr) {
    out.push({
      key: "vfr",
      label: tr("可变帧率", "VFR"),
      tone: "vfr",
      title: tr(
        `平均 ${v.fpsAvg.toFixed(2)} fps，名义 ${v.fpsNominal} fps。导入剪辑软件前建议转为固定帧率`,
        `Average ${v.fpsAvg.toFixed(2)} fps, nominal ${v.fpsNominal} fps. Convert to constant frame rate before editing`,
      ),
    });
  }
  if (v && v.bitDepth >= 10 && v.color.hdrKind === "none") {
    out.push({ key: "10bit", label: `${v.bitDepth}bit`, tone: "neutral", title: tr(`${v.bitDepth}bit 色深`, `${v.bitDepth}-bit color`) });
  }
  const pgs = m.subtitle.filter((s) => s.imageBased).length;
  if (pgs) {
    const title = tr("蓝光图形字幕，MP4 无法容纳", "Blu-ray image subtitles, which MP4 cannot hold");
    out.push({ key: "pgs", label: `PGS ×${pgs}`, tone: "neutral", title });
  }
  if (m.audio.length > 1) {
    const label = tr(`${m.audio.length} 音轨`, `${m.audio.length} tracks`);
    out.push({ key: "tracks", label, tone: "neutral", title: tr("多条音轨", "Multiple audio tracks") });
  }
  return out;
}
