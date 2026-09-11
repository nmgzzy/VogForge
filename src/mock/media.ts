import type { AudioStream, MediaInfo, VideoStream } from "@/lib/types";

/**
 * 演示用媒体样本。覆盖用户会遇到的六类典型源，每类都至少触发一条引擎规则。
 * 数值参照真实设备的 ffprobe 输出量级。
 */

const BT2020_PQ = {
  primaries: "bt2020",
  transfer: "smpte2084",
  space: "bt2020nc",
  range: "tv" as const,
  hdrKind: "hdr10" as const,
};
const BT2020_HLG = { ...BT2020_PQ, transfer: "arib-std-b67", hdrKind: "hlg" as const };
const BT709 = {
  primaries: "bt709",
  transfer: "bt709",
  space: "bt709",
  range: "tv" as const,
  hdrKind: "none" as const,
};

function video(v: Partial<VideoStream> & Pick<VideoStream, "codec" | "width" | "height">): VideoStream {
  return {
    index: 0,
    fpsAvg: 30,
    fpsNominal: 30,
    isVfr: false,
    bitDepth: 8,
    pixFmt: "yuv420p",
    color: BT709,
    hdr10plus: false,
    rotation: 0,
    ...v,
  };
}

function audio(a: Partial<AudioStream> & Pick<AudioStream, "index" | "codec" | "channels">): AudioStream {
  return {
    channelLayout: a.channels === 2 ? "stereo" : a.channels === 6 ? "5.1" : a.channels === 8 ? "7.1" : "mono",
    sampleRate: 48000,
    isDefault: a.index === 1,
    lossless: false,
    atmos: false,
    dtsX: false,
    ...a,
  };
}

/** iPhone 拍摄：HEVC 10bit + HLG + 杜比视界 8.4 + 可变帧率 */
const iphone: MediaInfo = {
  id: "m-iphone",
  path: "D:\\素材\\2026-08 京都\\IMG_4521.MOV",
  name: "IMG_4521.MOV",
  container: "mov",
  durationSec: 134.6,
  sizeBytes: 1_318_000_000,
  bitrate: 78_300_000,
  video: [
    video({
      codec: "hevc",
      profile: "Main 10",
      width: 3840,
      height: 2160,
      fpsAvg: 29.41,
      fpsNominal: 30,
      isVfr: true,
      bitDepth: 10,
      pixFmt: "yuv420p10le",
      bitrate: 77_900_000,
      color: BT2020_HLG,
      dolbyVision: { profile: 8, blCompatId: 4, hasEnhancementLayer: false },
      frameCount: 3959,
    }),
  ],
  audio: [audio({ index: 1, codec: "aac", channels: 2, bitrate: 256_000, language: "und" })],
  subtitle: [],
  chapters: 0,
  attachments: 0,
  sourceHint: "iphone",
  device: "Apple iPhone 16 Pro",
};

/** 蓝光 remux：HDR10 + 杜比视界 P7 FEL + TrueHD Atmos + 多音轨 + PGS 字幕 */
const bluray: MediaInfo = {
  id: "m-bluray",
  path: "\\\\NAS\\Movies\\The.Long.Night.2025.2160p.UHD.BluRay.REMUX.DV.HDR.TrueHD.Atmos.7.1.mkv",
  name: "The.Long.Night.2025.2160p.UHD.BluRay.REMUX.DV.HDR.TrueHD.Atmos.7.1.mkv",
  container: "matroska",
  durationSec: 9_874,
  sizeBytes: 71_400_000_000,
  bitrate: 57_800_000,
  video: [
    video({
      codec: "hevc",
      profile: "Main 10",
      width: 3840,
      height: 2160,
      fpsAvg: 23.976,
      fpsNominal: 23.976,
      bitDepth: 10,
      pixFmt: "yuv420p10le",
      bitrate: 49_600_000,
      color: BT2020_PQ,
      hdr10: {
        maxLuminance: 1000,
        minLuminance: 0.0001,
        maxCll: 1000,
        maxFall: 400,
        masteringPrimaries: "p3",
      },
      dolbyVision: { profile: 7, blCompatId: 6, hasEnhancementLayer: true, elType: "FEL" },
      hdr10plus: false,
      frameCount: 236_745,
    }),
  ],
  audio: [
    audio({
      index: 1,
      codec: "truehd",
      channels: 8,
      bitrate: 5_200_000,
      language: "eng",
      title: "TrueHD 7.1 Atmos",
      lossless: true,
      atmos: true,
    }),
    audio({ index: 2, codec: "ac3", channels: 6, bitrate: 640_000, language: "eng", title: "AC-3 5.1" }),
    audio({
      index: 3,
      codec: "eac3",
      channels: 6,
      bitrate: 768_000,
      language: "chi",
      title: "国语 DD+ 5.1",
    }),
    audio({ index: 4, codec: "ac3", channels: 2, bitrate: 192_000, language: "eng", title: "导演评论音轨" }),
  ],
  subtitle: [
    { index: 5, codec: "hdmv_pgs_subtitle", language: "chi", title: "简体中文", imageBased: true },
    { index: 6, codec: "hdmv_pgs_subtitle", language: "chi", title: "繁体中文", imageBased: true },
    { index: 7, codec: "hdmv_pgs_subtitle", language: "eng", title: "English", imageBased: true },
    { index: 8, codec: "subrip", language: "eng", title: "English SDH", imageBased: false },
  ],
  chapters: 24,
  attachments: 0,
  sourceHint: "bluray",
};

/** 无人机：H.264 8bit 4K60 高码率，常见的"体积大但画质冗余"素材 */
const drone: MediaInfo = {
  id: "m-drone",
  path: "D:\\素材\\航拍\\DJI_20260812_0142.MP4",
  name: "DJI_20260812_0142.MP4",
  container: "mp4",
  durationSec: 318.2,
  sizeBytes: 5_730_000_000,
  bitrate: 144_000_000,
  video: [
    video({
      codec: "h264",
      profile: "High",
      width: 3840,
      height: 2160,
      fpsAvg: 59.94,
      fpsNominal: 59.94,
      bitrate: 143_700_000,
      frameCount: 19_073,
    }),
  ],
  audio: [],
  subtitle: [],
  chapters: 0,
  attachments: 0,
  sourceHint: "dji",
  device: "DJI Mavic 4 Pro",
};

/** 手机录屏：竖屏，帧率剧烈波动（静止时掉到个位数） */
const screen: MediaInfo = {
  id: "m-screen",
  path: "D:\\素材\\录屏\\Screen_Recording_20260901_213355.mp4",
  name: "Screen_Recording_20260901_213355.mp4",
  container: "mp4",
  durationSec: 412.0,
  sizeBytes: 186_000_000,
  bitrate: 3_610_000,
  video: [
    video({
      codec: "h264",
      profile: "High",
      width: 1080,
      height: 2400,
      fpsAvg: 17.3,
      fpsNominal: 60,
      isVfr: true,
      bitrate: 3_480_000,
      frameCount: 7_128,
    }),
  ],
  audio: [audio({ index: 1, codec: "aac", channels: 2, bitrate: 128_000 })],
  subtitle: [],
  chapters: 0,
  attachments: 0,
  sourceHint: "screen",
  device: "Xiaomi 15",
};

/** 已高度压缩的流媒体片源：用于演示"不建议转码" */
const streaming: MediaInfo = {
  id: "m-stream",
  path: "\\\\NAS\\Documentary\\深海纪录片.E03.1080p.WEB-DL.mp4",
  name: "深海纪录片.E03.1080p.WEB-DL.mp4",
  container: "mp4",
  durationSec: 2_874,
  sizeBytes: 1_290_000_000,
  bitrate: 3_590_000,
  video: [
    video({
      codec: "h264",
      profile: "High",
      width: 1920,
      height: 1080,
      fpsAvg: 25,
      fpsNominal: 25,
      bitrate: 3_400_000,
      frameCount: 71_850,
    }),
  ],
  audio: [
    audio({ index: 1, codec: "aac", channels: 2, bitrate: 192_000, language: "chi", title: "国语" }),
  ],
  subtitle: [{ index: 2, codec: "mov_text", language: "chi", title: "中文", imageBased: false }],
  chapters: 0,
  attachments: 0,
  sourceHint: "streaming",
};

/** 相机：10bit 4:2:2 All-I + HLG + 线性 PCM */
const camera: MediaInfo = {
  id: "m-camera",
  path: "D:\\素材\\婚礼\\C0087.MP4",
  name: "C0087.MP4",
  container: "mp4",
  durationSec: 206.4,
  sizeBytes: 15_480_000_000,
  bitrate: 600_000_000,
  video: [
    video({
      codec: "h264",
      profile: "High 4:2:2 Intra",
      width: 3840,
      height: 2160,
      fpsAvg: 25,
      fpsNominal: 25,
      bitDepth: 10,
      pixFmt: "yuv422p10le",
      bitrate: 598_000_000,
      color: BT2020_HLG,
      frameCount: 5_160,
    }),
  ],
  audio: [
    audio({
      index: 1,
      codec: "pcm_s24le",
      channels: 2,
      bitrate: 2_304_000,
      lossless: true,
    }),
  ],
  subtitle: [],
  chapters: 0,
  attachments: 0,
  sourceHint: "camera",
  device: "Sony ILCE-7M4",
};

export const MOCK_MEDIA: MediaInfo[] = [iphone, bluray, drone, screen, camera, streaming];
