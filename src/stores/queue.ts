import { create } from "zustand";
import { backend } from "@/backend";
import type { Job, QueueItem, QueueOp, QueueSnapshot } from "@/lib/types";

/**
 * 队列的界面镜像。任务状态全部来自后端（桌面端 vidforge-core 的队列，浏览器预览是模拟队列）：
 * 结构或状态变化推完整快照、运行中推进度；这里只转发操作，不自己推进任何状态。
 */
interface QueueState {
  jobs: Job[];
  /** 全部暂停 */
  paused: boolean;
  selectedJobId?: string;
  /** 最近一次操作被后端拒绝的原因 */
  error?: string;
  /** 订阅后端事件并取一次完整状态；返回取消订阅 */
  init: () => () => void;
  enqueue: (items: QueueItem[]) => Promise<void>;
  select: (id: string) => void;
  pause: (id: string) => Promise<void>;
  resume: (id: string) => Promise<void>;
  cancel: (id: string) => Promise<void>;
  retry: (id: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  move: (id: string, delta: -1 | 1) => Promise<void>;
  setGlobalPaused: (paused: boolean) => Promise<void>;
  clearFinished: () => Promise<void>;
  dismissError: () => void;
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export const useQueue = create<QueueState>((set, get) => {
  const apply = (snap: QueueSnapshot) =>
    set((s) => ({
      jobs: snap.jobs,
      paused: snap.paused,
      // 选中的任务被移除后，改选进行中的第一个
      selectedJobId:
        s.selectedJobId && snap.jobs.some((j) => j.id === s.selectedJobId)
          ? s.selectedJobId
          : (snap.jobs.find((j) => j.status === "running") ?? snap.jobs[0])?.id,
    }));

  const run = async (op: QueueOp) => {
    try {
      await backend.queueControl(op);
      if (get().error) set({ error: undefined });
    } catch (e) {
      set({ error: message(e) });
    }
  };

  return {
    jobs: [],
    paused: false,

    init: () => {
      const offSnapshot = backend.onQueueSnapshot(apply);
      const offProgress = backend.onQueueProgress(({ id, progress }) =>
        set((s) => ({ jobs: s.jobs.map((j) => (j.id === id ? { ...j, progress } : j)) })),
      );
      void backend.getQueue().then(apply, (e) => set({ error: message(e) }));
      return () => {
        offSnapshot();
        offProgress();
      };
    },

    enqueue: async (items) => {
      try {
        await backend.queueAdd(items);
      } catch (e) {
        set({ error: message(e) });
      }
    },

    select: (selectedJobId) => set({ selectedJobId }),
    pause: (id) => run({ kind: "pause", id }),
    resume: (id) => run({ kind: "resume", id }),
    cancel: (id) => run({ kind: "cancel", id }),
    retry: (id) => run({ kind: "retry", id }),
    remove: (id) => run({ kind: "remove", id }),
    move: (id, delta) => run({ kind: "move", id, delta }),
    setGlobalPaused: (paused) => run({ kind: "set_paused", paused }),
    clearFinished: () => run({ kind: "clear_finished" }),
    dismissError: () => set({ error: undefined }),
  };
});
