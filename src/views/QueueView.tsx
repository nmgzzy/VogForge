import { useState } from "react";
import { useShallow } from "zustand/react/shallow";
import {
  ArrowDown,
  ArrowUp,
  Ban,
  CircleCheck,
  CircleX,
  Clock,
  Copy,
  Cpu,
  FolderOpen,
  ListVideo,
  Minus,
  Pause,
  Play,
  Plus,
  RotateCcw,
  SkipForward,
  Trash2,
  TriangleAlert,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { tr } from "@/i18n";
import type { Job, JobStatus } from "@/lib/types";
import { cn } from "@/lib/cn";
import { argsToCommand, formatBytes, formatDuration, formatEta, formatFps } from "@/lib/format";
import { encoderVendor, isHardware, vendorLabel } from "@/lib/encoders";
import { scenarioTitle } from "@/lib/scenarios";
import { backend } from "@/backend";
import { useQueue } from "@/stores/queue";
import { useSettings } from "@/stores/settings";
import { useUi } from "@/stores/ui";
import { Badge, Button, Empty, ProgressBar, RawDetail } from "@/components/ui";

function statusLook(status: JobStatus): { label: string; icon: LucideIcon; cls: string } {
  switch (status) {
    case "queued":
      return { label: tr("排队中", "Queued"), icon: Clock, cls: "text-subtle" };
    case "running":
      return { label: tr("进行中", "Running"), icon: Play, cls: "text-accent" };
    case "paused":
      return { label: tr("已暂停", "Paused"), icon: Pause, cls: "text-warn" };
    case "done":
      return { label: tr("已完成", "Done"), icon: CircleCheck, cls: "text-ok" };
    case "skipped":
      return { label: tr("已跳过", "Skipped"), icon: SkipForward, cls: "text-subtle" };
    case "failed":
      return { label: tr("失败", "Failed"), icon: CircleX, cls: "text-danger" };
    case "cancelled":
      return { label: tr("已取消", "Cancelled"), icon: Ban, cls: "text-subtle" };
  }
}

/** 回退事件：后端在两种语言下都写明"回退" */
const isFallback = (message: string) => message.includes("回退") || message.includes("falling back");

function EncoderChip({ job }: { job: Job }) {
  if (job.plan.video.action === "copy") return <Badge>{tr("流复制", "Stream copy")}</Badge>;
  const hw = isHardware(job.encoderUsed);
  return (
    <Badge tone={hw ? "accent" : "neutral"} icon={hw ? <Zap className="size-3" /> : <Cpu className="size-3" />}>
      <span className="font-mono">{job.encoderUsed}</span>
    </Badge>
  );
}

function JobRow({ job, selected }: { job: Job; selected: boolean }) {
  // 只取动作函数（引用稳定），避免进度刷新时整行重渲染之外的额外订阅
  const q = useQueue(
    useShallow((s) => ({
      select: s.select, pause: s.pause, resume: s.resume, cancel: s.cancel,
      retry: s.retry, remove: s.remove, move: s.move,
    })),
  );
  const st = statusLook(job.status);
  const p = job.progress;
  const scenario = scenarioTitle(job.plan.scenario);
  const fellBack = job.events.some((e) => e.level === "warn" && isFallback(e.message));
  const lastError = job.events.filter((e) => e.level === "error").at(-1);

  return (
    <div
      onClick={() => q.select(job.id)}
      className={cn(
        "group cursor-default rounded-lg border px-3.5 py-3 transition-colors",
        selected ? "border-accent/50 bg-accent/[0.05]" : "border-line bg-panel hover:border-line-strong",
      )}
    >
      <div className="flex items-center gap-2.5">
        <st.icon className={cn("size-4 shrink-0", st.cls)} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="truncate text-[13px] font-medium" title={job.media.name}>
              {job.media.name}
            </span>
            {fellBack && (
              <Badge
                tone="warn"
                icon={<TriangleAlert className="size-3" />}
                title={tr("失败后自动回退了编码器", "The encoder was switched automatically after a failure")}
              >
                {tr("已回退", "Fell back")}
              </Badge>
            )}
          </div>
          <div className="mt-0.5 truncate text-[11.5px] text-muted">
            {scenario} · {job.outputPath.split(/[\\/]/).pop()}
          </div>
        </div>
        <EncoderChip job={job} />
        <div className="flex items-center gap-0.5 opacity-70 group-hover:opacity-100">
          {job.status === "running" && (
            <Button size="xs" variant="ghost" icon={<Pause className="size-3.5" />} title={tr("暂停", "Pause")} onClick={(e) => (e.stopPropagation(), q.pause(job.id))} />
          )}
          {job.status === "paused" && (
            <Button size="xs" variant="ghost" icon={<Play className="size-3.5" />} title={tr("继续", "Resume")} onClick={(e) => (e.stopPropagation(), q.resume(job.id))} />
          )}
          {job.status === "queued" && (
            <>
              <Button size="xs" variant="ghost" icon={<ArrowUp className="size-3.5" />} title={tr("上移", "Move up")} onClick={(e) => (e.stopPropagation(), q.move(job.id, -1))} />
              <Button size="xs" variant="ghost" icon={<ArrowDown className="size-3.5" />} title={tr("下移", "Move down")} onClick={(e) => (e.stopPropagation(), q.move(job.id, 1))} />
            </>
          )}
          {(job.status === "failed" || job.status === "cancelled" || job.status === "skipped") && (
            <Button size="xs" variant="ghost" icon={<RotateCcw className="size-3.5" />} title={tr("重试", "Retry")} onClick={(e) => (e.stopPropagation(), q.retry(job.id))} />
          )}
          {(job.status === "running" || job.status === "paused" || job.status === "queued") && (
            <Button size="xs" variant="ghost" icon={<Ban className="size-3.5" />} title={tr("取消", "Cancel")} onClick={(e) => (e.stopPropagation(), q.cancel(job.id))} />
          )}
          {(job.status === "done" || job.status === "skipped" || job.status === "failed" || job.status === "cancelled") && (
            <Button size="xs" variant="ghost" icon={<Trash2 className="size-3.5" />} title={tr("从列表移除", "Remove from the list")} onClick={(e) => (e.stopPropagation(), q.remove(job.id))} />
          )}
        </div>
      </div>

      {(job.status === "running" || job.status === "paused") && (
        <div className="mt-2.5 pl-[26px]">
          <ProgressBar value={p.percent} live={job.status === "running"} tone={job.status === "paused" ? "muted" : "accent"} />
          <div className="mt-1.5 flex flex-wrap gap-x-4 gap-y-0.5 text-[11.5px] text-muted tabular">
            <span className="font-semibold text-fg">{p.percent.toFixed(1)}%</span>
            {p.pass && (
              <span title={tr("两遍编码：第一遍分析，第二遍输出", "Two-pass: the first pass analyzes, the second writes the output")}>
                {tr(`第 ${p.pass}/2 遍`, `pass ${p.pass}/2`)}
              </span>
            )}
            <span>
              {formatDuration(p.outTimeSec)} / {formatDuration(job.media.durationSec)}
            </span>
            <span>{p.speed.toFixed(2)}×</span>
            <span>{formatFps(p.fps)} fps</span>
            <span>{formatBytes(p.sizeBytes)}</span>
            {p.dupFrames > 0 && (
              <span>{tr(`复制 ${p.dupFrames.toLocaleString()} 帧`, `${p.dupFrames.toLocaleString()} frames duplicated`)}</span>
            )}
            <span className="ml-auto">{tr(`剩余 ${formatEta(p.etaSec)}`, `${formatEta(p.etaSec)} left`)}</span>
          </div>
        </div>
      )}
      {job.status === "done" && job.outputSize && (
        <div className="mt-1.5 pl-[26px] text-[11.5px] text-muted tabular">
          {formatBytes(job.media.sizeBytes)} → <span className="font-medium text-ok">{formatBytes(job.outputSize)}</span>
          {tr(`（${Math.round((job.outputSize / job.media.sizeBytes) * 100)}%）`, ` (${Math.round((job.outputSize / job.media.sizeBytes) * 100)}%)`)}
          {job.report && (
            <span className={job.report.every((r) => r.ok) ? "" : "text-warn"}>
              {tr(
                ` · 校验 ${job.report.filter((r) => r.ok).length}/${job.report.length} 通过`,
                ` · ${job.report.filter((r) => r.ok).length}/${job.report.length} checks passed`,
              )}
            </span>
          )}
        </div>
      )}
      {job.status === "failed" && lastError && (
        <div className="mt-1.5 pl-[26px]" onClick={(e) => e.stopPropagation()}>
          <p className="text-[11.5px] leading-relaxed text-danger">{lastError.message}</p>
          <RawDetail text={lastError.detail} />
        </div>
      )}
    </div>
  );
}

type Tab = "overview" | "report" | "log" | "command";

/** 任务的完整命令；两遍编码是两行 */
function commandText(job: Job): string {
  return [job.firstPass, job.args].filter((a): a is string[] => !!a).map((a) => argsToCommand(a)).join("\n");
}

function JobDetail({ job }: { job: Job }) {
  const [tab, setTab] = useState<Tab>("overview");
  const [copied, setCopied] = useState(false);
  const vendor = encoderVendor(job.encoderUsed);
  const tabs: { id: Tab; label: string }[] = [
    { id: "overview", label: tr("概览", "Overview") },
    { id: "report", label: tr("保真度报告", "Fidelity report") },
    { id: "log", label: tr("日志", "Log") },
    { id: "command", label: tr("命令", "Command") },
  ];
  const status = statusLook(job.status);

  return (
    <div className="flex h-full flex-col">
      <header className="border-b border-line px-4 pt-3">
        <p className="truncate text-[13px] font-semibold" title={job.media.name}>
          {job.media.name}
        </p>
        <div className="mt-2 flex gap-4">
          {tabs.map((t) => (
            <button
              key={t.id}
              onClick={() => setTab(t.id)}
              className={cn(
                "-mb-px border-b-2 pb-2 text-xs transition-colors",
                tab === t.id ? "border-accent font-medium text-fg" : "border-transparent text-muted hover:text-fg",
              )}
            >
              {t.label}
            </button>
          ))}
        </div>
      </header>

      <div className="flex-1 overflow-y-auto p-4">
        {tab === "overview" && (
          <div className="space-y-4">
            <dl className="grid grid-cols-[64px_1fr] gap-x-3 gap-y-1.5 text-xs">
              <dt className="text-subtle">{tr("状态", "Status")}</dt>
              <dd className={status.cls}>{status.label}</dd>
              <dt className="text-subtle">{tr("编码器", "Encoder")}</dt>
              <dd>
                <span className="font-mono">{job.encoderUsed}</span>
                <span className="text-muted"> · {vendorLabel(vendor)}</span>
              </dd>
              <dt className="text-subtle">{tr("输出", "Output")}</dt>
              <dd className="selectable font-mono text-[11px] break-all text-muted">
                {job.outputPath}
                {job.status === "done" && backend.kind === "tauri" && (
                  <button
                    className="ml-1.5 inline-flex items-center gap-1 align-middle font-sans text-accent hover:underline"
                    onClick={() => void backend.revealPath(job.outputPath)}
                  >
                    <FolderOpen className="size-3" />
                    {tr("在文件夹中显示", "Show in folder")}
                  </button>
                )}
              </dd>
              {job.startedAt && (
                <>
                  <dt className="text-subtle">{tr("耗时", "Elapsed")}</dt>
                  <dd className="tabular">{formatDuration(((job.finishedAt ?? Date.now()) - job.startedAt) / 1000)}</dd>
                </>
              )}
            </dl>
            <div>
              <h3 className="mb-2 text-xs font-semibold text-muted">{tr("事件", "Events")}</h3>
              <ol className="relative space-y-2.5 border-l border-line pl-4">
                {job.events.map((e, i) => (
                  <li key={i} className="relative text-xs">
                    <span
                      className={cn(
                        "absolute top-1 -left-[21px] size-2 rounded-full ring-4 ring-panel",
                        e.level === "error" ? "bg-danger" : e.level === "warn" ? "bg-warn" : "bg-line-strong",
                      )}
                    />
                    <div className="text-[10.5px] text-subtle tabular">{new Date(e.at).toLocaleTimeString()}</div>
                    <p className={cn("leading-relaxed", e.level === "error" ? "text-danger" : e.level === "warn" ? "text-warn" : "text-fg/90")}>
                      {e.message}
                    </p>
                    <RawDetail text={e.detail} />
                  </li>
                ))}
              </ol>
            </div>
          </div>
        )}

        {tab === "report" &&
          (job.report ? (
            <div className="space-y-3">
              <p className="text-xs text-muted">
                {tr(
                  "转码完成后用 ffprobe 读取输出文件，逐项核对是否符合预期。",
                  "After encoding, ffprobe reads the output file and checks each item against what was expected.",
                )}
              </p>
              <div className="overflow-hidden rounded-lg border border-line">
                <table className="w-full text-xs">
                  <thead className="bg-raised text-left text-[11px] text-subtle">
                    <tr>
                      <th className="px-3 py-2 font-medium">{tr("项目", "Item")}</th>
                      <th className="px-3 py-2 font-medium">{tr("期望", "Expected")}</th>
                      <th className="px-3 py-2 font-medium">{tr("实际", "Actual")}</th>
                      <th className="w-8" />
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-line">
                    {job.report.map((r, i) => (
                      <tr key={i}>
                        <td className="px-3 py-2 font-medium">{r.label}</td>
                        <td className="px-3 py-2 text-muted">{r.expected}</td>
                        <td className="px-3 py-2 text-muted">{r.actual}</td>
                        <td className="pr-3">
                          {r.ok ? <CircleCheck className="size-4 text-ok" /> : <CircleX className="size-4 text-danger" />}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          ) : (
            <Empty
              icon={<Clock className="size-5" />}
              title={tr("完成后生成", "Available when done")}
              description={tr(
                "转码完成后会逐项核对你勾选要保留的内容是否真的保留下来。",
                "After encoding, each item you asked to keep is checked against the actual output.",
              )}
            />
          ))}

        {tab === "log" &&
          (job.log.length ? (
            <pre className="selectable overflow-x-auto rounded-lg bg-sunken p-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-muted">
              {job.log.join("\n")}
            </pre>
          ) : (
            <Empty
              icon={<ListVideo className="size-5" />}
              title={tr("暂无日志", "No log yet")}
              description={tr("开始转码后，这里显示 ffmpeg 输出的最后 500 行。", "Once encoding starts, the last 500 lines of ffmpeg output appear here.")}
            />
          ))}

        {tab === "command" && (
          <div className="space-y-2">
            {job.firstPass && (
              <p className="text-xs text-muted">
                {tr(
                  "两遍编码：先运行第一遍（只分析，不输出文件），再运行第二遍。",
                  "Two-pass: run the first pass (analysis only, no output file), then the second.",
                )}
              </p>
            )}
            <pre className="selectable overflow-x-auto rounded-lg bg-sunken p-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-all">
              {commandText(job)}
            </pre>
            <Button
              size="sm"
              icon={<Copy className="size-3.5" />}
              onClick={async () => {
                try {
                  await navigator.clipboard.writeText(commandText(job));
                  setCopied(true);
                  window.setTimeout(() => setCopied(false), 1500);
                } catch {
                  /* 剪贴板不可用时用户可手动选择 */
                }
              }}
            >
              {copied ? tr("已复制", "Copied") : tr("复制命令", "Copy command")}
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}

function Stepper({ label, value, onChange, icon }: { label: string; value: number; onChange: (n: number) => void; icon: React.ReactNode }) {
  return (
    <div className="flex items-center gap-1.5 text-xs text-muted" title={tr(`${label}同时运行的任务数`, `Jobs running at once on the ${label}`)}>
      {icon}
      {label}
      <div className="flex items-center rounded-md border border-line bg-panel">
        <button className="flex size-6 items-center justify-center hover:text-fg" onClick={() => onChange(value - 1)}>
          <Minus className="size-3" />
        </button>
        <span className="w-4 text-center font-medium text-fg tabular">{value}</span>
        <button className="flex size-6 items-center justify-center hover:text-fg" onClick={() => onChange(value + 1)}>
          <Plus className="size-3" />
        </button>
      </div>
    </div>
  );
}

export function QueueView() {
  const jobs = useQueue((s) => s.jobs);
  const selectedId = useQueue((s) => s.selectedJobId);
  const paused = useQueue((s) => s.paused);
  const setPaused = useQueue((s) => s.setGlobalPaused);
  const clearFinished = useQueue((s) => s.clearFinished);
  const error = useQueue((s) => s.error);
  const dismissError = useQueue((s) => s.dismissError);
  // 并发数是设置项，改了立即作用于后端的调度
  const cpuSlots = useSettings((s) => s.settings.cpuSlots);
  const gpuSlots = useSettings((s) => s.settings.gpuSlots);
  const updateSettings = useSettings((s) => s.update);
  const setView = useUi((s) => s.setView);
  const selected = jobs.find((j) => j.id === selectedId);
  const count = (s: JobStatus) => jobs.filter((j) => j.status === s).length;

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center gap-3 border-b border-line px-5">
        <h1 className="shrink-0 text-[14px] font-semibold whitespace-nowrap">{tr("任务队列", "Queue")}</h1>
        {/* 最小窗口（960）放不下统计，任务列表里本来就有状态 */}
        <div className="hidden gap-1.5 text-[11.5px] min-[1100px]:flex">
          <Badge tone="accent">{tr(`进行中 ${count("running")}`, `Running ${count("running")}`)}</Badge>
          <Badge>{tr(`排队 ${count("queued")}`, `Queued ${count("queued")}`)}</Badge>
          <Badge tone="ok">{tr(`完成 ${count("done")}`, `Done ${count("done")}`)}</Badge>
          {count("failed") > 0 && <Badge tone="danger">{tr(`失败 ${count("failed")}`, `Failed ${count("failed")}`)}</Badge>}
        </div>
        <div
          className="ml-auto flex items-center gap-4"
          title={tr(
            "CPU 软编与 GPU 硬编分别计数：x265 会吃满所有核心，并行两个软编只会互相拖慢；一个软编加一个硬编可以同时跑",
            "CPU and GPU jobs are counted separately: x265 uses every core, so two software jobs only slow each other down, while one CPU and one GPU job run well together",
          )}
        >
          <Stepper
            label="CPU"
            icon={<Cpu className="size-3.5" />}
            value={cpuSlots}
            onChange={(n) => void updateSettings({ cpuSlots: Math.min(4, Math.max(1, n)) })}
          />
          <Stepper
            label="GPU"
            icon={<Zap className="size-3.5" />}
            value={gpuSlots}
            onChange={(n) => void updateSettings({ gpuSlots: Math.min(2, Math.max(1, n)) })}
          />
          <div className="h-5 w-px bg-line" />
          <Button size="sm" icon={paused ? <Play className="size-3.5" /> : <Pause className="size-3.5" />} onClick={() => setPaused(!paused)}>
            {paused ? tr("全部继续", "Resume all") : tr("全部暂停", "Pause all")}
          </Button>
          <Button size="sm" variant="ghost" onClick={clearFinished}>
            {tr("清除已完成", "Clear finished")}
          </Button>
        </div>
      </header>

      {jobs.length === 0 ? (
        <div className="flex flex-1 items-center justify-center">
          <Empty
            icon={<ListVideo className="size-5" />}
            title={tr("队列是空的", "The queue is empty")}
            description={tr("在转码页设置好参数后，点击“加入队列”。", "Set things up on the Transcode page, then click \"Add to queue\".")}
            action={
              <Button size="sm" variant="primary" onClick={() => setView("transcode")}>
                {tr("去添加", "Add videos")}
              </Button>
            }
          />
        </div>
      ) : (
        <div className="flex min-h-0 flex-1">
          <div className="flex-1 overflow-y-auto p-4">
            {paused && (
              <div className="mb-3 rounded-md border border-warn/30 bg-warn/10 px-3 py-2 text-xs text-warn">
                {tr(
                  "队列已暂停：进行中的任务保持当前进度，排队任务不会开始。",
                  "The queue is paused: running jobs keep their progress and queued jobs will not start.",
                )}
              </div>
            )}
            {error && (
              <div role="alert" className="mb-3 flex items-center gap-2 rounded-md border border-danger/30 bg-danger/[0.06] px-3 py-2 text-xs text-danger">
                <span className="flex-1">{error}</span>
                <button className="text-muted hover:text-fg" onClick={dismissError}>
                  {tr("知道了", "Dismiss")}
                </button>
              </div>
            )}
            <div className="flex flex-col gap-2">
              {jobs.map((j) => (
                <JobRow key={j.id} job={j} selected={j.id === selectedId} />
              ))}
            </div>
          </div>
          <aside className="w-[400px] shrink-0 border-l border-line bg-panel">
            {selected ? <JobDetail key={selected.id} job={selected} /> : <Empty icon={<ListVideo className="size-5" />} title={tr("选择一个任务", "Select a job")} />}
          </aside>
        </div>
      )}
    </div>
  );
}
