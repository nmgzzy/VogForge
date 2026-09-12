import { useEffect, useMemo, useState } from "react";
import { ChevronRight, SlidersHorizontal } from "lucide-react";
import { tr } from "@/i18n";
import type { Capabilities, MediaInfo, PlanResult, RateControl, RateControlKind, TranscodePlan } from "@/lib/types";
import { cn } from "@/lib/cn";
import { encoderSupports10bit } from "@/lib/encoders";
import { encoderMeta, engineMeta, videoHints } from "@/lib/engine";
import { formatBitrate } from "@/lib/format";
import { useEngineCaps } from "@/stores/engine-caps";
import { useProject } from "@/stores/project";
import { Field, Segmented, Select } from "./ui";

const boxCls =
  "h-8 rounded-md border border-line bg-panel px-2.5 font-mono text-xs text-fg transition-colors hover:border-line-strong focus:border-accent focus:outline-none";
const inputCls = cn(boxCls, "w-full");

function rcLabel(kind: RateControlKind): string {
  switch (kind) {
    case "quality":
      return tr("恒定质量", "Quality");
    case "bitrate":
      return tr("目标码率", "Bitrate");
    case "capped":
      return tr("限峰值", "Capped");
    case "two_pass":
      return tr("两遍", "Two-pass");
  }
}

function rcHint(kind: RateControlKind): string {
  switch (kind) {
    case "quality":
      return tr("按画质编码，体积随画面复杂度变化", "Encodes by quality; the size follows scene complexity");
    case "bitrate":
      return tr("按平均码率编码，体积可预测，峰值不超过 1.5 倍", "Encodes to an average bitrate with a predictable size; peaks stay under 1.5×");
    case "capped":
      return tr("按画质编码，同时限制峰值码率，适合网络串流", "Encodes by quality while capping the peak bitrate, good for streaming");
    case "two_pass":
      return tr(
        "先分析全片再分配码率，体积准确、画质均匀，耗时约 1.7 倍",
        "Analyzes the whole video before distributing the bitrate: accurate size, even quality, about 1.7× the time",
      );
  }
}

/** 简短描述当前码率控制，用于摘要与提示 */
export function rateControlSummary(rc: RateControl): string | undefined {
  const mbps = (kbps: number) => formatBitrate(kbps * 1000);
  switch (rc.kind) {
    case "quality":
      return undefined;
    case "bitrate":
      return tr(`平均 ${mbps(rc.kbps)}`, `avg ${mbps(rc.kbps)}`);
    case "capped":
      return tr(`峰值 ${mbps(rc.kbps)}`, `peak ${mbps(rc.kbps)}`);
    case "two_pass":
      return tr(`两遍 · 平均 ${mbps(rc.kbps)}`, `2-pass · avg ${mbps(rc.kbps)}`);
  }
}

/** 切换码率控制方式时的起始数值：沿用已有的码率，否则取当前画质下的预计码率 */
function startingKbps(kind: RateControlKind, current: RateControl, videoBps: number): number {
  const { minKbps, maxKbps } = engineMeta();
  const round = (k: number) => Math.min(maxKbps, Math.max(minKbps, Math.round(k / 100) * 100));
  const avg =
    current.kind === "bitrate" || current.kind === "two_pass"
      ? current.kbps
      : current.kind === "capped"
        ? (current.kbps * 2) / 3
        : videoBps / 1000;
  return round(kind === "capped" ? avg * 1.5 : avg);
}

/** 编码器做不到的码率控制：返回原因，可用时返回 undefined */
function rcUnavailable(kind: RateControlKind, plan: TranscodePlan, caps: Capabilities): string | undefined {
  const vp = plan.video;
  const meta = encoderMeta(vp.encoder);
  if (meta.rateControls.includes(kind)) return undefined;
  if (kind === "two_pass") {
    // 自动选择编码器时，引擎会为两遍换成软件编码器
    const sw = caps.encoders.some((e) => e.codec === vp.codec && e.vendor === "software" && e.usable);
    if (vp.encoderAuto && sw) return undefined;
    return tr(
      "两遍编码只有软件编码器支持；把编码器改回自动或选择软件编码器",
      "Only software encoders support two-pass; set the encoder back to automatic or pick a software encoder",
    );
  }
  return tr(`${vp.encoder} 没有"按质量编码 + 限峰值"的模式`, `${vp.encoder} has no quality mode with a peak cap`);
}

/** 以 Mbps 输入码率。输入过程中允许暂时不合法（例如清空），失焦或回车时提交 */
function MbpsInput({
  kbps,
  onCommit,
  label,
  sourceKbps,
}: {
  kbps: number;
  onCommit: (kbps: number) => void;
  label: string;
  sourceKbps?: number;
}) {
  const [draft, setDraft] = useState(String(kbps / 1000));
  // 引擎会把提交的数值拉回范围内（例如不高于源）；拉回后数值没变时，输入框也要回到实际值
  const [commits, setCommits] = useState(0);
  useEffect(() => setDraft(String(kbps / 1000)), [kbps, commits]);
  const commit = () => {
    const v = Number(draft);
    if (Number.isFinite(v) && v > 0) onCommit(Math.round(v * 1000));
    setCommits((n) => n + 1);
  };
  return (
    <div className="flex h-8 items-center gap-1.5">
      <input
        className={cn(boxCls, "w-20 text-right")}
        inputMode="decimal"
        aria-label={label}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => e.key === "Enter" && commit()}
      />
      <span className="text-xs text-subtle">Mbps</span>
      {sourceKbps !== undefined && (
        <span
          className="text-xs text-subtle"
          title={tr(
            "目标码率不会高于源视频码率，限峰值时峰值不高于它的 1.5 倍",
            "The target never exceeds the source video bitrate; a peak cap stays within 1.5× of it",
          )}
        >
          {tr(`· 源 ${formatBitrate(sourceKbps * 1000)}`, `· source ${formatBitrate(sourceKbps * 1000)}`)}
        </span>
      )}
    </div>
  );
}

/** 由场景自动决定、一般无需改动的参数，默认折叠 */
export function ExpertPanel({ media, plan, result }: { media: MediaInfo; plan: TranscodePlan; result: PlanResult }) {
  const [open, setOpen] = useState(false);
  const patch = useProject((s) => s.patchPlan);
  const caps = useEngineCaps();
  const sourceKbps = useMemo(() => videoHints(media)?.sourceKbps, [media]);
  // 附加参数有问题时引擎整段不用，推荐理由里有警告；输入框下面也提示一次
  const extraWarn = result.decisions.find((d) => d.field === tr("附加参数", "Extra arguments") && d.severity === "warn");
  const vp = plan.video;
  const meta = encoderMeta(vp.encoder);
  const copy = vp.action === "copy";
  const can10 = encoderSupports10bit(vp.encoder, caps);
  const rc = vp.rateControl;
  const byBitrate = rc.kind === "bitrate" || rc.kind === "two_pass";

  const setKind = (kind: RateControlKind) =>
    patch((d) => {
      d.video.rateControl =
        kind === "quality" ? { kind } : { kind, kbps: startingKbps(kind, d.video.rateControl, result.estimate.videoBps) };
    });

  const encodedAudio = plan.audio.some((t) => t.action === "encode");
  const summary = [
    rateControlSummary(rc),
    plan.loudnorm ? tr("响度 -16 LUFS", "loudness -16 LUFS") : undefined,
    `${vp.bitDepth}bit`,
    {
      all: tr("全部字幕", "all subtitles"),
      text_only: tr("仅文本字幕", "text subtitles only"),
      none: tr("不保留字幕", "no subtitles"),
    }[plan.subtitles],
    `preset ${vp.preset}`,
    vp.gop ? `GOP ${vp.gop}` : undefined,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <section className="rounded-lg border border-line bg-panel">
      <button onClick={() => setOpen(!open)} aria-expanded={open} className="flex h-10 w-full items-center gap-2 px-4 text-left">
        <ChevronRight className={cn("size-4 text-subtle transition-transform", open && "rotate-90")} />
        <SlidersHorizontal className="size-3.5 text-subtle" />
        <h2 className="text-[13px] font-semibold">{tr("更多参数", "More options")}</h2>
        <span className="ml-auto truncate text-[11px] text-subtle">{summary}</span>
      </button>
      {open && (
        <div className="grid gap-x-5 gap-y-3.5 border-t border-line p-4 md:grid-cols-3">
          <Field
            label={tr("色深", "Bit depth")}
            hint={can10 ? undefined : tr(`${vp.encoder} 仅 8bit`, `${vp.encoder} is 8-bit only`)}
            className={cn(copy && "pointer-events-none opacity-40")}
          >
            <Segmented<"8" | "10">
              className="w-full"
              value={String(vp.bitDepth) as "8" | "10"}
              options={[
                { value: "8", label: "8bit" },
                { value: "10", label: "10bit", disabled: !can10 },
              ]}
              onChange={(b) =>
                patch((p) => {
                  p.video.bitDepth = Number(b) as 8 | 10;
                })
              }
            />
          </Field>

          <Field label={tr("字幕", "Subtitles")}>
            <Segmented<TranscodePlan["subtitles"]>
              className="w-full"
              value={plan.subtitles}
              options={[
                { value: "all", label: tr("全部", "All") },
                { value: "text_only", label: tr("仅文本", "Text only") },
                { value: "none", label: tr("不保留", "None") },
              ]}
              onChange={(s) =>
                patch((p) => {
                  p.subtitles = s;
                })
              }
            />
          </Field>

          <Field
            label={tr("响度", "Loudness")}
            hint={tr("两遍测量，只作用于重新编码的音轨", "Two-pass measurement; only affects re-encoded tracks")}
          >
            <Segmented<"off" | "on">
              className="w-full"
              value={plan.loudnorm ? "on" : "off"}
              options={[
                { value: "off", label: tr("不调整", "Off") },
                {
                  value: "on",
                  label: tr("标准化 -16 LUFS", "Normalize -16 LUFS"),
                  disabled: !encodedAudio && !plan.loudnorm,
                  title: encodedAudio
                    ? tr(
                        "先测量整段响度再线性调整，适合音量忽大忽小的素材",
                        "Measures the whole program, then adjusts linearly; good for footage with uneven volume",
                      )
                    : tr("音轨都是原样复制，无法调整响度", "All audio tracks are copied, so loudness cannot change"),
                },
              ]}
              onChange={(v) =>
                patch((d) => {
                  d.loudnorm = v === "on";
                })
              }
            />
          </Field>

          <Field
            label={tr("编码速度", "Speed preset")}
            hint={tr("越慢画质越好", "Slower gives better quality")}
            className={cn(copy && "pointer-events-none opacity-40")}
          >
            <Select
              mono
              value={vp.preset}
              options={meta.presets.map((p) => ({ value: p, label: p }))}
              onChange={(p) =>
                patch((d) => {
                  d.video.preset = p;
                })
              }
            />
          </Field>

          <Field
            label={meta.param}
            hint={
              byBitrate
                ? tr("按码率编码时不使用", "Unused when encoding by bitrate")
                : meta.lowerIsBetter
                  ? tr("越小画质越好", "Lower is better")
                  : tr("越大画质越好", "Higher is better")
            }
            className={cn((copy || byBitrate) && "pointer-events-none opacity-40")}
          >
            <div className="flex h-8 items-center gap-2">
              <input
                type="range"
                min={meta.min}
                max={meta.max}
                value={vp.qualityValue}
                aria-label={meta.param}
                onChange={(e) =>
                  patch((d) => {
                    d.video.qualityValue = Number(e.target.value);
                  })
                }
                className="flex-1 accent-[var(--vf-accent)]"
              />
              <span className="w-7 text-right font-mono text-xs tabular">{vp.qualityValue}</span>
            </div>
          </Field>

          <Field
            label={tr("码率控制", "Rate control")}
            hint={rcHint(rc.kind)}
            className={cn("md:col-span-2", copy && "pointer-events-none opacity-40")}
          >
            <div className="flex flex-wrap items-center gap-2">
              <Segmented<RateControlKind>
                className="shrink-0"
                value={rc.kind}
                options={(["quality", "bitrate", "capped", "two_pass"] as const).map((k) => {
                  const why = rcUnavailable(k, plan, caps);
                  return { value: k, label: rcLabel(k), disabled: !!why, title: why ?? rcHint(k) };
                })}
                onChange={setKind}
              />
              {rc.kind !== "quality" && (
                <MbpsInput
                  kbps={rc.kbps}
                  sourceKbps={sourceKbps}
                  label={rc.kind === "capped" ? tr("峰值码率", "Peak bitrate") : tr("目标码率", "Target bitrate")}
                  onCommit={(kbps) =>
                    patch((d) => {
                      if (d.video.rateControl.kind !== "quality") d.video.rateControl = { ...d.video.rateControl, kbps };
                    })
                  }
                />
              )}
            </div>
          </Field>

          <Field
            label={tr("关键帧间隔", "Keyframe interval")}
            hint={tr("留空为默认", "Empty means default")}
            className={cn(copy && "pointer-events-none opacity-40")}
          >
            <input
              className={inputCls}
              inputMode="numeric"
              placeholder={tr("默认", "Default")}
              value={vp.gop ?? ""}
              onChange={(e) =>
                patch((d) => {
                  const n = Number(e.target.value);
                  d.video.gop = e.target.value && Number.isFinite(n) && n > 0 ? Math.round(n) : undefined;
                })
              }
            />
          </Field>

          {vp.hdrAction === "tonemap" && (
            <Field label={tr("色调映射管线", "Tone mapping pipeline")}>
              <Select
                value={vp.tonemap ?? "libplacebo"}
                options={caps.tonemap.map((t) => ({
                  value: t.id,
                  label: `${t.id}${t.available ? "" : tr("（不可用）", " (unavailable)")}`,
                  disabled: !t.available,
                }))}
                onChange={(t) =>
                  patch((d) => {
                    d.video.tonemap = t as typeof d.video.tonemap;
                  })
                }
              />
            </Field>
          )}

          {vp.encoder === "libx265" && (
            <Field label={tr("附加 x265-params", "Extra x265-params")} hint={tr("冒号分隔", "Colon separated")} className="md:col-span-3">
              <input
                className={inputCls}
                placeholder={tr("例如 aq-mode=3:psy-rd=2", "e.g. aq-mode=3:psy-rd=2")}
                value={vp.extraParams ?? ""}
                onChange={(e) =>
                  patch((d) => {
                    d.video.extraParams = e.target.value || undefined;
                  })
                }
              />
            </Field>
          )}

          <Field
            label={tr("附加 ffmpeg 参数", "Extra ffmpeg arguments")}
            hint={tr("插在视频编码参数之后", "Inserted after the video encoder options")}
            className="md:col-span-3"
          >
            <input
              className={inputCls}
              placeholder={tr("例如 -tune grain", "e.g. -tune grain")}
              value={vp.extraArgs ?? ""}
              onChange={(e) =>
                patch((d) => {
                  d.video.extraArgs = e.target.value || undefined;
                })
              }
            />
            {extraWarn && <p className="mt-1 text-[11.5px] text-warn">{extraWarn.reason}</p>}
          </Field>

          <p className="text-[11px] text-subtle md:col-span-3">
            {tr(
              '质量数值在不同编码器间不等价：x265 CRF 20、NVENC CQ 23、QSV 21 大致都是"高"档。',
              'Quality values are not comparable across encoders: x265 CRF 20, NVENC CQ 23 and QSV 21 are all roughly "High".',
            )}
          </p>
        </div>
      )}
    </section>
  );
}
