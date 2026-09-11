import { useMemo } from "react";
import { Info } from "lucide-react";
import type { Codec, Container, FpsInsight, MediaInfo, PlanResult, QualityTier, TranscodePlan } from "@/lib/types";
import { cn } from "@/lib/cn";
import { CODEC_LABEL, canTonemap, codecAvailable, qualityValue, VENDOR_LABEL } from "@/lib/encoders";
import { encoderMeta, engineMeta, videoHints } from "@/lib/engine";
import { channelLabel, formatBitrate, formatFps } from "@/lib/format";
import { useCapabilities } from "@/stores/capability";
import { useProject } from "@/stores/project";
import { Badge, Field, Section, Segmented, Select, Switch } from "./ui";

const QUALITY_OPTIONS: { value: QualityTier; label: string }[] = [
  { value: "lossless", label: "视觉无损" },
  { value: "high", label: "高" },
  { value: "standard", label: "标准" },
  { value: "small", label: "小体积" },
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
  if (v.isVfr && !cfr) hint = { text: "导入剪辑软件前建议开启，否则音画会随时间逐渐错位", tone: "vfr" };
  else if (cfr && hints.extremeVfr) hint = { text: "源帧率波动大，会复制大量帧，编码耗时明显增加", tone: "warn" };

  return (
    <div>
      <div className="flex min-h-9 flex-wrap items-center gap-x-3 gap-y-1.5 rounded-md border border-line bg-raised/40 px-3 py-1.5">
        <Switch
          checked={cfr}
          label="转为固定帧率"
          onChange={(on) =>
            patch((p) => {
              p.video.fps = on ? { kind: "cfr", fps: recommended } : { kind: "keep" };
            })
          }
        />
        <span className="text-[13px]">转为固定帧率</span>
        {v.isVfr ? (
          <Badge tone="vfr" title={`平均 ${formatFps(v.fpsAvg)} fps，名义 ${formatFps(v.fpsNominal)} fps`}>
            源为可变帧率
          </Badge>
        ) : (
          <span className="text-xs text-subtle">源为固定 {formatFps(v.fpsAvg)} fps</span>
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
                label: `${s.label} fps${Math.abs(s.value - recommended) < 1e-6 ? " 推荐" : ""}`,
              }))}
            />
            {insight && (
              <span className="ml-auto text-xs text-muted tabular" title="转换后用 ffprobe 校验音视频时长差小于 1 帧">
                {insight.sourceFrames.toLocaleString()} → {insight.targetFrames.toLocaleString()} 帧
                {insight.duplicated > 0 && `，复制 ${insight.duplicated.toLocaleString()}`}
                {insight.dropped > 0 && `，丢弃 ${insight.dropped.toLocaleString()}`}
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
  if (plan.audio.length === 0) return <span className="text-xs text-subtle">源文件没有音轨</span>;
  return (
    <>
      {plan.audio.map((t, i) => {
        const src = media.audio.find((a) => a.index === t.sourceIndex);
        const name = t.action === "copy" ? (src?.title ?? src?.codec.toUpperCase() ?? "音轨") : t.title ?? src?.title;
        const detail =
          t.action === "copy"
            ? "复制"
            : `${t.codec?.toUpperCase()} ${t.channels ? channelLabel(t.channels) : ""} ${t.bitrateKbps}k`;
        return (
          <span
            key={i}
            title={t.role === "compat" ? "新增的兼容音轨" : undefined}
            className={cn(
              "inline-flex h-7 max-w-[240px] items-center gap-1.5 rounded-md border px-2 text-[11.5px]",
              t.role === "compat" ? "border-dashed border-line-strong" : "border-line bg-raised/50",
            )}
          >
            <span className="truncate">{t.action === "copy" ? name : detail}</span>
            <span className={cn("shrink-0", src?.atmos && t.action === "copy" ? "text-atmos" : "text-subtle")}>
              {t.action === "copy" ? "· 复制" : "· 新增"}
            </span>
          </span>
        );
      })}
    </>
  );
}

export function ParamsPanel({ media, plan, result }: { media: MediaInfo; plan: TranscodePlan; result: PlanResult }) {
  const patch = useProject((s) => s.patchPlan);
  const caps = useCapabilities((s) => s.caps);
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
    { value: "auto", label: `自动（${vp.encoder}）` },
    ...caps.encoders
      .filter((e) => e.codec === vp.codec)
      .map((e) => ({
        value: e.id,
        label: `${e.id} · ${VENDOR_LABEL[e.vendor]}${e.usable ? "" : "（不可用）"}`,
        disabled: !e.usable,
      })),
  ];

  return (
    <Section step={2} title="关键参数">
      {copy && (
        <p className="mb-3 flex items-center gap-1.5 text-xs text-muted">
          <Info className="size-3.5" />
          原样封装不重新编码视频，画面参数不可调整。
        </p>
      )}

      <div className={cn("grid gap-x-5 gap-y-3.5 md:grid-cols-2", copy && "pointer-events-none opacity-40")}>
        <Field
          label="画质"
          hint={
            byBitrate
              ? `${rc.kind === "two_pass" ? "两遍 · " : ""}平均 ${formatBitrate(rc.kbps * 1000)}`
              : `${meta.param} ${vp.qualityValue}${rc.kind === "capped" ? ` · 峰值 ${formatBitrate(rc.kbps * 1000)}` : ""}`
          }
        >
          <Segmented
            className={cn("w-full", byBitrate && "pointer-events-none opacity-40")}
            value={vp.quality}
            options={QUALITY_OPTIONS}
            onChange={(q) =>
              patch((p) => {
                p.video.quality = q;
                p.video.qualityValue = qualityValue(p.video.encoder, q);
              })
            }
          />
        </Field>

        <Field label="编码格式">
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
                title: off ? `当前 ffmpeg 没有可用的 ${CODEC_LABEL[c]} 编码器` : undefined,
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

        <Field label="分辨率">
          <Select
            value={vp.resolution}
            onChange={(r) =>
              patch((p) => {
                p.video.resolution = r as typeof p.video.resolution;
              })
            }
            options={[
              { value: "source", label: v ? `原始 ${v.width}×${v.height}` : "原始" },
              ...RESOLUTIONS.map((r) => ({
                value: r,
                label: `${r === "2160" ? "4K" : `${r}p`}${Number(r) >= shortEdge ? "（不放大）" : ""}`,
                disabled: Number(r) >= shortEdge,
              })),
            ]}
          />
        </Field>

        <Field label="编码器">
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

        <Field label="容器">
          <Segmented<Container>
            className="w-full"
            value={plan.container}
            options={[
              { value: "mkv", label: "MKV", title: "容纳能力最强：杜比视界、无损音轨、图形字幕、章节" },
              { value: "mp4", label: "MP4", title: "兼容性最好" },
              { value: "mov", label: "MOV", title: "剪辑软件友好" },
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
                { value: "keep", label: "保留 HDR" },
                {
                  value: "tonemap",
                  label: "转为 SDR",
                  disabled: !canTonemap(caps),
                  title: canTonemap(caps) ? "做色调映射，适合手机与普通屏幕" : "当前 ffmpeg 没有可用的色调映射滤镜",
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
        <Field label="帧率">
          <FpsControl media={media} plan={plan} insight={result.fpsInsight} />
        </Field>
      </div>

      <div className="mt-3.5">
        <Field label="音频">
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
                { value: "copy_all", label: "全部原样复制" },
                { value: "original_plus_compat", label: "原样复制 + 兼容立体声" },
                { value: "compat_only", label: "转为兼容格式" },
              ]}
            />
            <AudioChips media={media} plan={plan} />
          </div>
        </Field>
      </div>
    </Section>
  );
}
