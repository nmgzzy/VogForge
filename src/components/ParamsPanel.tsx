import { useMemo } from "react";
import { Info } from "lucide-react";
import { tr } from "@/i18n";
import type { Codec, Container, FpsInsight, MediaInfo, PlanResult, QualityTier, TranscodePlan } from "@/lib/types";
import { cn } from "@/lib/cn";
import { CODEC_LABEL, canTonemap, codecAvailable, qualityValue, vendorLabel } from "@/lib/encoders";
import { encoderMeta, engineMeta, videoHints } from "@/lib/engine";
import { channelLabel, formatBitrate, formatFps } from "@/lib/format";
import { useEngineCaps } from "@/stores/engine-caps";
import { useProject } from "@/stores/project";
import { Badge, Field, Section, Segmented, Select, Switch } from "./ui";

const qualityOptions = (): { value: QualityTier; label: string }[] => [
  { value: "lossless", label: tr("视觉无损", "Lossless") },
  { value: "high", label: tr("高", "High") },
  { value: "standard", label: tr("标准", "Standard") },
  { value: "small", label: tr("小体积", "Small") },
];

const RESOLUTIONS = ["2160", "1440", "1080", "720", "480"] as const;

/** 帧率：一行开关 + 目标帧率 + 帧数变化；只在需要时多一行提示 */
export function FpsControl({ media, plan, insight }: { media: MediaInfo; plan: TranscodePlan; insight?: FpsInsight }) {
  const patch = useProject((s) => s.patchPlan);
  const hints = useMemo(() => videoHints(media), [media]);
  const v = media.video[0];
  if (!v || !hints) return null;
  const cfr = plan.video.fps.kind === "cfr";
  const recommended = hints.recommendedFps;
  const target = plan.video.fps.kind === "cfr" ? plan.video.fps.fps : recommended;

  let hint: { text: string; tone: "vfr" | "warn" } | undefined;
  if (v.isVfr && !cfr) {
    const text = tr(
      "导入剪辑软件前建议开启，否则音画会随时间逐渐错位",
      "Turn this on before importing into an editor, or audio will slowly drift out of sync",
    );
    hint = { text, tone: "vfr" };
  } else if (cfr && hints.extremeVfr) {
    const text = tr(
      "源帧率波动大，会复制大量帧，编码耗时明显增加",
      "The source frame rate varies a lot, so many frames are duplicated and encoding takes noticeably longer",
    );
    hint = { text, tone: "warn" };
  }

  return (
    <div>
      <div className="flex min-h-9 flex-wrap items-center gap-x-3 gap-y-1.5 rounded-md border border-line bg-raised/40 px-3 py-1.5">
        <Switch
          checked={cfr}
          label={tr("转为固定帧率", "Convert to constant frame rate")}
          onChange={(on) =>
            patch((p) => {
              p.video.fps = on ? { kind: "cfr", fps: recommended } : { kind: "keep" };
            })
          }
        />
        <span className="text-[13px]">{tr("转为固定帧率", "Convert to constant frame rate")}</span>
        {v.isVfr ? (
          <Badge
            tone="vfr"
            title={tr(
              `平均 ${formatFps(v.fpsAvg)} fps，名义 ${formatFps(v.fpsNominal)} fps`,
              `average ${formatFps(v.fpsAvg)} fps, nominal ${formatFps(v.fpsNominal)} fps`,
            )}
          >
            {tr("源为可变帧率", "Variable frame rate source")}
          </Badge>
        ) : (
          <span className="text-xs text-subtle">
            {tr(`源为固定 ${formatFps(v.fpsAvg)} fps`, `Source is constant ${formatFps(v.fpsAvg)} fps`)}
          </span>
        )}
        {cfr && (
          <>
            <Select
              className="w-32"
              value={String(target)}
              onChange={(val) =>
                patch((p) => {
                  p.video.fps = { kind: "cfr", fps: Number(val) };
                })
              }
              options={engineMeta().standardFps.map((s) => ({
                value: String(s.value),
                label: `${s.label} fps${Math.abs(s.value - recommended) < 1e-6 ? tr(" 推荐", " (recommended)") : ""}`,
              }))}
            />
            {insight && (
              <span
                className="ml-auto text-xs text-muted tabular"
                title={tr(
                  "转换后用 ffprobe 校验音视频时长差小于 1 帧",
                  "After encoding, ffprobe checks that audio and video differ by less than one frame",
                )}
              >
                {insight.sourceFrames.toLocaleString()} → {insight.targetFrames.toLocaleString()} {tr("帧", "frames")}
                {insight.duplicated > 0 &&
                  tr(`，复制 ${insight.duplicated.toLocaleString()}`, `, ${insight.duplicated.toLocaleString()} duplicated`)}
                {insight.dropped > 0 &&
                  tr(`，丢弃 ${insight.dropped.toLocaleString()}`, `, ${insight.dropped.toLocaleString()} dropped`)}
              </span>
            )}
          </>
        )}
      </div>
      {hint && <p className={cn("mt-1 text-[11.5px]", hint.tone === "vfr" ? "text-vfr" : "text-warn")}>{hint.text}</p>}
    </div>
  );
}

function AudioChips({ media, plan }: { media: MediaInfo; plan: TranscodePlan }) {
  if (plan.audio.length === 0) return <span className="text-xs text-subtle">{tr("源文件没有音轨", "The source has no audio")}</span>;
  return (
    <>
      {plan.audio.map((t, i) => {
        const src = media.audio.find((a) => a.index === t.sourceIndex);
        const name = t.action === "copy" ? (src?.title ?? src?.codec.toUpperCase() ?? tr("音轨", "Track")) : t.title ?? src?.title;
        const detail =
          t.action === "copy"
            ? tr("复制", "copy")
            : `${t.codec?.toUpperCase()} ${t.channels ? channelLabel(t.channels) : ""} ${t.bitrateKbps}k`;
        return (
          <span
            key={i}
            title={t.role === "compat" ? tr("新增的兼容音轨", "Added compatible track") : undefined}
            className={cn(
              "inline-flex h-7 max-w-[240px] items-center gap-1.5 rounded-md border px-2 text-[11.5px]",
              t.role === "compat" ? "border-dashed border-line-strong" : "border-line bg-raised/50",
            )}
          >
            <span className="truncate">{t.action === "copy" ? name : detail}</span>
            <span className={cn("shrink-0", src?.atmos && t.action === "copy" ? "text-atmos" : "text-subtle")}>
              {t.action === "copy" ? tr("· 复制", "· copy") : tr("· 新增", "· new")}
            </span>
          </span>
        );
      })}
    </>
  );
}

export function ParamsPanel({ media, plan, result }: { media: MediaInfo; plan: TranscodePlan; result: PlanResult }) {
  const patch = useProject((s) => s.patchPlan);
  const caps = useEngineCaps();
  const v = media.video[0];
  const vp = plan.video;
  const copy = vp.action === "copy";
  const isHdr = !!v && v.color.hdrKind !== "none";
  const meta = encoderMeta(vp.encoder);
  const rc = vp.rateControl;
  // 按码率编码时画质档位不起作用；限峰值仍按档位编码
  const byBitrate = rc.kind === "bitrate" || rc.kind === "two_pass";
  const shortEdge = v ? Math.min(v.width, v.height) : 0;

  const encoderOptions = [
    { value: "auto", label: tr(`自动（${vp.encoder}）`, `Auto (${vp.encoder})`) },
    ...caps.encoders
      .filter((e) => e.codec === vp.codec)
      .map((e) => ({
        value: e.id,
        label: `${e.id} · ${vendorLabel(e.vendor)}${e.usable ? "" : tr("（不可用）", " (unavailable)")}`,
        disabled: !e.usable,
      })),
  ];

  return (
    <Section step={2} title={tr("关键参数", "Key settings")}>
      {copy && (
        <p className="mb-3 flex items-center gap-1.5 text-xs text-muted">
          <Info className="size-3.5" />
          {tr("原样封装不重新编码视频，画面参数不可调整。", "Remux does not re-encode the video, so picture settings cannot change.")}
        </p>
      )}

      <div className={cn("grid gap-x-5 gap-y-3.5 md:grid-cols-2", copy && "pointer-events-none opacity-40")}>
        <Field
          label={tr("画质", "Quality")}
          hint={
            byBitrate
              ? `${rc.kind === "two_pass" ? tr("两遍 · ", "2-pass · ") : ""}${tr("平均", "avg")} ${formatBitrate(rc.kbps * 1000)}`
              : `${meta.param} ${vp.qualityValue}${rc.kind === "capped" ? ` · ${tr("峰值", "peak")} ${formatBitrate(rc.kbps * 1000)}` : ""}`
          }
        >
          <Segmented
            className={cn("w-full", byBitrate && "pointer-events-none opacity-40")}
            value={vp.quality}
            options={qualityOptions()}
            onChange={(q) =>
              patch((p) => {
                p.video.quality = q;
                p.video.qualityValue = qualityValue(p.video.encoder, q);
              })
            }
          />
        </Field>

        <Field label={tr("编码格式", "Format")}>
          <Segmented<Codec>
            className="w-full"
            value={vp.codec}
            options={(["h264", "hevc", "av1"] as const).map((c) => {
              // 探测完成前（probing）不置灰，避免界面在启动时闪一下
              const off = caps.status === "ready" && !codecAvailable(c, caps);
              return {
                value: c,
                label: CODEC_LABEL[c],
                disabled: off,
                title: off ? tr(`当前 ffmpeg 没有可用的 ${CODEC_LABEL[c]} 编码器`, `This ffmpeg has no usable ${CODEC_LABEL[c]} encoder`) : undefined,
              };
            })}
            onChange={(c) =>
              patch((p) => {
                p.video.codec = c;
                p.video.encoderAuto = true;
              })
            }
          />
        </Field>

        <Field label={tr("分辨率", "Resolution")}>
          <Select
            value={vp.resolution}
            onChange={(r) =>
              patch((p) => {
                p.video.resolution = r as typeof p.video.resolution;
              })
            }
            options={[
              { value: "source", label: v ? tr(`原始 ${v.width}×${v.height}`, `Source ${v.width}×${v.height}`) : tr("原始", "Source") },
              ...RESOLUTIONS.map((r) => ({
                value: r,
                label: `${r === "2160" ? "4K" : `${r}p`}${Number(r) >= shortEdge ? tr("（不放大）", " (no upscaling)") : ""}`,
                disabled: Number(r) >= shortEdge,
              })),
            ]}
          />
        </Field>

        <Field label={tr("编码器", "Encoder")}>
          <Select
            mono
            value={vp.encoderAuto ? "auto" : vp.encoder}
            options={encoderOptions}
            onChange={(val) =>
              patch((p) => {
                if (val === "auto") {
                  p.video.encoderAuto = true;
                } else {
                  p.video.encoderAuto = false;
                  p.video.encoder = val as typeof p.video.encoder;
                  // preset 不属于新编码器时由引擎换成它的默认值
                  p.video.qualityValue = qualityValue(p.video.encoder, p.video.quality);
                }
              })
            }
          />
        </Field>

        <Field label={tr("容器", "Container")}>
          <Segmented<Container>
            className="w-full"
            value={plan.container}
            options={[
              {
                value: "mkv",
                label: "MKV",
                title: tr(
                  "容纳能力最强：杜比视界、无损音轨、图形字幕、章节",
                  "Holds the most: Dolby Vision, lossless audio, image subtitles, chapters",
                ),
              },
              { value: "mp4", label: "MP4", title: tr("兼容性最好", "Most compatible") },
              { value: "mov", label: "MOV", title: tr("剪辑软件友好", "Editor friendly") },
            ]}
            onChange={(c) =>
              patch((p) => {
                p.container = c;
              })
            }
          />
        </Field>

        {isHdr && (
          <Field label="HDR">
            <Segmented<"keep" | "tonemap">
              className="w-full"
              value={vp.hdrAction === "tonemap" ? "tonemap" : "keep"}
              options={[
                { value: "keep", label: tr("保留 HDR", "Keep HDR") },
                {
                  value: "tonemap",
                  label: tr("转为 SDR", "Convert to SDR"),
                  disabled: !canTonemap(caps),
                  title: canTonemap(caps)
                    ? tr("做色调映射，适合手机与普通屏幕", "Tone maps for phones and regular screens")
                    : tr("当前 ffmpeg 没有可用的色调映射滤镜", "This ffmpeg has no usable tone mapping filter"),
                },
              ]}
              onChange={(a) =>
                patch((p) => {
                  p.video.hdrAction = a;
                  if (a === "tonemap") {
                    p.video.dovi = "disable";
                    p.video.bitDepth = 8;
                  } else {
                    // 切回保留 HDR 时恢复 10bit；编码器不支持时 normalizePlan 会再降下来并在保真度提示
                    p.video.bitDepth = 10;
                  }
                })
              }
            />
          </Field>
        )}
      </div>

      <div className={cn("mt-3.5", copy && "pointer-events-none opacity-40")}>
        <Field label={tr("帧率", "Frame rate")}>
          <FpsControl media={media} plan={plan} insight={result.fpsInsight} />
        </Field>
      </div>

      <div className="mt-3.5">
        <Field label={tr("音频", "Audio")}>
          <div className="flex flex-wrap items-center gap-1.5">
            <Select
              className="w-52"
              value={plan.audioMode}
              onChange={(m) =>
                patch((p) => {
                  p.audioMode = m as TranscodePlan["audioMode"];
                })
              }
              options={[
                { value: "copy_all", label: tr("全部原样复制", "Copy all") },
                { value: "original_plus_compat", label: tr("原样复制 + 兼容立体声", "Copy + compatible stereo") },
                { value: "compat_only", label: tr("转为兼容格式", "Convert to compatible") },
              ]}
            />
            <AudioChips media={media} plan={plan} />
          </div>
        </Field>
      </div>
    </Section>
  );
}
