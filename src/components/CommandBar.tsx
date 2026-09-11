import { useState } from "react";
import { Check, ChevronsDownUp, ChevronsUpDown, Copy, ListPlus, SquareTerminal } from "lucide-react";
import type { MediaInfo, PlanResult, TranscodePlan } from "@/lib/types";
import { argsToCommand, quoteArg } from "@/lib/format";
import { useCapabilities } from "@/stores/capability";
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

export function CommandBar({ media, plan, result }: { media: MediaInfo; plan: TranscodePlan; result: PlanResult }) {
  const expanded = useUi((s) => s.commandExpanded);
  const toggle = useUi((s) => s.toggleCommand);
  const setView = useUi((s) => s.setView);
  const caps = useCapabilities((s) => s.caps);
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
    setQueued("已加入队列");
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
          <span className="shrink-0 rounded bg-raised px-1.5 py-0.5 text-[10.5px] text-muted" title="两遍编码：复制时包含第一遍的分析命令">
            两遍
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
        {expanded && <span className="flex-1 text-xs font-medium text-muted">将要执行的 ffmpeg 命令</span>}
        <div className="flex shrink-0 items-center gap-1.5">
          <Button
            size="sm"
            variant="ghost"
            icon={expanded ? <ChevronsDownUp className="size-3.5" /> : <ChevronsUpDown className="size-3.5" />}
            onClick={toggle}
            title={expanded ? "收起" : "展开查看完整命令"}
          />
          <Button
            size="sm"
            icon={copied ? <Check className="size-3.5 text-ok" /> : <Copy className="size-3.5" />}
            onClick={copy}
            title={caps.platform === "windows" ? "按 PowerShell 规则加引号" : "按 bash/zsh 规则加引号"}
          >
            {copied ? "已复制" : "复制"}
          </Button>
          <div className="mx-1 h-5 w-px bg-line" />
          {files.length > 1 && (
            <Button size="sm" onClick={addAll}>
              全部加入（{files.length}）
            </Button>
          )}
          <Button
            size="sm"
            variant="primary"
            icon={<ListPlus className="size-3.5" />}
            onClick={addOne}
            title={conflicts ? `有 ${conflicts} 项保真度要求与当前参数冲突` : undefined}
          >
            {queued ?? "加入队列"}
          </Button>
        </div>
      </div>

      {expanded && (
        <div className="max-h-[38vh] overflow-y-auto border-t border-line bg-sunken/60 px-4 py-3">
          <div className="selectable grid grid-cols-[52px_1fr] gap-x-3 gap-y-1.5 font-mono text-[11.5px] leading-relaxed">
            {result.firstPass && (
              <div className="contents" data-testid="first-pass">
                <span className="pt-px text-right font-sans text-[10.5px] text-subtle">第一遍</span>
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
                <span className="pt-px text-right font-sans text-[10.5px] text-subtle">{s.label}</span>
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
            {result.firstPass && "两遍编码：先运行第一遍（只分析画面、不输出文件），再运行其余命令。"}
            实际执行时写入 <code className="font-mono">.vidforge-part</code> 临时文件，校验通过后才改名为上面的最终文件名。
          </p>
        </div>
      )}
    </div>
  );
}
