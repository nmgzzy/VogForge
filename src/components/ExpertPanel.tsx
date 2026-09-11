import { useEffect, useState } from "react";
import { ChevronRight, SlidersHorizontal } from "lucide-react";
import type { Capabilities, PlanResult, RateControl, RateControlKind, TranscodePlan } from "@/lib/types";
import { cn } from "@/lib/cn";
import { encoderSupports10bit } from "@/lib/encoders";
import { encoderMeta, engineMeta } from "@/lib/engine";
import { formatBitrate } from "@/lib/format";
import { useCapabilities } from "@/stores/capability";
import { useProject } from "@/stores/project";
import { Field, Segmented, Select } from "./ui";

const boxCls =
  "h-8 rounded-md border border-line bg-panel px-2.5 font-mono text-xs text-fg transition-colors hover:border-line-strong focus:border-accent focus:outline-none";
const inputCls = cn(boxCls, "w-full");

const RC_LABEL: Record<RateControlKind, string> = {
  quality: "恒定质量",
  bitrate: "目标码率",
  capped: "限峰值",
  two_pass: "两遍",
};

const RC_HINT: Record<RateControlKind, string> = {
  quality: "按画质编码，体积随画面复杂度变化",
  bitrate: "按平均码率编码，体积可预测，峰值不超过 1.5 倍",
  capped: "按画质编码，同时限制峰值码率，适合网络串流",
  two_pass: "先分析全片再分配码率，体积准确、画质均匀，耗时约 1.7 倍",
};

/** 简短描述当前码率控制，用于摘要与提示 */
export function rateControlSummary(rc: RateControl): string | undefined {
  const mbps = (kbps: number) => formatBitrate(kbps * 1000);
  switch (rc.kind) {
    case "quality":
      return undefined;
    case "bitrate":
      return `平均 ${mbps(rc.kbps)}`;
    case "capped":
      return `峰值 ${mbps(rc.kbps)}`;
    case "two_pass":
      return `两遍 · 平均 ${mbps(rc.kbps)}`;
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
    return "两遍编码只有软件编码器支持；把编码器改回自动或选择软件编码器";
  }
  return `${vp.encoder} 没有"按质量编码 + 限峰值"的模式`;
}

/** 以 Mbps 输入码率。输入过程中允许暂时不合法（例如清空），失焦或回车时提交 */
function MbpsInput({ kbps, onCommit, label }: { kbps: number; onCommit: (kbps: number) => void; label: string }) {
  const [draft, setDraft] = useState(String(kbps / 1000));
  useEffect(() => setDraft(String(kbps / 1000)), [kbps]);
  const commit = () => {
    const v = Number(draft);
    if (Number.isFinite(v) && v > 0) onCommit(Math.round(v * 1000));
    else setDraft(String(kbps / 1000));
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
    </div>
  );
}

/** 由场景自动决定、一般无需改动的参数，默认折叠 */
export function ExpertPanel({ plan, result }: { plan: TranscodePlan; result: PlanResult }) {
  const [open, setOpen] = useState(false);
  const patch = useProject((s) => s.patchPlan);
  const caps = useCapabilities((s) => s.caps);
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

  const summary = [
    rateControlSummary(rc),
    `${vp.bitDepth}bit`,
    { all: "全部字幕", text_only: "仅文本字幕", none: "不保留字幕" }[plan.subtitles],
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
        <h2 className="text-[13px] font-semibold">更多参数</h2>
        <span className="ml-auto truncate text-[11px] text-subtle">{summary}</span>
      </button>
      {open && (
        <div className="grid gap-x-5 gap-y-3.5 border-t border-line p-4 md:grid-cols-3">
          <Field label="色深" hint={can10 ? undefined : `${vp.encoder} 仅 8bit`} className={cn(copy && "pointer-events-none opacity-40")}>
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

          <Field label="字幕">
            <Segmented<TranscodePlan["subtitles"]>
              className="w-full"
              value={plan.subtitles}
              options={[
                { value: "all", label: "全部" },
                { value: "text_only", label: "仅文本" },
                { value: "none", label: "不保留" },
              ]}
              onChange={(s) =>
                patch((p) => {
                  p.subtitles = s;
                })
              }
            />
          </Field>

          <Field label="编码速度" hint="越慢画质越好" className={cn(copy && "pointer-events-none opacity-40")}>
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
            hint={byBitrate ? "按码率编码时不使用" : meta.lowerIsBetter ? "越小画质越好" : "越大画质越好"}
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

          <Field label="码率控制" hint={RC_HINT[rc.kind]} className={cn("md:col-span-2", copy && "pointer-events-none opacity-40")}>
            <div className="flex flex-wrap items-center gap-2">
              <Segmented<RateControlKind>
                className="shrink-0"
                value={rc.kind}
                options={(["quality", "bitrate", "capped", "two_pass"] as const).map((k) => {
                  const why = rcUnavailable(k, plan, caps);
                  return { value: k, label: RC_LABEL[k], disabled: !!why, title: why ?? RC_HINT[k] };
                })}
                onChange={setKind}
              />
              {rc.kind !== "quality" && (
                <MbpsInput
                  kbps={rc.kbps}
                  label={rc.kind === "capped" ? "峰值码率" : "目标码率"}
                  onCommit={(kbps) =>
                    patch((d) => {
                      if (d.video.rateControl.kind !== "quality") d.video.rateControl = { ...d.video.rateControl, kbps };
                    })
                  }
                />
              )}
            </div>
          </Field>

          <Field label="关键帧间隔" hint="留空为默认" className={cn(copy && "pointer-events-none opacity-40")}>
            <input
              className={inputCls}
              inputMode="numeric"
              placeholder="默认"
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
            <Field label="色调映射管线">
              <Select
                value={vp.tonemap ?? "libplacebo"}
                options={caps.tonemap.map((t) => ({
                  value: t.id,
                  label: `${t.id}${t.available ? "" : "（不可用）"}`,
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
            <Field label="附加 x265-params" hint="冒号分隔" className="md:col-span-3">
              <input
                className={inputCls}
                placeholder="例如 aq-mode=3:psy-rd=2"
                value={vp.extraParams ?? ""}
                onChange={(e) =>
                  patch((d) => {
                    d.video.extraParams = e.target.value || undefined;
                  })
                }
              />
            </Field>
          )}

          <Field label="附加 ffmpeg 参数" hint="插在视频编码参数之后" className="md:col-span-3">
            <input
              className={inputCls}
              placeholder="例如 -tune grain"
              value={vp.extraArgs ?? ""}
              onChange={(e) =>
                patch((d) => {
                  d.video.extraArgs = e.target.value || undefined;
                })
              }
            />
          </Field>

          <p className="text-[11px] text-subtle md:col-span-3">
            质量数值在不同编码器间不等价：x265 CRF 20、NVENC CQ 23、QSV 21 大致都是"高"档。
          </p>
        </div>
      )}
    </section>
  );
}
