import { useState } from "react";
import { ChevronDown, Clock, Cpu, HardDrive, Lightbulb, TriangleAlert, Zap } from "lucide-react";
import { isEnglish, tr } from "@/i18n";
import type { Decision, MediaInfo, PlanResult } from "@/lib/types";
import { cn } from "@/lib/cn";
import { formatBytes, formatPercent, formatTimeRange } from "@/lib/format";
import { encoderVendor, isHardware, vendorLabel } from "@/lib/encoders";

export function EstimateCard({ media, result }: { media: MediaInfo; result: PlanResult }) {
  const { estimate: e, plan } = result;
  const copy = plan.video.action === "copy";
  const mid = (e.sizeMin + e.sizeMax) / 2;
  const grows = e.ratio > 1;
  const hw = !copy && isHardware(plan.video.encoder);

  return (
    <div className="rounded-lg border border-line bg-panel p-4">
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-xs text-muted">{tr("预计输出", "Estimated output")}</span>
        <span className={cn("text-xs font-medium tabular", grows ? "text-warn" : "text-ok")}>
          {copy ? tr("与源相同", "Same as source") : tr(`源的 ${formatPercent(e.ratio)}`, `${formatPercent(e.ratio)} of source`)}
        </span>
      </div>
      <div
        className="mt-0.5 text-[21px] leading-tight font-semibold tracking-tight tabular"
        title={tr(
          "按经验码率估算的区间，实际体积取决于画面复杂度",
          "A range estimated from typical bitrates; the real size depends on scene complexity",
        )}
      >
        {copy ? formatBytes(media.sizeBytes) : `${formatBytes(e.sizeMin, 0)} – ${formatBytes(e.sizeMax, 0)}`}
      </div>

      {/* 一条对比条：底色是源文件，前景是输出；输出更大时反过来 */}
      <div className="relative mt-2.5 h-1.5 overflow-hidden rounded-full bg-line-strong/50" title={tr(
          `源 ${formatBytes(media.sizeBytes)} → 输出约 ${formatBytes(mid)}`,
          `Source ${formatBytes(media.sizeBytes)} → output about ${formatBytes(mid)}`,
        )}
      >
        <div
          className={cn("absolute inset-y-0 left-0 rounded-full transition-[width] duration-300", grows ? "bg-warn" : "bg-accent")}
          style={{ width: `${Math.max(2, Math.min(100, (grows ? 1 / e.ratio : e.ratio) * 100))}%` }}
        />
      </div>

      <div className="mt-3 flex items-center justify-between gap-2 text-xs text-muted">
        <span className="flex items-center gap-1 tabular">
          <Clock className="size-3" />
          {formatTimeRange(e.timeMinSec, e.timeMaxSec)}
        </span>
        <span className="flex min-w-0 items-center gap-1" title={plan.video.encoder}>
          {hw ? <Zap className="size-3 text-accent" /> : copy ? <HardDrive className="size-3" /> : <Cpu className="size-3" />}
          <span className="truncate">
            {copy ? tr("流复制", "Stream copy") : vendorLabel(hw ? encoderVendor(plan.video.encoder) : "software")}
          </span>
        </span>
      </div>
    </div>
  );
}

/** 默认只展开警告与提示的理由；普通条目点一下再看，避免整栏都是解释文字 */
export function DecisionList({ result }: { result: PlanResult }) {
  const [all, setAll] = useState(false);
  const [open, setOpen] = useState<Set<number>>(new Set());
  const hidden = result.decisions.filter((d) => d.severity === "info").length;

  const toggle = (i: number) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });

  return (
    <div className="rounded-lg border border-line bg-panel">
      <header className="flex h-10 items-center gap-2 border-b border-line px-4">
        <Lightbulb className="size-3.5 text-accent" />
        <h2 className="text-[13px] font-semibold">{tr("推荐说明", "Why these settings")}</h2>
        {hidden > 0 && (
          <button onClick={() => setAll(!all)} className="ml-auto text-[11px] text-muted hover:text-fg">
            {all ? tr("收起说明", "Collapse") : tr("展开全部说明", "Expand all")}
          </button>
        )}
      </header>
      <ul className="py-1">
        {result.decisions.map((d, i) => (
          <DecisionRow
            key={`${d.field}-${i}`}
            d={d}
            expanded={all || d.severity !== "info" || open.has(i)}
            onToggle={d.severity === "info" && !all ? () => toggle(i) : undefined}
          />
        ))}
      </ul>
    </div>
  );
}

function DecisionRow({ d, expanded, onToggle }: { d: Decision; expanded: boolean; onToggle?: () => void }) {
  const warn = d.severity === "warn";
  // 英文字段名更长（Resolution、Frame rate），标签列相应加宽
  const en = isEnglish();
  return (
    <li className={cn("relative px-4 py-1.5", warn && "bg-warn/[0.05]")}>
      {warn && <span className="absolute inset-y-1.5 left-0 w-[3px] rounded-r bg-warn" />}
      <button
        onClick={onToggle}
        disabled={!onToggle}
        className="flex w-full items-baseline gap-2 text-left disabled:cursor-default"
      >
        <span className={cn("shrink-0 text-[11px] text-subtle", en ? "w-[68px]" : "w-12")}>{d.field}</span>
        <span className="min-w-0 flex-1 truncate text-xs font-medium" title={d.value}>
          {warn && <TriangleAlert className="mr-1 inline size-3 -translate-y-px text-warn" />}
          {d.value}
        </span>
        {onToggle && <ChevronDown className={cn("size-3 shrink-0 text-subtle transition-transform", expanded && "rotate-180")} />}
      </button>
      {expanded && (
        <p className={cn("mt-0.5 mb-0.5 text-[11.5px] leading-relaxed text-muted", en ? "pl-[76px]" : "pl-14")}>{d.reason}</p>
      )}
    </li>
  );
}
