import type { Capabilities, Decision, Estimate, FpsInsight, MediaInfo, QualityTier, TranscodePlan } from "@/lib/types";
import { formatFps, formatPercent } from "@/lib/format";
import { CODEC_LABEL, encoderVendor, isHardware, qualityMeta, VENDOR_LABEL } from "./encoders";
import { isExtremeVfr } from "./fps";
import { targetDimensions } from "./args";
import { preferHwFor } from "./recommend";
import { pickTonemap } from "./color";

const TIER_LABEL: Record<QualityTier, string> = {
  lossless: "视觉无损",
  high: "高画质",
  standard: "标准",
  small: "小体积",
};

const TONEMAP_LABEL = {
  libplacebo: "libplacebo",
  tonemap_opencl: "OpenCL",
  zscale: "zscale（CPU）",
  scale_vt: "VideoToolbox",
} as const;

/**
 * 为当前计划生成"为什么这么选"。依据当前计划推导，而不是记录推荐时的理由，
 * 这样用户改了参数后理由会同步更新。
 */
export function explain(
  media: MediaInfo,
  plan: TranscodePlan,
  caps: Capabilities,
  fps?: FpsInsight,
  est?: Estimate,
): Decision[] {
  const v = media.video[0];
  const vp = plan.video;
  const out: Decision[] = [];
  const add = (field: string, value: string, reason: string, severity: Decision["severity"] = "info") =>
    out.push({ field, value, reason, severity });

  if (!v) return out;

  if (vp.action === "copy") {
    add("视频", "原样复制", "不重新编码，画质零损失，速度只受磁盘读写限制");
    add("容器", plan.container.toUpperCase(), "MKV 能完整容纳杜比视界、无损音轨、图形字幕与章节");
    return out;
  }

  // ── 编码器 ──
  const vendor = encoderVendor(vp.encoder);
  if (!vp.encoderAuto) {
    add("编码器", vp.encoder, "你手动指定了编码器，自动选择已关闭");
  } else if (vp.dovi === "preserve") {
    add("编码器", vp.encoder, "杜比视界的逐帧元数据只能由软件编码器写入，因此本次不使用 GPU 编码");
  } else if (isHardware(vp.encoder)) {
    add(
      "编码器",
      vp.encoder,
      `使用 ${VENDOR_LABEL[vendor]}（启动时已真实试编码验证可用），速度约为软编的 5–10 倍；同画质下体积会大 15–30%`,
    );
  } else if (preferHwFor(plan.scenario)) {
    add("编码器", vp.encoder, `没有可用的 ${CODEC_LABEL[vp.codec]} 硬件编码器，已改用软件编码`, "warn");
  } else if (plan.scenario === "editing") {
    add("编码器", vp.encoder, "剪辑素材要经得起调色与二次导出，用软件编码保证画质；硬件编码的码率控制不够稳定");
  } else {
    add("编码器", vp.encoder, "软件编码在同体积下画质最好，适合长期保存；硬件编码更快但同画质体积更大");
  }

  // ── 编码格式 ──
  if (plan.scenario === "editing") {
    add(
      "编码格式",
      CODEC_LABEL[vp.codec],
      vp.codec === "h264"
        ? "H.264 在各剪辑软件中解码最流畅，时间线拖动不卡"
        : "源为 HDR，用 HEVC 10bit 才能保留 HDR；达芬奇与 Final Cut 均支持",
    );
  } else if (vp.codec === "hevc" && v.codec === "h264") {
    add("编码格式", "HEVC", "同画质下 HEVC 比 H.264 体积小约 40–50%，2016 年后的设备普遍能硬解播放");
  } else if (vp.codec === "av1") {
    add("编码格式", "AV1", "AV1 比 HEVC 再省约 20–30%，但 2020 年前的设备多数无法硬解播放", "tip");
  } else if (vp.codec === "h264") {
    add("编码格式", "H.264", "H.264 兼容性最好，几乎所有设备和平台都能直接播放");
  }

  // ── 质量 ──
  const meta = qualityMeta(vp.encoder);
  add(
    "画质",
    `${TIER_LABEL[vp.quality]} · ${meta.param} ${vp.qualityValue}`,
    `${meta.param} ${meta.lowerIsBetter ? "越小" : "越大"}画质越好。不同编码器的数值刻度不等价，档位已按编码器分别换算`,
  );

  // ── 位深 ──
  if (vp.bitDepth === 10) {
    if (v.color.hdrKind !== "none" && vp.hdrAction === "keep") {
      add("位深", "10bit", "HDR 必须 10bit，8bit 会在天空与暗部出现明显色带");
    } else if (v.bitDepth === 8) {
      add("位深", "10bit", "8bit 源用 10bit 编码能减少渐变处的色带，体积几乎不变");
    }
  }

  // ── HDR ──
  if (v.color.hdrKind !== "none") {
    const kind = v.color.hdrKind === "hlg" ? "HLG" : "HDR10";
    const wantsSdr = plan.scenario === "mobile" || plan.scenario === "social";
    if (vp.hdrAction === "keep" && wantsSdr && !pickTonemap(caps)) {
      add(
        "HDR",
        `无法转为 SDR，保留 ${kind}`,
        "当前 ffmpeg 没有任何可用的色调映射滤镜（libplacebo / OpenCL / zscale）。在普通屏幕上可能发灰，建议换用带 libplacebo 或 zscale 的构建",
        "warn",
      );
    } else if (vp.hdrAction === "tonemap") {
      const pipe = TONEMAP_LABEL[vp.tonemap ?? "libplacebo"];
      const dvNote = v.dolbyVision && vp.tonemap === "libplacebo" ? "，并利用杜比视界元数据提升映射准确度" : "";
      add("HDR", `色调映射为 SDR`, `源为 ${kind}，目标多为 SDR 屏幕。使用 ${pipe} 做色调映射${dvNote}，避免画面发灰`);
    } else if (isHardware(vp.encoder) && kind === "HDR10") {
      add("HDR", `保留 ${kind}`, `${VENDOR_LABEL[vendor]} 会把 HDR10 元数据写入码流，已在本机实测验证`);
    } else {
      add("HDR", `保留 ${kind}`, kind === "HLG" ? "保留 HLG 色彩标记，HDR 电视与手机可直接识别" : "母版显示与 MaxCLL 元数据由 ffmpeg 自动透传");
    }
  }

  // ── 杜比视界 ──
  const dv = v.dolbyVision;
  if (dv) {
    if (dv.hasEnhancementLayer) {
      add(
        "杜比视界",
        "仅保留基础层",
        `源为 Profile ${dv.profile} 双层。ffmpeg 无法编码增强层，重编码后只剩 HDR10 基础层。要完整保留，请改为"原样封装"`,
        "warn",
      );
    } else if (vp.dovi === "preserve") {
      if (dv.profile === 5) {
        add("杜比视界", "保留 Profile 5", "Profile 5 没有 HDR10 回退层，不支持杜比视界的设备会显示偏绿或偏紫", "warn");
      } else {
        add("杜比视界", `保留 Profile ${dv.profile}.${dv.blCompatId}`, "传入 -dolbyvision 1，保留失败时会明确报错而不是静默丢弃");
      }
    } else {
      add(
        "杜比视界",
        "不保留",
        `已显式传入 -dolbyvision 0（ffmpeg 默认会自动开启），输出将以 ${v.color.hdrKind === "hlg" ? "HLG" : "HDR10"} 播放`,
      );
    }
  }

  // ── 分辨率 ──
  const dims = targetDimensions(v, vp.resolution);
  if (dims) {
    add("分辨率", `${dims.w}×${dims.h}`, `从 ${v.width}×${v.height} 缩小，使用 lanczos 保留细节`);
  } else if (vp.resolution !== "source") {
    add("分辨率", "保持原始", `目标分辨率不低于源（${v.width}×${v.height}），不做放大`, "tip");
  }

  // ── 帧率 ──
  if (vp.fps.kind === "cfr" && fps) {
    const target = formatFps(fps.targetFps);
    if (v.isVfr && isExtremeVfr(v)) {
      const pct = formatPercent(fps.duplicated / Math.max(fps.targetFrames, 1));
      add(
        "帧率",
        `${target} fps 固定`,
        `源平均仅 ${formatFps(v.fpsAvg)} fps，会复制 ${fps.duplicated.toLocaleString()} 帧（占 ${pct}）。重复帧几乎不占体积，但编码更慢；剪辑也可降到 30 fps`,
        "warn",
      );
    } else if (v.isVfr) {
      add(
        "帧率",
        `${target} fps 固定`,
        `导入剪辑软件不会逐渐音画错位；复制约 ${fps.duplicated.toLocaleString()} 帧补齐时间轴`,
      );
    } else {
      add("帧率", `${target} fps 固定`, "源本身已是固定帧率，输出保持一致");
    }
  } else if (vp.fps.kind === "keep" && v.isVfr) {
    add("帧率", "保持可变帧率", "保持原始时间戳，播放没有问题；之后要剪辑的话，在帧率里打开“转为固定帧率”");
  }

  if (vp.gop) {
    add("关键帧", `每 ${vp.gop} 帧`, "关键帧间隔约 0.5 秒，剪辑软件拖动时间线更流畅，代价是体积略增");
  }

  // ── 耗时 ──
  const LONG_ENCODE_SEC = 3 * 3600;
  if (est && !isHardware(vp.encoder) && est.timeMaxSec > LONG_ENCODE_SEC) {
    const faster = vp.preset === "slow" || vp.preset === "slower" || vp.preset === "veryslow";
    add(
      "耗时",
      `可能超过 ${Math.round(est.timeMaxSec / 3600)} 小时`,
      faster
        ? `CPU 以 ${vp.preset} 速度编码 ${v.width}×${v.height} 很慢。不在意极致压缩率的话，可在“更多参数”里改为 medium，速度约快 2 倍，体积仅增加 5% 左右`
        : "CPU 编码高分辨率长片耗时较长，可以放在队列里夜间运行",
      "tip",
    );
  }

  // ── 音频 ──
  const hq = media.audio.find((a) => a.atmos || a.lossless);
  const hqCopied = hq && plan.audio.some((t) => t.sourceIndex === hq.index && t.action === "copy");
  const hqEncoded = hq && plan.audio.some((t) => t.sourceIndex === hq.index && t.action === "encode");
  const hasCompat = plan.audio.some((t) => t.role === "compat");
  if (hq?.atmos && hqCopied) {
    add(
      "音频",
      "Atmos 原样保留",
      `全景声无法重新编码（需要杜比商业授权），只能原样复制${hasCompat ? "；已额外生成兼容轨供手机和耳机使用" : ""}`,
    );
  } else if (hq?.atmos && hqEncoded && !hqCopied) {
    add("音频", "Atmos 转为兼容格式", "全景声元数据会丢失，只保留声道混音。若要保留，请在保真度里勾选「全景声与无损音轨」", "warn");
  }
  if (plan.audio.some((t) => t.role === "compat" && t.channels === 2) && media.audio.some((a) => a.channels > 2)) {
    add("降混", "中置 +3dB", "多声道降为立体声时提升中置声道，对白更清楚，并用限幅器防止削波");
  }

  // ── 容器 ──
  if (plan.container === "mkv") {
    add("容器", "MKV", "能完整容纳杜比视界、无损音轨、图形字幕与章节");
  } else if (plan.container === "mp4") {
    add("容器", "MP4", "兼容性最好；已加 faststart 便于边下边播，HEVC 标记为 hvc1 以兼容苹果设备");
  } else {
    add("容器", "MOV", "剪辑软件最友好的容器");
  }

  return out;
}
