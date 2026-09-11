import { Ban, CircleCheck, CircleHelp, TriangleAlert, Wand2 } from "lucide-react";
import { tr } from "@/i18n";
import type { FidelityItem, FidelityState, TranscodePlan } from "@/lib/types";
import { cn } from "@/lib/cn";
import { fidelityHint } from "@/lib/fidelity";
import { useProject } from "@/stores/project";
import { Badge, Button, Checkbox, Section } from "./ui";

type Look = { label: string; tone: "ok" | "warn" | "danger"; icon: typeof CircleCheck };

function stateLook(state: Exclude<FidelityState, "not_applicable">): Look {
  switch (state) {
    case "achievable":
      return { label: tr("可保留", "Kept"), tone: "ok", icon: CircleCheck };
    case "needs_change":
      return { label: tr("需调整", "Needs change"), tone: "warn", icon: TriangleAlert };
    case "impossible":
      return { label: tr("无法保留", "Not possible"), tone: "danger", icon: Ban };
  }
}

export function isProblem(item: FidelityItem, checked: boolean): boolean {
  return checked && (item.state === "needs_change" || item.state === "impossible");
}

function Row({ item, checked }: { item: FidelityItem; checked: boolean }) {
  const patch = useProject((s) => s.patchPlan);
  const applyFix = useProject((s) => s.applyFix);
  const st = item.state === "not_applicable" ? undefined : stateLook(item.state);
  const problem = isProblem(item, checked);

  return (
    <div
      data-testid={`fidelity-${item.kind}`}
      className={cn(
        problem
          ? cn(
              "col-span-full rounded-md border px-3 py-2.5",
              item.state === "impossible" ? "border-danger/30 bg-danger/[0.05]" : "border-warn/35 bg-warn/[0.06]",
            )
          : "px-1 py-1.5",
      )}
    >
      <div className="flex items-center gap-2.5">
        <Checkbox
          checked={checked}
          label={item.label}
          onChange={(on) =>
            patch((p) => {
              p.fidelity[item.kind] = on;
            })
          }
        />
        <span className={cn("truncate text-[13px]", !checked && "text-muted")} title={fidelityHint(item.kind)}>
          {item.label}
        </span>
        <span className="ml-auto shrink-0">
          {checked && st ? (
            <Badge tone={st.tone} icon={<st.icon className="size-3" />} title={problem ? undefined : item.detail}>
              {st.label}
            </Badge>
          ) : (
            <span className="text-[11px] text-subtle" title={item.detail}>
              {tr("未要求", "Not required")}
            </span>
          )}
        </span>
      </div>
      {problem && (
        <>
          <p className={cn("mt-1.5 pl-[26px] text-xs leading-relaxed", item.state === "impossible" ? "text-danger" : "text-warn")}>
            {item.detail}
          </p>
          {item.fixes.length > 0 && (
            <div className="mt-2 flex flex-wrap gap-1.5 pl-[26px]">
              {item.fixes.map((f) => (
                <Button key={f.id} size="xs" variant="warn" icon={<Wand2 className="size-3" />} onClick={() => applyFix(f.id)}>
                  {f.label}
                </Button>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}

export function FidelityPanel({ plan, items }: { plan: TranscodePlan; items: FidelityItem[] }) {
  const applicable = items.filter((i) => i.state !== "not_applicable");
  const na = items.filter((i) => i.state === "not_applicable");
  const wanted = applicable.filter((i) => plan.fidelity[i.kind]);
  const conflicts = wanted.filter((i) => i.state !== "achievable").length;
  // 冲突项排在前面，醒目展示；其余按原顺序两列排布
  const sorted = [
    ...applicable.filter((i) => isProblem(i, plan.fidelity[i.kind])),
    ...applicable.filter((i) => !isProblem(i, plan.fidelity[i.kind])),
  ];

  return (
    <Section
      step={3}
      title={
        <span className="flex items-center gap-1.5">
          {tr("尽量保留", "Keep as much as possible")}
          <span
            title={tr(
              "勾选你在意的内容。系统会判断在当前参数下能否保留，冲突时给出一键修正；转码完成后逐项核对实际结果。悬停状态标签可查看详细说明。",
              "Check what you care about. VidForge tells you whether the current settings keep it, offers one-click fixes for conflicts, and verifies the result after encoding. Hover a status label for details.",
            )}
          >
            <CircleHelp className="size-3.5 text-subtle" />
          </span>
        </span>
      }
      aside={
        wanted.length > 0 &&
        (conflicts === 0 ? (
          <Badge tone="ok" icon={<CircleCheck className="size-3" />}>
            {tr(`${wanted.length} 项均可保留`, `All ${wanted.length} kept`)}
          </Badge>
        ) : (
          <Badge tone="warn" icon={<TriangleAlert className="size-3" />}>
            {tr(`${conflicts} 项冲突`, `${conflicts} conflict${conflicts === 1 ? "" : "s"}`)}
          </Badge>
        ))
      }
    >
      {applicable.length === 0 ? (
        <p className="text-xs text-subtle">
          {tr("这个文件没有需要特别保留的高价值信息。", "This file has nothing that needs special care to keep.")}
        </p>
      ) : (
        <div className="@container">
          <div className="grid gap-x-6 gap-y-0.5 @min-[520px]:grid-cols-2">
            {sorted.map((i) => (
              <Row key={i.kind} item={i} checked={plan.fidelity[i.kind]} />
            ))}
          </div>
        </div>
      )}
      {na.length > 0 && (
        <p className="mt-2 text-[11px] text-subtle" title={tr("源文件中没有这些内容", "The source has none of these")}>
          {tr("不适用：", "Not applicable: ")}
          {na.map((i) => i.label).join(tr("、", ", "))}
        </p>
      )}
    </Section>
  );
}
