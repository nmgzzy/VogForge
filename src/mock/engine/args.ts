import type { AudioStream, Capabilities, MediaInfo, TranscodePlan, VideoStream } from "@/lib/types";
import { encoderVendor, isHardware } from "./encoders";
import { fpsArg } from "./fps";
import { splitArgs } from "@/lib/format";

/**
 * 命令构建。每条规则都对应 docs/ffmpeg-facts.md 里的一条已核实事实，
 * 修改时请同步更新文档与测试。
 *
 * 返回分段结构，便于命令预览按段换行；复制时再拍平。
 */
export interface ArgSegment {
  label: string;
  args: string[];
}

export const OUTPUT_DIR = "D:\\转码输出";

export function targetDimensions(v: VideoStream, preset: TranscodePlan["video"]["resolution"]) {
  if (preset === "source") return null;
  const target = Number(preset);
  const short = Math.min(v.width, v.height);
  if (target >= short) return null; // 绝不放大
  const portrait = v.height > v.width;
  const scale = target / short;
  const even = (n: number) => Math.round(n / 2) * 2;
  return portrait
    ? { w: target, h: even(v.height * scale), portrait }
    : { w: even(v.width * scale), h: target, portrait };
}

export function outputPath(media: MediaInfo, plan: TranscodePlan): string {
  const base = media.name.replace(/\.[^.]+$/, "");
  const v = media.video[0];
  if (plan.video.action === "copy") return `${OUTPUT_DIR}\\${base}_remux.${plan.container}`;
  const dims = v ? targetDimensions(v, plan.video.resolution) : null;
  const short = v ? (dims ? Math.min(dims.w, dims.h) : Math.min(v.width, v.height)) : 0;
  return `${OUTPUT_DIR}\\${base}_${short}p_${plan.video.codec}.${plan.container}`;
}

const MUXER: Record<TranscodePlan["container"], string> = { mkv: "matroska", mp4: "mp4", mov: "mov" };

/** 5.1 与 7.1 的声道名不同，pan 引用不存在的声道会直接报错，所以按布局分别生成 */
export function downmixFilter(src: AudioStream): string {
  const layout = src.channelLayout;
  let expr: string;
  if (src.channels >= 8 || /7\.1/.test(layout)) {
    expr = "FL=0.707*FC+1.0*FL+0.6*BL+0.6*SL|FR=0.707*FC+1.0*FR+0.6*BR+0.6*SR";
  } else if (/side/.test(layout)) {
    expr = "FL=0.707*FC+1.0*FL+0.707*SL|FR=0.707*FC+1.0*FR+0.707*SR";
  } else {
    expr = "FL=0.707*FC+1.0*FL+0.707*BL|FR=0.707*FC+1.0*FR+0.707*BR";
  }
  // 中置提升约 3dB 让对白更清楚，alimiter 防止叠加后削波；LFE 不混入立体声
  return `pan=stereo|${expr},alimiter=limit=0.97:level=false`;
}

function hwaccelFor(plan: TranscodePlan, media: MediaInfo): string[] {
  const vp = plan.video;
  if (vp.action === "copy") return [];
  // 硬解后的 hwdownload 可能丢失 DV RPU 与 HDR10+ 等 side data，保留这些时走全软件路径
  if (vp.dovi === "preserve" || media.video[0]?.hdr10plus) return [];
  if (!isHardware(vp.encoder)) return ["-hwaccel", "auto"];
  switch (encoderVendor(vp.encoder)) {
    case "intel":
      return ["-hwaccel", "qsv"];
    case "nvidia":
      return ["-hwaccel", "cuda"];
    case "amd":
      return ["-hwaccel", "d3d11va"];
    case "apple":
      return ["-hwaccel", "videotoolbox"];
    default:
      return [];
  }
}

/**
 * 视频滤镜。`hwaccel` 有值时替换默认的硬解参数：scale_vt 只接受 VideoToolbox 硬件帧，
 * 必须配 `-hwaccel_output_format videotoolbox_vld`。
 */
function videoFilters(plan: TranscodePlan, v: VideoStream): { vf?: string; pre: string[]; hwaccel?: string[] } {
  const vp = plan.video;
  const dims = targetDimensions(v, vp.resolution);
  const scaleExpr = dims ? (dims.portrait ? `scale=${dims.w}:-2:flags=lanczos` : `scale=-2:${dims.h}:flags=lanczos`) : "";

  // tonemap 为空说明没有可用管线，normalizePlan 已退回保留 HDR；这里不再猜测默认值
  if (vp.hdrAction === "tonemap" && vp.tonemap) {
    const dv = v.dolbyVision ? ":apply_dolbyvision=1" : "";
    switch (vp.tonemap) {
      case "scale_vt": {
        // scale_vt 只做色彩空间转换与缩放，不做感知色调映射（docs/ffmpeg-facts.md 5.4），仅作兜底
        const size = dims ? `w=${dims.w}:h=${dims.h}:` : "";
        const chain = [`scale_vt=${size}color_matrix=bt709:color_primaries=bt709:color_transfer=bt709`];
        if (!isHardware(vp.encoder)) chain.push("hwdownload", "format=nv12");
        return {
          pre: [],
          hwaccel: ["-hwaccel", "videotoolbox", "-hwaccel_output_format", "videotoolbox_vld"],
          vf: chain.join(","),
        };
      }
      case "libplacebo": {
        const size = dims ? `w=${dims.w}:h=${dims.h}:downscaler=mitchell:` : "";
        return {
          pre: [],
          vf:
            `libplacebo=${size}tonemapping=bt.2390:peak_detect=1:gamut_mode=perceptual${dv}` +
            ":colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv:format=yuv420p",
        };
      }
      case "tonemap_opencl":
        return {
          pre: ["-init_hw_device", "opencl=ocl", "-filter_hw_device", "ocl"],
          vf: [
            "format=p010",
            "hwupload",
            "tonemap_opencl=tonemap=hable:desat=0:t=bt709:m=bt709:p=bt709:r=tv:format=nv12",
            "hwdownload",
            "format=nv12",
            scaleExpr,
          ]
            .filter(Boolean)
            .join(","),
        };
      case "zscale":
        return {
          pre: [],
          vf: [
            "zscale=t=linear:npl=100",
            "format=gbrpf32le",
            "zscale=p=bt709",
            "tonemap=tonemap=hable:desat=0",
            "zscale=t=bt709:m=bt709:r=tv",
            "format=yuv420p",
            scaleExpr,
          ]
            .filter(Boolean)
            .join(","),
        };
    }
  }
  return { pre: [], vf: scaleExpr || undefined };
}

function encoderArgs(plan: TranscodePlan, v: VideoStream): string[] {
  const vp = plan.video;
  const ten = vp.bitDepth === 10;
  const q = String(vp.qualityValue);
  const out: string[] = ["-c:v", vp.encoder];

  switch (vp.encoder) {
    case "libx265": {
      out.push("-pix_fmt", ten ? "yuv420p10le" : "yuv420p", "-preset", vp.preset, "-crf", q);
      const params = ["repeat-headers=1"];
      // HDR10 元数据由 ffmpeg 自动透传，这里只打开码率分配优化，不手写 master-display
      if (vp.hdrAction === "keep" && v.color.hdrKind === "hdr10") params.unshift("hdr10-opt=1");
      if (vp.extraParams) params.push(vp.extraParams);
      out.push("-x265-params", params.join(":"));
      break;
    }
    case "libx264":
      out.push("-pix_fmt", ten ? "yuv420p10le" : "yuv420p", "-profile:v", ten ? "high10" : "high");
      out.push("-preset", vp.preset, "-crf", q);
      break;
    case "libsvtav1":
      out.push("-pix_fmt", ten ? "yuv420p10le" : "yuv420p", "-preset", vp.preset, "-crf", q);
      out.push("-svtav1-params", "tune=0");
      break;
    case "hevc_qsv":
    case "h264_qsv":
    case "av1_qsv":
      // QSV 10bit 必须 p010le；7.0 起默认 RC 变为 CQP，这里用 global_quality 显式指定
      out.push("-pix_fmt", ten ? "p010le" : "nv12");
      if (ten && vp.encoder === "hevc_qsv") out.push("-profile:v", "main10");
      out.push("-preset", vp.preset, "-global_quality", q);
      break;
    case "hevc_nvenc":
    case "h264_nvenc":
    case "av1_nvenc":
      out.push("-pix_fmt", ten ? "p010le" : "yuv420p");
      if (ten && vp.encoder === "hevc_nvenc") out.push("-profile:v", "main10");
      // -cq 必须配 -rc vbr -b:v 0，否则会被码率约束
      out.push("-preset", vp.preset, "-tune", "hq", "-rc", "vbr", "-b:v", "0", "-cq", q);
      break;
    case "hevc_amf":
    case "h264_amf":
    case "av1_amf":
      out.push("-quality", vp.preset, "-rc", "cqp", "-qp_i", q, "-qp_p", q);
      break;
    case "hevc_videotoolbox":
    case "h264_videotoolbox":
      if (ten && vp.encoder === "hevc_videotoolbox") out.push("-profile:v", "main10");
      out.push("-q:v", q);
      break;
  }

  if (vp.gop) out.push("-g", String(vp.gop));

  // 源含杜比视界时必须显式表态：留空会被 auto 自动开启
  if (v.dolbyVision && (vp.encoder === "libx265" || vp.encoder === "libsvtav1")) {
    out.push("-dolbyvision", vp.dovi === "preserve" ? "1" : "0");
  }

  if (vp.hdrAction === "tonemap") {
    out.push("-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709");
  } else if (v.color.hdrKind !== "none") {
    out.push(
      "-color_primaries", "bt2020",
      "-color_trc", v.color.hdrKind === "hlg" ? "arib-std-b67" : "smpte2084",
      "-colorspace", "bt2020nc",
      "-color_range", "tv",
    );
  }
  // 支持引号：-metadata title="My Video" 应是两个 argv，而不是按空白拆成三段
  if (vp.extraArgs) out.push(...splitArgs(vp.extraArgs));
  return out;
}

export function buildArgSegments(media: MediaInfo, plan: TranscodePlan, _caps: Capabilities): ArgSegment[] {
  const v = media.video[0];
  const vp = plan.video;
  const segs: ArgSegment[] = [];
  const filters: ReturnType<typeof videoFilters> = v && vp.action === "encode" ? videoFilters(plan, v) : { pre: [] };

  segs.push({
    label: "全局",
    args: ["ffmpeg", "-hide_banner", "-nostdin", "-loglevel", "warning", "-progress", "pipe:1", "-nostats"],
  });
  const input = [...(filters.hwaccel ?? hwaccelFor(plan, media)), ...filters.pre, "-i", media.path];
  segs.push({ label: "输入", args: input });

  // 映射
  const map: string[] = ["-map", "0:v:0"];
  for (const t of plan.audio) map.push("-map", `0:${t.sourceIndex}`);
  if (plan.subtitles === "all" && media.subtitle.length > 0) {
    if (plan.container === "mkv") map.push("-map", "0:s?");
    else for (const s of media.subtitle.filter((x) => !x.imageBased)) map.push("-map", `0:${s.index}`);
  } else if (plan.subtitles === "text_only") {
    for (const s of media.subtitle.filter((x) => !x.imageBased)) map.push("-map", `0:${s.index}`);
  }
  if (media.chapters > 0) map.push("-map_chapters", "0");
  map.push("-map_metadata", "0");
  segs.push({ label: "映射", args: map });

  // 视频
  if (vp.action === "copy" || !v) {
    segs.push({ label: "视频", args: ["-c:v", "copy"] });
  } else {
    segs.push({ label: "视频", args: encoderArgs(plan, v) });
    if (filters.vf) segs.push({ label: "滤镜", args: ["-vf", filters.vf] });
    if (vp.fps.kind === "cfr") {
      // 实测：fps 滤镜会丢最后一帧，-vsync 在 9.0 已移除，唯一正确写法是 -fps_mode:v cfr 加 -r
      segs.push({ label: "帧率", args: ["-fps_mode:v", "cfr", "-r", fpsArg(vp.fps.fps)] });
    } else if (vp.fps.kind === "cap" && v.fpsNominal > vp.fps.max) {
      segs.push({ label: "帧率", args: ["-fps_mode:v", "cfr", "-r", fpsArg(vp.fps.max)] });
    }
  }

  // 音频
  const audio: string[] = [];
  plan.audio.forEach((t, i) => {
    const src = media.audio.find((a) => a.index === t.sourceIndex);
    if (t.action === "copy") {
      audio.push(`-c:a:${i}`, "copy");
    } else {
      audio.push(`-c:a:${i}`, t.codec ?? "aac");
      if (t.bitrateKbps) audio.push(`-b:a:${i}`, `${t.bitrateKbps}k`);
      const af: string[] = [];
      const usePan = !!src && t.channels === 2 && src.channels > 2;
      if (usePan) af.push(downmixFilter(src));
      if (vp.fps.kind === "cfr") af.push("aresample=async=1");
      if (af.length) audio.push(`-filter:a:${i}`, af.join(","));
      // pan 已确定声道布局，之后不能再加 -ac，否则会被二次重混
      if (!usePan && t.channels && src && src.channels !== t.channels) {
        audio.push(`-ac:a:${i}`, String(t.channels));
      }
    }
    if (t.title) audio.push(`-metadata:s:a:${i}`, `title=${t.title}`);
  });
  if (audio.length) segs.push({ label: "音频", args: audio });

  // 字幕
  const hasSubs =
    plan.subtitles === "all"
      ? media.subtitle.length > 0
      : plan.subtitles === "text_only" && media.subtitle.some((s) => !s.imageBased);
  if (hasSubs) segs.push({ label: "字幕", args: ["-c:s", plan.container === "mkv" ? "copy" : "mov_text"] });

  // 容器
  const mux: string[] = [];
  if (plan.container !== "mkv") {
    const hevcOut = vp.action === "copy" ? v?.codec === "hevc" : vp.codec === "hevc";
    if (hevcOut) mux.push("-tag:v", "hvc1"); // Apple 全家只认 hvc1，ffmpeg 默认写 hev1
    const dvOut = vp.action === "copy" ? !!v?.dolbyVision : vp.dovi === "preserve";
    if (dvOut) mux.push("-strict", "unofficial"); // 否则 dvcC/dvvC box 不写入
    mux.push("-movflags", "+faststart");
  }
  // 实际写入 .vidforge-part 临时文件，扩展名无法推断格式，必须显式 -f
  mux.push("-f", MUXER[plan.container]);
  segs.push({ label: "封装", args: mux });

  segs.push({ label: "输出", args: [outputPath(media, plan)] });
  return segs;
}

export function flattenArgs(segs: ArgSegment[]): string[] {
  return segs.flatMap((s) => s.args);
}
