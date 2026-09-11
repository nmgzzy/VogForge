import { useState } from "react";
import { Check, ChevronsDownUp, ChevronsUpDown, Copy, ListPlus, SquareTerminal } from "lucide-react";
import { tr } from "@/i18n";
import type { MediaInfo, PlanResult, SegmentKind, TranscodePlan } from "@/lib/types";
import { argsToCommand, quoteArg } from "@/lib/format";
import { useEngineCaps } from "@/stores/engine-caps";
import { useProject } from "@/stores/project";
import { useQueue } from "@/stores/queue";
import { useUi } from "@/stores/ui";
import { Button } from "./ui";

/** 命令里的参数着色：选项名、值、路径各用一种颜色，便于扫读 */
function Token({ a }: { a: string }) {
  const q = quoteArg(a);
  // 选项名很短且连字符后是合法断行点，整体不换行，避免 "-color_trc" 被拆成两行
  const cls = a.startsWith("-")
    ? "text-accent whitespace-nowrap"
    : /[\\/]/.test(a) && /\.\w{2,4}$/.test(a)
      ? "text-lossless"
      : a === "ffmpeg"
        ? "text-fg font-semibold"
        : "text-fg/85";
  return <span className={cls}>{q}</span>;
}

/** 命令段的名字 */
function segmentLabel(kind: SegmentKind): string {
  switch (kind) {
    case "global":
      return tr("全局", "Global");
    case "input":
      return tr("输入", "Input");
    case "video":
      return tr("视频", "Video");
    case "filter":
      return tr("滤镜", "Filter");
    case "fps":
      return tr("帧率", "FPS");
    case "map":
      return tr("映射", "Map");
    case "audio":
      return tr("音频", "Audio");
    case "subtitle":
      return tr("字幕", "Subs");
    case "mux":
      return tr("封装", "Mux");
    case "output":
      return tr("输出", "Output");
  }
}

export function CommandBar({ media, plan, result }: { media: MediaInfo; plan: TranscodePlan; result: PlanResult }) {
  const expanded = useUi((s) => s.commandExpanded);
  const toggle = useUi((s) => s.toggleCommand);
  const setView = useUi((s) => s.setView);
  const caps = useEngineCaps();
  const enqueue = useQueue((s) => s.enqueue);
  const files = useProject((s) => s.files);
  const plans = useProject((s) => s.plans);
  const [copied, setCopied] = useState(false);
  const [queued, setQueued] = useState<string>();

  const segs = result.segments;
  const shell = caps.platform === "windows" ? "powershell" : "posix";
  // 两遍编码复制两行：先第一遍分析，再第二遍输出
  const cmd = [result.firstPass, result.args]
    .filter((a): a is string[] => !!a)
    .map((a) => argsToCommand(a, shell))
    .join("\n");
  const conflicts = result.fidelity.filter(
    (f) => plan.fidelity[f.kind] && (f.state === "needs_change" || f.state === "impossible"),
  ).length;

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(cmd);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      /* 剪贴板权限被拒时静默失败，用户仍可展开后手动选择复制 */
    }
  };

  const addOne = () => {
    enqueue([{ media, plan }]);
    setQueued(tr("已加入队列", "Added"));
    window.setTimeout(() => setQueued(undefined), 2200);
  };
  const addAll = () => {
    enqueue(files.map((f) => ({ media: f, plan: plans[f.id]! })).filter((x) => x.plan));
    setView("queue");
  };

  return (
    <div className="border-t border-line bg-panel">
      <div className="flex items-center gap-3 px-4 py-2.5">
        <SquareTerminal className="size-4 shrink-0 text-subtle" />
        {!expanded && result.firstPass && (
          <span
            className="shrink-0 rounded bg-raised px-1.5 py-0.5 text-[10.5px] text-muted"
            title={tr("两遍编码：复制时包含第一遍的分析命令", "Two-pass: copying includes the first-pass analysis command")}
          >
            {tr("两遍", "2-pass")}
          </span>
        )}
        {!expanded && (
          <code className="selectable min-w-0 flex-1 truncate font-mono text-[11.5px]" title={cmd}>
            {result.args.map((a, i) => (
              <span key={i}>
                {i > 0 && " "}
                <Token a={a} />
              </span>
            ))}
          </code>
        )}
        {expanded && (
          <span className="flex-1 text-xs font-medium text-muted">{tr("将要执行的 ffmpeg 命令", "The ffmpeg command to run")}</span>
        )}
        <div className="flex shrink-0 items-center gap-1.5">
          <Button
            size="sm"
            variant="ghost"
            icon={expanded ? <ChevronsDownUp className="size-3.5" /> : <ChevronsUpDown className="size-3.5" />}
            onClick={toggle}
            title={expanded ? tr("收起", "Collapse") : tr("展开查看完整命令", "Expand to see the full command")}
          />
          <Button
            size="sm"
            icon={copied ? <Check className="size-3.5 text-ok" /> : <Copy className="size-3.5" />}
            onClick={copy}
            title={
              caps.platform === "windows"
                ? tr("按 PowerShell 规则加引号", "Quoted for PowerShell")
                : tr("按 bash/zsh 规则加引号", "Quoted for bash/zsh")
            }
          >
            {copied ? tr("已复制", "Copied") : tr("复制", "Copy")}
          </Button>
          <div className="mx-1 h-5 w-px bg-line" />
          {files.length > 1 && (
            <Button size="sm" onClick={addAll}>
              {tr(`全部加入（${files.length}）`, `Add all (${files.length})`)}
            </Button>
          )}
          <Button
            size="sm"
            variant="primary"
            icon={<ListPlus className="size-3.5" />}
            onClick={addOne}
            title={
              conflicts
                ? tr(
                    `有 ${conflicts} 项保真度要求与当前参数冲突`,
                    `${conflicts} fidelity requirement(s) conflict with the current settings`,
                  )
                : undefined
            }
          >
            {queued ?? tr("加入队列", "Add to queue")}
          </Button>
        </div>
      </div>

      {expanded && (
        <div className="max-h-[38vh] overflow-y-auto border-t border-line bg-sunken/60 px-4 py-3">
          <div className="selectable grid grid-cols-[56px_1fr] gap-x-3 gap-y-1.5 font-mono text-[11.5px] leading-relaxed">
            {result.loudnessMeasure?.map((m, k) => (
              <div key={`m${k}`} className="contents" data-testid="loudness-measure">
                <span className="pt-px text-right font-sans text-[10.5px] text-subtle">{tr("测量响度", "Loudness")}</span>
                <span className="text-muted [overflow-wrap:anywhere]">
                  {m.map((a, j) => (
                    <span key={j}>
                      {j > 0 && " "}
                      <Token a={a} />
                    </span>
                  ))}
                </span>
              </div>
            ))}
            {result.firstPass && (
              <div className="contents" data-testid="first-pass">
                <span className="pt-px text-right font-sans text-[10.5px] text-subtle">{tr("第一遍", "Pass 1")}</span>
                <span className="border-b border-line pb-2 [overflow-wrap:anywhere]">
                  {result.firstPass.map((a, j) => (
                    <span key={j}>
                      {j > 0 && " "}
                      <Token a={a} />
                    </span>
                  ))}
                </span>
              </div>
            )}
            {segs.map((s, i) => (
              <div key={i} className="contents">
                <span className="pt-px text-right font-sans text-[10.5px] text-subtle">{segmentLabel(s.kind)}</span>
                <span className="[overflow-wrap:anywhere]">
                  {s.args.map((a, j) => (
                    <span key={j}>
                      {j > 0 && " "}
                      <Token a={a} />
                    </span>
                  ))}
                </span>
              </div>
            ))}
          </div>
          <p className="mt-3 font-sans text-[11px] text-subtle">
            {result.loudnessMeasure &&
              tr(
                "响度标准化：执行时先测量每条音轨的响度，再把测得的值填进 loudnorm（预览里是单遍写法）。",
                "Loudness normalization: each audio track is measured first and the values are passed to loudnorm (the preview shows the single-pass form). ",
              )}
            {result.firstPass &&
              tr(
                "两遍编码：先运行第一遍（只分析画面、不输出文件），再运行其余命令。",
                "Two-pass: the first pass runs first (analysis only, no output file), then the rest. ",
              )}
            {tr("实际执行时写入 ", "Encoding writes to a ")}
            <code className="font-mono">.vidforge-part</code>
            {tr(
              " 临时文件，校验通过后才改名为上面的最终文件名。",
              " temporary file that is renamed to the final name above only after verification.",
            )}
          </p>
        </div>
      )}
    </div>
  );
}
