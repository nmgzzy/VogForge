/**
 * 浏览器预览的队列：模拟 ffmpeg 的进度推进。接口与事件和桌面端一致（后端的 queue://snapshot 与
 * queue://progress），界面与 store 不感知差别。调度规则照搬后端：软编占 CPU 票、硬编占 GPU 票、
 * 原样封装占 IO 票，暂停的任务仍占票，全部暂停时进行中的挂起、排队的不开始。
 */
import { tr } from "@/i18n";
import type { Job, JobEvent, JobProgressEvent, QueueItem, QueueOp, QueueSnapshot, Settings } from "@/lib/types";
import { formatBytes } from "@/lib/format";
import { isHardware } from "@/lib/encoders";
import { buildReport, estimatedOutputSize, makeJob, simulatedSpeed } from "@/mock/queue";

/** 演示加速倍数：让进度条在预览里肉眼可见地推进 */
const DEMO_ACCEL = 25;

type Ticket = "cpu" | "gpu" | "io";
/** 与后端一致：原样封装只受磁盘读写限制，单独占一张 IO 票 */
const ticketOf = (j: Job): Ticket => (j.plan.video.action === "copy" ? "io" : isHardware(j.encoderUsed) ? "gpu" : "cpu");
const event = (level: JobEvent["level"], message: string): JobEvent => ({ at: Date.now(), level, message });
const active = (j: Job) => j.status === "running" || j.status === "paused";
const finished = (j: Job) => ["done", "skipped", "failed", "cancelled"].includes(j.status);

export class MockQueue {
  private jobs?: Job[];
  private paused = false;
  /** 被"全部暂停"挂起的任务，全部继续时只恢复这些 */
  private held = new Set<string>();
  private snapshotListeners = new Set<(s: QueueSnapshot) => void>();
  private progressListeners = new Set<(e: JobProgressEvent) => void>();
  private timer?: ReturnType<typeof setInterval>;

  constructor(
    private readonly settings: () => Settings,
    private readonly seed: () => Job[],
  ) {}

  /** 演示任务在第一次使用时才生成：决策引擎要先初始化 */
  private get list(): Job[] {
    this.jobs ??= this.seed();
    return this.jobs;
  }

  /** 测试用：回到预置任务 */
  reset(): void {
    this.jobs = undefined;
    this.paused = false;
    this.held.clear();
    this.publish();
  }

  snapshot(): QueueSnapshot {
    return { jobs: structuredClone(this.list), paused: this.paused };
  }

  private publish(): void {
    const s = this.snapshot();
    this.snapshotListeners.forEach((cb) => cb(s));
  }

  onSnapshot(cb: (s: QueueSnapshot) => void): () => void {
    this.snapshotListeners.add(cb);
    this.startTimer();
    return () => {
      this.snapshotListeners.delete(cb);
      if (this.snapshotListeners.size === 0) this.stopTimer();
    };
  }

  onProgress(cb: (e: JobProgressEvent) => void): () => void {
    this.progressListeners.add(cb);
    return () => this.progressListeners.delete(cb);
  }

  add(items: QueueItem[]): string[] {
    const fresh = items.map(({ media, plan }) => makeJob(media, plan, this.settings()));
    this.list.push(...fresh);
    this.publish();
    return fresh.map((j) => j.id);
  }

  control(op: QueueOp): void {
    const jobs = this.list;
    const find = (id: string) => {
      const j = jobs.find((x) => x.id === id);
      if (!j) throw new Error(tr(`没有这个任务：${id}`, `No such job: ${id}`));
      return j;
    };
    switch (op.kind) {
      case "pause": {
        const j = find(op.id);
        if (j.status !== "running") throw new Error(tr("只有进行中的任务可以暂停", "Only running jobs can be paused"));
        j.status = "paused";
        j.events.push(event("info", tr("已暂停", "Paused")));
        break;
      }
      case "resume": {
        const j = find(op.id);
        if (j.status !== "paused") throw new Error(tr("只有已暂停的任务可以继续", "Only paused jobs can be resumed"));
        j.status = "running";
        this.held.delete(j.id);
        j.events.push(event("info", tr("已继续", "Resumed")));
        break;
      }
      case "cancel": {
        const j = find(op.id);
        if (finished(j)) throw new Error(tr("任务已经结束", "The job has already finished"));
        const wasActive = active(j);
        j.status = "cancelled";
        j.finishedAt = Date.now();
        j.progress.etaSec = undefined;
        const msg = wasActive
          ? tr(
              "已取消，临时文件已删除，目标目录无残留",
              "Cancelled; the temporary file was deleted and nothing was left in the output folder",
            )
          : tr("已取消", "Cancelled");
        j.events.push(event("info", msg));
        break;
      }
      case "retry": {
        const j = find(op.id);
        if (!finished(j) || j.status === "done") {
          throw new Error(tr("只有失败、取消或跳过的任务可以重试", "Only failed, cancelled or skipped jobs can be retried"));
        }
        Object.assign(j, {
          status: "queued",
          progress: { percent: 0, outTimeSec: 0, speed: 0, fps: 0, sizeBytes: 0, dupFrames: 0, dropFrames: 0 },
          report: undefined,
          finishedAt: undefined,
          outputSize: undefined,
        } satisfies Partial<Job>);
        j.events.push(event("info", tr("重新加入队列", "Queued again")));
        break;
      }
      case "remove": {
        if (active(find(op.id))) throw new Error(tr("进行中的任务要先取消", "Cancel the running job first"));
        this.jobs = jobs.filter((j) => j.id !== op.id);
        break;
      }
      case "move": {
        const i = jobs.findIndex((j) => j.id === op.id);
        const k = i + Math.sign(op.delta);
        if (i >= 0 && k >= 0 && k < jobs.length) [jobs[i], jobs[k]] = [jobs[k]!, jobs[i]!];
        break;
      }
      case "set_paused": {
        this.paused = op.paused;
        if (op.paused) {
          for (const j of jobs.filter((x) => x.status === "running")) {
            j.status = "paused";
            this.held.add(j.id);
          }
        } else {
          for (const j of jobs.filter((x) => this.held.has(x.id) && x.status === "paused")) j.status = "running";
          this.held.clear();
        }
        break;
      }
      case "clear_finished":
        this.jobs = jobs.filter((j) => !["done", "skipped", "cancelled"].includes(j.status));
        break;
    }
    this.publish();
  }

  /** 推进 dt 秒：运行中的任务前进，完成的生成报告，空出的票据放行排队任务 */
  tick(dt: number): void {
    if (this.paused) return;
    let changed = false;
    for (const j of this.list) {
      if (j.status !== "running") continue;
      const base = simulatedSpeed(j);
      const speed = base * (0.94 + Math.random() * 0.12);
      const dur = j.media.durationSec;
      const out = Math.min(dur, j.progress.outTimeSec + speed * dt * DEMO_ACCEL);
      const percent = (out / dur) * 100;
      const srcFps = j.media.video[0]?.fpsAvg ?? 30;
      const cfr = j.plan.video.fps.kind === "cfr" ? j.plan.video.fps.fps : undefined;
      const dup = cfr && j.media.video[0]?.isVfr ? Math.round(Math.max(0, (cfr - srcFps) * out)) : 0;
      const size = Math.round(estimatedOutputSize(j) * (percent / 100));
      if (out >= dur) {
        changed = true;
        Object.assign(j, {
          status: "done",
          finishedAt: Date.now(),
          outputSize: size,
          progress: { ...j.progress, percent: 100, outTimeSec: dur, sizeBytes: size, etaSec: undefined },
        } satisfies Partial<Job>);
        j.report = buildReport(j);
        const bad = j.report.filter((r) => !r.ok);
        const sizes = `${formatBytes(j.media.sizeBytes)} → ${formatBytes(size)}`;
        const list = bad.map((r) => r.label).join(tr("、", ", "));
        j.events.push(
          bad.length
            ? event(
                "warn",
                tr(
                  `已完成（${sizes}），但 ${bad.length} 项与预期不符：${list}`,
                  `Done (${sizes}), but ${bad.length} item(s) differ from what was expected: ${list}`,
                ),
              )
            : event("info", tr(`完成，校验通过：${sizes}`, `Done, all checks passed: ${sizes}`)),
        );
        continue;
      }
      j.progress = {
        percent,
        outTimeSec: out,
        speed,
        fps: speed * (cfr ?? srcFps),
        sizeBytes: size,
        // 真实 ETA 按编码器实际速度计算；前 5 秒不显示（速度尚不可信）
        etaSec: out > 5 ? (dur - out) / speed : undefined,
        dupFrames: dup,
        dropFrames: 0,
      };
      const e: JobProgressEvent = { id: j.id, progress: { ...j.progress } };
      this.progressListeners.forEach((cb) => cb(e));
    }

    // 按票据放行排队任务。暂停只是挂起进程，内存与 GPU 会话仍被占用，所以同样占票
    const s = this.settings();
    const limit: Record<Ticket, number> = { cpu: s.cpuSlots, gpu: s.gpuSlots, io: 1 };
    const holding = (t: Ticket) => this.list.filter((j) => active(j) && ticketOf(j) === t).length;
    for (const j of this.list) {
      if (j.status !== "queued") continue;
      const t = ticketOf(j);
      if (holding(t) >= limit[t]) continue;
      changed = true;
      j.status = "running";
      j.startedAt ??= Date.now();
      j.attempts += 1;
      if (isHardware(j.encoderUsed)) {
        const msg = tr(
          `预检通过：${j.encoderUsed} 以当前参数试编码 3 帧成功`,
          `Pre-check passed: ${j.encoderUsed} encoded 3 test frames with these settings`,
        );
        j.events.push(event("info", msg));
      }
      j.events.push(event("info", j.firstPass ? tr("开始两遍编码", "Two-pass encoding started") : tr("开始转码", "Transcoding started")));
    }
    if (changed) this.publish();
  }

  private startTimer(): void {
    // 测试里由用例手动 tick，不起定时器
    if (this.timer || import.meta.env.MODE === "test") return;
    this.timer = setInterval(() => this.tick(0.5), 500);
  }

  private stopTimer(): void {
    clearInterval(this.timer);
    this.timer = undefined;
  }
}
