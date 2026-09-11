import { useState } from "react";
import { ChevronRight, SlidersHorizontal } from "lucide-react";
import type { TranscodePlan } from "@/lib/types";
import { cn } from "@/lib/cn";
import { useCapabilities } from "@/stores/capability";
import { useProject } from "@/stores/project";
import { encoderSupports10bit, presetOptions, qualityMeta } from "@/mock/engine/encoders";
import { Field, Segmented, Select } from "./ui";

const inputCls =
  "h-8 w-full rounded-md border border-line bg-panel px-2.5 font-mono text-xs text-fg transition-colors hover:border-line-strong focus:border-accent focus:outline-none";

/** 由场景自动决定、一般无需改动的参数，默认折叠 */
export function ExpertPanel({ plan }: { plan: TranscodePlan }) {
  const [open, setOpen] = useState(false);
  const patch = useProject((s) => s.patchPlan);
  const caps = useCapabilities((s) => s.caps);
  const vp = plan.video;
  const meta = qualityMeta(vp.encoder);
  const copy = vp.action === "copy";
  const can10 = encoderSupports10bit(vp.encoder, caps);

  const summary = [
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
              options={presetOptions(vp.encoder).map((p) => ({ value: p, label: p }))}
              onChange={(p) =>
                patch((d) => {
                  d.video.preset = p;
                })
              }
            />
          </Field>

          <Field
            label={meta.param}
            hint={meta.lowerIsBetter ? "越小画质越好" : "越大画质越好"}
            className={cn(copy && "pointer-events-none opacity-40")}
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
