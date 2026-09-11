import type {
  AudioCodec,
  AudioStream,
  AudioTrackPlan,
  Capabilities,
  Codec,
  Container,
  FidelityRequest,
  MediaInfo,
  QualityTier,
  ResolutionPreset,
  Scenario,
  TranscodePlan,
} from "@/lib/types";
import { defaultPreset, encoderCodec, encoderSupports10bit, pickEncoder, qualityValue } from "./encoders";
import { recommendCfrTarget } from "./fps";
import { pickTonemap, tonemapAvailable } from "./color";

export interface ScenarioMeta {
  id: Scenario;
  title: string;
  /** 卡片上的短说明，控制在 7 个字以内，避免被截断 */
  short: string;
  tagline: string;
  outcome: string;
}

export const SCENARIOS: ScenarioMeta[] = [
  { id: "archive", title: "素材归档", short: "手机相机拍摄", tagline: "手机、相机素材压缩后长期保存", outcome: "体积大幅下降，保留 HDR 与杜比视界" },
  { id: "collection", title: "高画质收藏", short: "收藏级保存", tagline: "影片按收藏级保存", outcome: "视觉无损，保留全部音轨、字幕与章节" },
  { id: "streaming", title: "流媒体直出", short: "Plex / Jellyfin", tagline: "Plex / Jellyfin 直接播放", outcome: "主流电视与盒子直出，GPU 加速" },
  { id: "mobile", title: "手机平板", short: "随时随地看", tagline: "随时随地观看", outcome: "1080p SDR，体积小，处处可播" },
  { id: "social", title: "社交分享", short: "微信 B站 抖音", tagline: "微信、B站、抖音等平台", outcome: "平台友好，减少二次压缩变糊" },
  { id: "editing", title: "剪辑预处理", short: "导入剪辑前", tagline: "导入 Premiere / 达芬奇 / FCP 前", outcome: "固定帧率，音画不再逐渐失步" },
  { id: "smallest", title: "最小体积", short: "尽量省空间", tagline: "尽量省空间，画质可接受", outcome: "AV1 编码，体积最小" },
  { id: "remux", title: "原样封装", short: "不重新编码", tagline: "不重编码，只换容器或整理音轨", outcome: "零画质损失，秒级完成" },
];

interface Profile {
  codec: Codec;
  preferHw: boolean;
  quality: QualityTier;
  resolution: ResolutionPreset;
  tonemap: boolean;
  container: Container;
  audioMode: TranscodePlan["audioMode"];
  subtitles: TranscodePlan["subtitles"];
  cfr: "never" | "if_vfr" | "always";
  shortGop: boolean;
  keepDv: boolean;
}

function profileFor(scenario: Scenario, media: MediaInfo): Profile {
  const v = media.video[0];
  const isHdr = !!v && v.color.hdrKind !== "none";
  const hasLossless = media.audio.some((a) => a.lossless || a.atmos);
  const base: Omit<Profile, "codec" | "preferHw" | "quality" | "container" | "audioMode"> = {
    resolution: "source",
    tonemap: false,
    subtitles: "all",
    cfr: "never",
    shortGop: false,
    keepDv: false,
  };

  switch (scenario) {
    case "archive":
      return {
        ...base,
        codec: "hevc",
        preferHw: false,
        quality: "high",
        container: "mkv",
        audioMode: hasLossless ? "original_plus_compat" : "copy_all",
        keepDv: true,
      };
    case "collection":
      return {
        ...base,
        codec: "hevc",
        preferHw: false,
        quality: "lossless",
        container: "mkv",
        audioMode: "copy_all",
        keepDv: true,
      };
    case "streaming":
      return {
        ...base,
        codec: "hevc",
        preferHw: true,
        quality: "standard",
        container: "mp4",
        audioMode: "compat_only",
        subtitles: "text_only",
        cfr: "if_vfr",
      };
    case "mobile":
      return {
        ...base,
        codec: "h264",
        preferHw: true,
        quality: "standard",
        resolution: "1080",
        tonemap: isHdr,
        container: "mp4",
        audioMode: "compat_only",
        subtitles: "text_only",
      };
    case "social":
      return {
        ...base,
        codec: "h264",
        preferHw: true,
        quality: "high",
        resolution: "1080",
        tonemap: isHdr,
        container: "mp4",
        audioMode: "compat_only",
        subtitles: "none",
        cfr: "always",
      };
    case "editing":
      return {
        ...base,
        codec: isHdr ? "hevc" : "h264",
        preferHw: false,
        quality: "lossless",
        container: "mov",
        audioMode: "copy_all",
        subtitles: "none",
        cfr: "always",
        shortGop: true,
      };
    case "smallest":
      return {
        ...base,
        codec: "av1",
        preferHw: false,
        quality: "small",
        resolution: "1080",
        container: "mkv",
        audioMode: "compat_only",
        subtitles: "text_only",
      };
    case "remux":
      return {
        ...base,
        codec: "hevc",
        preferHw: false,
        quality: "high",
        container: "mkv",
        audioMode: "copy_all",
        keepDv: true,
      };
  }
}

/** 按源文件特征推荐的起始场景。已高度压缩的片源推荐原样封装，避免二次有损 */
export function suggestScenario(m: MediaInfo): Scenario {
  switch (m.sourceHint) {
    case "bluray":
      return "collection";
    case "streaming":
      return "remux";
    case "screen":
      return "editing";
    default:
      return "archive";
  }
}

export function preferHwFor(s: Scenario): boolean {
  return s === "streaming" || s === "mobile" || s === "social";
}

export function recommend(media: MediaInfo, scenario: Scenario, caps: Capabilities): TranscodePlan {
  const v = media.video[0];
  const p = profileFor(scenario, media);
  const isHdr = !!v && v.color.hdrKind !== "none";
  const dv = v?.dolbyVision;
  // 单层杜比视界才能在重编码时保留；P7 双层只能原样封装
  const dvPreservable = !!dv && !dv.hasEnhancementLayer;
  const wantDv = p.keepDv && dvPreservable;
  // 想转 SDR 却没有任何可用的色调映射管线时，只能保留 HDR（explain 会给出警告）
  const tonemap = isHdr && p.tonemap ? pickTonemap(caps) : undefined;
  const keepHdr = isHdr && !tonemap;
  const bitDepth: 8 | 10 = keepHdr || scenario === "archive" || scenario === "collection" ? 10 : 8;

  const needHdr10 = keepHdr && v?.color.hdrKind === "hdr10";
  const pick = pickEncoder(p.codec, { preferHw: p.preferHw, need10bit: bitDepth === 10, needDv: wantDv, needHdr10 }, caps);
  const fpsTarget = v ? recommendCfrTarget(v) : 30;
  const useCfr = !!v && (p.cfr === "always" || (p.cfr === "if_vfr" && v.isVfr));

  const plan: TranscodePlan = {
    scenario,
    video: {
      action: scenario === "remux" ? "copy" : "encode",
      codec: p.codec,
      encoder: pick.encoder,
      encoderAuto: true,
      quality: p.quality,
      qualityValue: qualityValue(pick.encoder, p.quality),
      preset: defaultPreset(pick.encoder, scenario),
      bitDepth,
      resolution: p.resolution,
      fps: useCfr ? { kind: "cfr", fps: fpsTarget } : { kind: "keep" },
      hdrAction: tonemap ? "tonemap" : "keep",
      tonemap,
      dovi: scenario === "remux" ? "remux" : wantDv ? "preserve" : "disable",
      gop: p.shortGop && v ? Math.max(1, Math.round(fpsTarget / 2)) : undefined,
    },
    audio: [],
    audioMode: p.audioMode,
    subtitles: p.subtitles,
    container: p.container,
    fidelity: defaultFidelity(media, scenario),
  };
  return normalizePlan(plan, media, caps);
}

/** 按场景给出默认的保真度勾选：源里有什么、场景在意什么，就勾什么 */
export function defaultFidelity(media: MediaInfo, scenario: Scenario): FidelityRequest {
  const v = media.video[0];
  const keepy = scenario === "archive" || scenario === "collection" || scenario === "remux";
  const isHdr = !!v && v.color.hdrKind !== "none";
  return {
    dolbyVision: keepy && !!v?.dolbyVision,
    hdr10: (keepy || scenario === "streaming" || scenario === "editing") && isHdr,
    hdr10plus: keepy && !!v?.hdr10plus,
    lossless: keepy && media.audio.some((a) => a.lossless || a.atmos),
    allAudio: (scenario === "collection" || scenario === "remux") && media.audio.length > 1,
    allSubtitles: (scenario === "collection" || scenario === "remux") && media.subtitle.length > 0,
    chapters: keepy && media.chapters > 0,
    tenBit: keepy && (v?.bitDepth ?? 8) >= 10,
  };
}

// ───────────────────────── 音轨生成 ─────────────────────────

const CONTAINER_AUDIO: Record<Container, readonly string[]> = {
  mp4: ["aac", "ac3", "eac3", "opus", "mp3", "flac"],
  mov: ["aac", "ac3", "eac3", "alac", "pcm_s16le", "pcm_s24le"],
  mkv: ["aac", "ac3", "eac3", "opus", "mp3", "flac", "truehd", "dts", "pcm_s16le", "pcm_s24le"],
};

export function audioFitsContainer(codec: string, container: Container): boolean {
  return CONTAINER_AUDIO[container].includes(codec);
}

function primaryAudio(media: MediaInfo): AudioStream | undefined {
  return media.audio.find((a) => a.isDefault) ?? media.audio[0];
}

function stereoCompatCodec(plan: TranscodePlan): AudioCodec {
  return plan.scenario === "smallest" ? "opus" : "aac";
}

export function buildAudioTracks(media: MediaInfo, plan: TranscodePlan): AudioTrackPlan[] {
  const primary = primaryAudio(media);
  if (!primary) return [];
  const tracks: AudioTrackPlan[] = [];
  const stereoCodec = stereoCompatCodec(plan);
  const stereoRate = plan.scenario === "smallest" ? 96 : plan.scenario === "mobile" ? 160 : 256;

  const pushStereoCompat = (src: AudioStream) =>
    tracks.push({
      sourceIndex: src.index,
      action: "encode",
      codec: stereoCodec,
      bitrateKbps: stereoRate,
      channels: 2,
      title: src.channels > 2 ? `${stereoCodec.toUpperCase()} 立体声（降混）` : src.title,
      role: "compat",
    });

  const copyOf = (a: AudioStream): AudioTrackPlan => ({
    sourceIndex: a.index,
    action: "copy",
    title: a.title,
    role: "original",
  });

  switch (plan.audioMode) {
    case "copy_all":
      for (const a of media.audio) tracks.push(copyOf(a));
      break;

    case "original_plus_compat":
      for (const a of media.audio) tracks.push(copyOf(a));
      if (primary.lossless || primary.atmos || primary.channels > 2) pushStereoCompat(primary);
      break;

    case "compat_only": {
      const single = plan.scenario === "mobile" || plan.scenario === "social" || plan.scenario === "smallest";
      const sources = single ? [primary] : media.audio;
      for (const a of sources) {
        const lossy = !a.lossless && !a.atmos;
        const fits = audioFitsContainer(a.codec, plan.container);
        if (lossy && fits && (!single || a.channels <= 2) && !(single && a.codec !== stereoCodec)) {
          tracks.push(copyOf(a));
        } else if (!single && a.channels > 2) {
          tracks.push({
            sourceIndex: a.index,
            action: "encode",
            codec: "eac3",
            bitrateKbps: 640,
            channels: 6,
            title: a.atmos ? "DD+ 5.1（由 Atmos 转换，不含全景声）" : "DD+ 5.1",
            role: "compat",
          });
        } else {
          pushStereoCompat(a);
        }
      }
      // 多声道主轨再追加一条立体声，保证耳机与手机可用
      if (!single && primary.channels > 2) pushStereoCompat(primary);
      break;
    }
  }
  return tracks;
}

/**
 * 让计划保持自洽。任何字段被修改后都应调用一次：
 * 编码格式变了要重选编码器、编码器变了要换质量数值、容器变了要重建音轨……
 */
export function normalizePlan(plan: TranscodePlan, media: MediaInfo, caps: Capabilities): TranscodePlan {
  const next: TranscodePlan = structuredClone(plan);
  const vp = next.video;

  if (vp.action === "encode") {
    if (!vp.encoderAuto && encoderCodec(vp.encoder) !== vp.codec) {
      // 手选的编码器与新编码格式不匹配，回到自动选择
      vp.encoderAuto = true;
    }
    if (vp.encoderAuto) {
      const before = vp.encoder;
      vp.encoder = pickEncoder(
        vp.codec,
        {
          preferHw: preferHwFor(next.scenario),
          need10bit: vp.bitDepth === 10,
          needDv: vp.dovi === "preserve",
          needHdr10: vp.hdrAction === "keep" && media.video[0]?.color.hdrKind === "hdr10",
        },
        caps,
      ).encoder;
      if (before !== vp.encoder) {
        vp.qualityValue = qualityValue(vp.encoder, vp.quality);
        vp.preset = defaultPreset(vp.encoder, next.scenario);
      }
    }
    // 硬件编码器不支持 10bit 时降为 8bit，保真度面板会给出提示
    if (vp.bitDepth === 10 && !encoderSupports10bit(vp.encoder, caps)) vp.bitDepth = 8;
    if (vp.hdrAction === "tonemap" && (!vp.tonemap || !tonemapAvailable(vp.tonemap, caps))) {
      vp.tonemap = pickTonemap(caps);
      if (!vp.tonemap) vp.hdrAction = "keep";
    }
    if (vp.hdrAction !== "tonemap") vp.tonemap = undefined;
  }

  next.audio = buildAudioTracks(media, next);
  return next;
}
