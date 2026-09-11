import { create } from "zustand";
import { backend } from "@/backend";
import type { Job, JobEvent, MediaInfo, TranscodePlan } from "@/lib/types";
import { formatBytes } from "@/lib/format";
import { isHardware } from "@/lib/encoders";
import { buildReport, estimatedOutputSize, makeJob, seedJobs, simulatedSpeed } from "@/mock/queue";
import { useSettings } from "./settings";

/** 演示加速倍数：让进度条在预览里肉眼可见地推进。真实进度来自 ffmpeg */
const DEMO_ACCEL = 25;

type Ticket = "cpu" | "gpu";
const ticketOf = (j: Job): Ticket => (isHardware(j.encoderUsed) ? "gpu" : "cpu");

interface QueueState {
  jobs: Job[];
  selectedJobId?: string;
  concurrency: Record<Ticket, number>;
  globalPaused: boolean;

  enqueue: (items: { media: MediaInfo; plan: TranscodePlan }[]) => void;
  select: (id: string) => void;
  pause: (id: string) => void;
  resume: (id: string) => void;
  cancel: (id: string) => void;
  retry: (id: string) => void;
  remove: (id: string) => void;
  move: (id: string, delta: -1 | 1) => void;
  setGlobalPaused: (p: boolean) => void;
  setConcurrency: (t: Ticket, n: number) => void;
  clearFinished: () => void;
  tick: (dtSec: number) => void;
}

const event = (level: JobEvent["level"], message: string): JobEvent => ({ at: Date.now(), level, message });

function patch(jobs: Job[], id: string, fn: (j: Job) => Job): Job[] {
  return jobs.map((j) => (j.id === id ? fn(j) : j));
}

export const useQueue = create<QueueState>((set, get) => ({
  // 浏览器预览放几条演示任务；桌面应用从空队列开始
  jobs: backend.kind === "mock" ? seedJobs() : [],
  selectedJobId: backend.kind === "mock" ? "job-run-cpu" : undefined,
  concurrency: { cpu: 1, gpu: 1 },
  globalPaused: false,

  enqueue: (items) =>
    set((s) => {
      const settings = useSettings.getState().settings;
      const fresh = items.map(({ media, plan }) => makeJob(media, plan, settings));
      return { jobs: [...s.jobs, ...fresh], selectedJobId: s.selectedJobId ?? fresh[0]?.id };
    }),

  select: (selectedJobId) => set({ selectedJobId }),

  pause: (id) =>
    set((s) => ({
      jobs: patch(s.jobs, id, (j) =>
        j.status === "running" ? { ...j, status: "paused", events: [...j.events, event("info", "已暂停")] } : j,
      ),
    })),

  resume: (id) =>
    set((s) => ({
      jobs: patch(s.jobs, id, (j) =>
        j.status === "paused" ? { ...j, status: "running", events: [...j.events, event("info", "已继续")] } : j,
      ),
    })),

  cancel: (id) =>
    set((s) => ({
      jobs: patch(s.jobs, id, (j) =>
        j.status === "running" || j.status === "paused" || j.status === "queued"
          ? {
              ...j,
              status: "cancelled",
              finishedAt: Date.now(),
              events: [...j.events, event("info", "已取消，临时文件已删除，目标目录无残留")],
            }
          : j,
      ),
    })),

  retry: (id) =>
    set((s) => ({
      jobs: patch(s.jobs, id, (j) => ({
        ...j,
        status: "queued",
        progress: { percent: 0, outTimeSec: 0, speed: 0, fps: 0, sizeBytes: 0, dupFrames: 0, dropFrames: 0 },
        report: undefined,
        finishedAt: undefined,
        events: [...j.events, event("info", "重新加入队列")],
      })),
    })),

  remove: (id) =>
    set((s) => {
      const jobs = s.jobs.filter((j) => j.id !== id);
      return { jobs, selectedJobId: s.selectedJobId === id ? jobs[0]?.id : s.selectedJobId };
    }),

  move: (id, delta) =>
    set((s) => {
      const i = s.jobs.findIndex((j) => j.id === id);
      const k = i + delta;
      if (i < 0 || k < 0 || k >= s.jobs.length) return s;
      const jobs = [...s.jobs];
      [jobs[i], jobs[k]] = [jobs[k]!, jobs[i]!];
      return { jobs };
    }),

  setGlobalPaused: (globalPaused) => set({ globalPaused }),
  setConcurrency: (t, n) => set((s) => ({ concurrency: { ...s.concurrency, [t]: Math.max(1, Math.min(4, n)) } })),
  clearFinished: () => set((s) => ({ jobs: s.jobs.filter((j) => j.status !== "done" && j.status !== "cancelled") })),

  tick: (dt) => {
    const { jobs, concurrency, globalPaused } = get();
    if (globalPaused) return;
    let changed = false;

    let next = jobs.map((j) => {
      if (j.status !== "running") return j;
      changed = true;
      const base = simulatedSpeed(j);
      const speed = base * (0.94 + Math.random() * 0.12);
      const dur = j.media.durationSec;
      const out = Math.min(dur, j.progress.outTimeSec + speed * dt * DEMO_ACCEL);
      const percent = (out / dur) * 100;
      const srcFps = j.media.video[0]?.fpsAvg ?? 30;
      const cfr = j.plan.video.fps.kind === "cfr" ? j.plan.video.fps.fps : undefined;
      const outFps = cfr ?? srcFps;
      const dup = cfr && j.media.video[0]?.isVfr ? Math.round(Math.max(0, (cfr - srcFps) * out)) : 0;
      const size = estimatedOutputSize(j) * (percent / 100);

      if (out >= dur) {
        const base: Job = {
          ...j,
          status: "done",
          finishedAt: Date.now(),
          outputSize: size,
          progress: { ...j.progress, percent: 100, outTimeSec: dur, sizeBytes: size, etaSec: 0 },
        };
        const report = buildReport(base);
        const failed = report.filter((r) => !r.ok);
        const sizes = `${formatBytes(j.media.sizeBytes)} → ${formatBytes(size)}`;
        return {
          ...base,
          report,
          events: [
            ...j.events,
            event("info", "编码完成，正在校验输出"),
            failed.length
              ? event("warn", `已改名为最终文件（${sizes}），但 ${failed.length} 项未按要求保留：${failed.map((r) => r.label).join("、")}`)
              : event("info", `校验通过，已改名为最终文件。${sizes}`),
          ],
        };
      }
      return {
        ...j,
        progress: {
          percent,
          outTimeSec: out,
          speed,
          fps: speed * outFps,
          sizeBytes: size,
          // 真实 ETA 按编码器实际速度计算；前 5 秒不显示（speed 尚不可信）
          etaSec: out > 5 ? (dur - out) / speed : undefined,
          dupFrames: dup,
          dropFrames: 0,
        },
      };
    });

    // 调度：按票据填充空闲槽位。暂停只是挂起 ffmpeg 进程，内存与 GPU 会话仍被占用，所以同样占票
    const holding = (t: Ticket) =>
      next.filter((j) => (j.status === "running" || j.status === "paused") && ticketOf(j) === t).length;
    for (const j of next) {
      if (j.status !== "queued") continue;
      const t = ticketOf(j);
      if (holding(t) >= concurrency[t]) continue;
      changed = true;
      next = patch(next, j.id, (x) => ({
        ...x,
        status: "running",
        startedAt: Date.now(),
        events: [
          ...x.events,
          event("info", `预检通过：${x.encoderUsed} 以当前参数试编码 3 帧成功`),
          event("info", "开始转码"),
        ],
      }));
    }

    if (changed) set({ jobs: next });
  },
}));
