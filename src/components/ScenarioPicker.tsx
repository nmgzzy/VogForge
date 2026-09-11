import {
  Archive,
  Feather,
  Gem,
  PackageOpen,
  Scissors,
  Share2,
  Smartphone,
  Tv,
  TriangleAlert,
  type LucideIcon,
} from "lucide-react";
import { tr } from "@/i18n";
import type { MediaInfo, Scenario } from "@/lib/types";
import { cn } from "@/lib/cn";
import { suggestScenario } from "@/lib/engine";
import { scenarios } from "@/lib/scenarios";
import { useProject } from "@/stores/project";
import { Button, Section } from "./ui";

const ICON: Record<Scenario, LucideIcon> = {
  archive: Archive,
  collection: Gem,
  streaming: Tv,
  mobile: Smartphone,
  social: Share2,
  editing: Scissors,
  smallest: Feather,
  remux: PackageOpen,
};

export function ScenarioPicker({ media, value }: { media: MediaInfo; value: Scenario }) {
  const setScenario = useProject((s) => s.setScenario);
  const applyAll = useProject((s) => s.applyScenarioToAll);
  const count = useProject((s) => s.files.length);
  const suggested = suggestScenario(media);

  return (
    <Section
      step={1}
      title={tr("选择用途", "Choose a purpose")}
      aside={
        count > 1 && (
          <Button
            size="xs"
            variant="ghost"
            onClick={applyAll}
            title={tr("其他文件会按各自情况重新推荐参数", "Settings are re-recommended for each file")}
          >
            {tr(`应用到全部 ${count} 个文件`, `Apply to all ${count} files`)}
          </Button>
        )
      }
    >
      <div className="@container">
        <div className="grid grid-cols-2 gap-1.5 @min-[500px]:grid-cols-4">
          {scenarios().map((s) => {
            const Icon = ICON[s.id];
            const on = s.id === value;
            return (
              <button
                key={s.id}
                onClick={() => setScenario(s.id)}
                title={tr(`${s.tagline}。${s.outcome}`, `${s.tagline}. ${s.outcome}`)}
                aria-pressed={on}
                className={cn(
                  "group relative flex min-w-0 items-center gap-2 rounded-md border px-2 py-2 text-left transition-colors",
                  on ? "border-accent bg-accent/[0.07]" : "border-line hover:border-line-strong hover:bg-raised/60",
                )}
              >
                <span
                  className={cn(
                    "flex size-6 shrink-0 items-center justify-center rounded transition-colors",
                    on ? "bg-accent text-accent-fg" : "bg-raised text-muted group-hover:text-fg",
                  )}
                >
                  <Icon className="size-3.5" />
                </span>
                <span className="min-w-0">
                  <span className="block truncate text-[13px] font-medium">{s.title}</span>
                  <span className="block truncate text-[11px] text-subtle">{s.short}</span>
                </span>
                {s.id === suggested && (
                  <span className="absolute top-1.5 right-1.5 size-1.5 rounded-full bg-ok" title={tr("根据源文件特征推荐", "Recommended for this file")} />
                )}
              </button>
            );
          })}
        </div>
      </div>
    </Section>
  );
}

export function NotWorthItBanner({ message }: { message: string }) {
  const applyFix = useProject((s) => s.applyFix);
  return (
    <div className="flex items-start gap-3 rounded-lg border border-warn/35 bg-warn/[0.09] px-3.5 py-3">
      <TriangleAlert className="mt-0.5 size-4 shrink-0 text-warn" />
      <p className="flex-1 text-[12.5px] leading-relaxed">
        <span className="font-semibold text-warn">{tr("不建议重新编码。", "Re-encoding is not recommended. ")}</span>
        <span className="text-fg/85">{message}</span>
      </p>
      <Button size="sm" variant="warn" onClick={() => applyFix("remux")}>
        {tr("改为原样封装", "Switch to Remux")}
      </Button>
    </div>
  );
}
