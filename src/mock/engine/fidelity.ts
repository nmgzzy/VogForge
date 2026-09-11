import type {
  Capabilities,
  FidelityItem,
  FidelityKind,
  Fix,
  MediaInfo,
  TranscodePlan,
} from "@/lib/types";
import { encoderVendor, isHardware, VENDOR_LABEL, writesHdr10, encoderSupports10bit } from "./encoders";
import { audioFitsContainer } from "./recommend";

export const FIDELITY_META: Record<FidelityKind, { label: string; hint: string }> = {
  dolbyVision: { label: "杜比视界", hint: "逐帧动态元数据，支持的电视能呈现更准确的 HDR" },
  hdr10: { label: "HDR（HDR10 / HLG）", hint: "高动态范围与广色域信息" },
  hdr10plus: { label: "HDR10+", hint: "另一种逐帧动态元数据" },
  lossless: { label: "全景声与无损音轨", hint: "TrueHD、Atmos、DTS-HD MA、PCM" },
  allAudio: { label: "全部音轨", hint: "多语言与评论音轨" },
  allSubtitles: { label: "全部字幕", hint: "包括蓝光图形字幕（PGS）" },
  chapters: { label: "章节", hint: "片内章节跳转点" },
  tenBit: { label: "10bit 色深", hint: "减少天空、渐变处的色带" },
};

export const FIDELITY_ORDER: FidelityKind[] = [
  "dolbyVision",
  "hdr10",
  "hdr10plus",
  "lossless",
  "allAudio",
  "allSubtitles",
  "chapters",
  "tenBit",
];

function item(
  kind: FidelityKind,
  state: FidelityItem["state"],
  detail: string,
  fixes: Fix[] = [],
): FidelityItem {
  return { kind, label: FIDELITY_META[kind].label, state, detail, fixes };
}

const REMUX_FIX: Fix = { id: "remux", label: "改为原样封装" };

function dvLabel(profile: number, blCompatId: number): string {
  return profile === 8 ? `8.${blCompatId}` : String(profile);
}

export function resolveFidelity(media: MediaInfo, plan: TranscodePlan, caps: Capabilities): FidelityItem[] {
  return FIDELITY_ORDER.map((k) => RESOLVERS[k](media, plan, caps));
}

type Resolver = (m: MediaInfo, p: TranscodePlan, c: Capabilities) => FidelityItem;

const RESOLVERS: Record<FidelityKind, Resolver> = {
  dolbyVision(m, p, caps) {
    const dv = m.video[0]?.dolbyVision;
    if (!dv) return item("dolbyVision", "not_applicable", "源文件不含杜比视界");
    const name = `Profile ${dvLabel(dv.profile, dv.blCompatId)}`;

    if (p.video.action === "copy") {
      return item("dolbyVision", "achievable", `原样封装完整保留 ${name} 的全部数据，包括增强层`);
    }
    if (dv.hasEnhancementLayer) {
      return item(
        "dolbyVision",
        "impossible",
        `源为 ${name} 双层（${dv.elType ?? "EL"}）。ffmpeg 无法编码增强层，重编码只能降级为 8.1 并丢失增强层的亮度与色度映射。要完整保留，请改为原样封装。`,
        [REMUX_FIX],
      );
    }
    if (!caps.dolbyVisionEncode) {
      return item("dolbyVision", "impossible", "当前 ffmpeg 低于 7.1，无法写入杜比视界", [REMUX_FIX]);
    }

    const blockers: string[] = [];
    const changes: string[] = [];
    if (p.video.dovi !== "preserve") blockers.push("当前设置为不保留");
    if (isHardware(p.video.encoder)) {
      blockers.push("硬件编码器无法输出杜比视界");
      changes.push("改用 CPU 编码");
    }
    if (p.video.codec === "h264") {
      blockers.push("H.264 的杜比视界几乎没有播放器支持");
      changes.push("改用 HEVC");
    }
    if (p.video.hdrAction === "tonemap") {
      blockers.push("当前会色调映射为 SDR");
      changes.push("保留 HDR");
    }
    if (p.video.bitDepth !== 10) {
      blockers.push("杜比视界要求 10bit");
      changes.push("10bit");
    }

    if (blockers.length > 0) {
      const label = changes.length ? `开启保留（${changes.join(" · ")}）` : "开启保留";
      return item("dolbyVision", "needs_change", blockers.join("；"), [{ id: "dovi_preserve", label }]);
    }

    let detail = `保留 ${name}。ffmpeg 解析源的 RPU 后重新生成，逐帧动态元数据完整保留。`;
    if (p.container !== "mkv") detail += " MP4/MOV 输出会自动添加 hvc1 标记与 -strict unofficial。";
    if (dv.profile === 5) detail += " 注意：Profile 5 没有 HDR10 回退层，不支持杜比视界的设备会显示偏色。";
    return item("dolbyVision", "achievable", detail);
  },

  hdr10(m, p) {
    const v = m.video[0];
    if (!v || v.color.hdrKind === "none") return item("hdr10", "not_applicable", "源为 SDR 视频");
    const isHlg = v.color.hdrKind === "hlg";
    if (p.video.action === "copy") return item("hdr10", "achievable", "原样封装完整保留 HDR 信息");

    const blockers: string[] = [];
    const changes: string[] = [];
    if (p.video.hdrAction === "tonemap") {
      blockers.push("当前会色调映射为 SDR");
      changes.push("保留 HDR");
    }
    if (p.video.bitDepth !== 10) {
      blockers.push("8bit 输出 HDR 会产生明显色带");
      changes.push("10bit");
    }
    if (!isHlg && !writesHdr10(p.video.encoder)) {
      blockers.push(`${p.video.encoder} 不会把 HDR10 元数据写入码流`);
      changes.push(p.video.codec === "h264" ? "改用 HEVC" : "改用 CPU 编码");
    }
    if (blockers.length > 0) {
      return item("hdr10", "needs_change", blockers.join("；"), [
        { id: "keep_hdr", label: `保留 HDR（${changes.join(" · ")}）` },
      ]);
    }
    if (isHlg) return item("hdr10", "achievable", "保留 HLG 色彩标记。HLG 不依赖额外元数据，电视与手机可直接识别。");
    const how = isHardware(p.video.encoder)
      ? `${VENDOR_LABEL[encoderVendor(p.video.encoder)]} 会把母版显示与 MaxCLL 写入码流（已在本机实测验证）`
      : "软件编码器会自动透传母版显示与 MaxCLL 元数据";
    return item("hdr10", "achievable", `保留 HDR10。${how}。`);
  },

  hdr10plus(m, p) {
    if (!m.video[0]?.hdr10plus) return item("hdr10plus", "not_applicable", "源文件不含 HDR10+");
    if (p.video.action === "copy") return item("hdr10plus", "achievable", "原样封装完整保留");
    return item(
      "hdr10plus",
      "impossible",
      "ffmpeg 无法把 HDR10+ 动态元数据透传给编码器，需借助 x265 命令行或事后注入（v2 支持）。重编码后仍会保留 HDR10 基础层。",
      [REMUX_FIX],
    );
  },

  lossless(m, p) {
    const hq = m.audio.filter((a) => a.lossless || a.atmos);
    if (hq.length === 0) return item("lossless", "not_applicable", "源文件没有无损或全景声音轨");

    const blockers: string[] = [];
    let needMkv = false;
    for (const a of hq) {
      const name = a.title ?? a.codec.toUpperCase();
      const copied = p.audio.some((t) => t.sourceIndex === a.index && t.action === "copy");
      if (!copied) {
        blockers.push(
          a.atmos ? `「${name}」会被重新编码，全景声元数据将丢失（Atmos 无法重新编码）` : `「${name}」会被有损压缩`,
        );
      } else if (!audioFitsContainer(a.codec, p.container)) {
        blockers.push(`${p.container.toUpperCase()} 不支持 ${a.codec.toUpperCase()}`);
        needMkv = true;
      }
    }
    if (blockers.length > 0) {
      const label = needMkv || p.audioMode === "compat_only" ? "原样保留（切换到 MKV · 追加兼容轨）" : "原样保留";
      return item("lossless", "needs_change", blockers.join("；"), [{ id: "keep_lossless", label }]);
    }
    const compat = p.audio.some((t) => t.role === "compat");
    const names = hq.map((a) => a.title ?? a.codec.toUpperCase()).join("、");
    return item(
      "lossless",
      "achievable",
      `原样复制 ${names}${compat ? "，并额外生成兼容轨，手机与耳机也能播放" : ""}`,
    );
  },

  allAudio(m, p) {
    if (m.audio.length <= 1) {
      return item("allAudio", "not_applicable", m.audio.length === 0 ? "源文件没有音轨" : "源文件只有一条音轨");
    }
    const kept = new Set(p.audio.map((t) => t.sourceIndex));
    const missing = m.audio.filter((a) => !kept.has(a.index));
    const unfit = p.audio.filter((t) => {
      const src = m.audio.find((a) => a.index === t.sourceIndex);
      return t.action === "copy" && src && !audioFitsContainer(src.codec, p.container);
    });
    if (missing.length === 0 && unfit.length === 0) {
      return item("allAudio", "achievable", `保留全部 ${m.audio.length} 条音轨`);
    }
    const parts: string[] = [];
    if (missing.length) parts.push(`将丢弃 ${missing.length} 条：${missing.map((a) => a.title ?? a.codec).join("、")}`);
    if (unfit.length) parts.push(`${unfit.length} 条音轨的编码不被 ${p.container.toUpperCase()} 支持`);
    return item("allAudio", "needs_change", parts.join("；"), [
      { id: "audio_copy_all", label: unfit.length || p.container === "mp4" ? "复制全部（切换到 MKV）" : "复制全部音轨" },
    ]);
  },

  allSubtitles(m, p) {
    if (m.subtitle.length === 0) return item("allSubtitles", "not_applicable", "源文件没有字幕");
    const image = m.subtitle.filter((s) => s.imageBased);
    const blockers: string[] = [];
    if (p.subtitles === "none") blockers.push("当前设置不保留字幕");
    else if (p.subtitles === "text_only" && image.length) blockers.push(`将丢弃 ${image.length} 条图形字幕（PGS）`);
    else if (p.container !== "mkv" && image.length) blockers.push(`${p.container.toUpperCase()} 不支持 PGS 图形字幕`);
    if (blockers.length) {
      return item("allSubtitles", "needs_change", blockers.join("；"), [
        { id: "subs_all", label: image.length ? "保留全部（切换到 MKV）" : "保留全部字幕" },
      ]);
    }
    return item("allSubtitles", "achievable", `保留全部 ${m.subtitle.length} 条字幕`);
  },

  chapters(m, p) {
    if (m.chapters === 0) return item("chapters", "not_applicable", "源文件没有章节");
    const note = p.container === "mkv" ? "" : "（MP4/MOV 的章节在部分播放器中不显示）";
    return item("chapters", "achievable", `保留 ${m.chapters} 个章节${note}`);
  },

  tenBit(m, p, caps) {
    const v = m.video[0];
    if (!v || v.bitDepth < 10) return item("tenBit", "not_applicable", "源为 8bit 视频");
    if (p.video.action === "copy") return item("tenBit", "achievable", "原样封装保持原始位深");
    if (p.video.bitDepth === 10 && encoderSupports10bit(p.video.encoder, caps)) {
      return item("tenBit", "achievable", "输出 10bit");
    }
    const why = encoderSupports10bit(p.video.encoder, caps) ? "当前输出 8bit" : `${p.video.encoder} 不支持 10bit`;
    return item("tenBit", "needs_change", why, [{ id: "ten_bit", label: "改为 10bit" }]);
  },
};

/** 把一键修正应用到计划上。调用方随后应再做一次 normalizePlan。 */
export function applyFix(plan: TranscodePlan, fixId: string, media: MediaInfo): TranscodePlan {
  const next: TranscodePlan = structuredClone(plan);
  const vp = next.video;
  const hasImageSubs = media.subtitle.some((s) => s.imageBased);

  switch (fixId) {
    case "remux":
      next.scenario = "remux";
      vp.action = "copy";
      vp.dovi = "remux";
      vp.hdrAction = "keep";
      next.audioMode = "copy_all";
      next.subtitles = "all";
      next.container = "mkv";
      break;
    case "dovi_preserve":
      vp.dovi = "preserve";
      vp.encoderAuto = true;
      vp.hdrAction = "keep";
      vp.bitDepth = 10;
      if (vp.codec === "h264") vp.codec = "hevc";
      next.fidelity.dolbyVision = true;
      break;
    case "keep_hdr":
      vp.hdrAction = "keep";
      vp.bitDepth = 10;
      if (vp.codec === "h264") vp.codec = "hevc";
      if (!writesHdr10(vp.encoder)) vp.encoderAuto = true;
      next.fidelity.hdr10 = true;
      break;
    case "keep_lossless":
      if (next.audioMode === "compat_only") next.audioMode = "original_plus_compat";
      next.container = "mkv";
      next.fidelity.lossless = true;
      break;
    case "audio_copy_all":
      next.audioMode = "copy_all";
      next.container = "mkv";
      next.fidelity.allAudio = true;
      break;
    case "subs_all":
      next.subtitles = "all";
      if (hasImageSubs) next.container = "mkv";
      next.fidelity.allSubtitles = true;
      break;
    case "ten_bit":
      vp.bitDepth = 10;
      vp.encoderAuto = true;
      next.fidelity.tenBit = true;
      break;
  }
  return next;
}
