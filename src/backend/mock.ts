import { DEFAULT_SETTINGS } from "@/lib/defaults";
import type { ImportProgress, ImportResult, ProbeProgress, Settings } from "@/lib/types";
import { MOCK_CAPABILITIES } from "@/mock/capabilities";
import { MOCK_MEDIA } from "@/mock/media";
import { seedJobs } from "@/mock/queue";
import { MockQueue } from "./mock-queue";
import type { Backend } from "./types";

const probeListeners = new Set<(p: ProbeProgress) => void>();
const importListeners = new Set<(p: ImportProgress) => void>();
let settings: Settings = { ...DEFAULT_SETTINGS };

/** 浏览器预览的模拟队列；测试里直接调它的 tick 推进 */
export const mockQueue = new MockQueue(() => settings, seedJobs);

const STAGES = ["检测编译能力", "初始化硬件设备", "试编码", "检测色调映射"];
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

function listen<T>(set: Set<(p: T) => void>, cb: (p: T) => void): () => void {
  set.add(cb);
  return () => set.delete(cb);
}

/**
 * 浏览器预览：强制探测时模拟约 1.4 秒的四个阶段，否则当作命中缓存立即返回。
 * 导入只认得示例素材的路径（浏览器拿不到本地文件路径），路径含 broken 的模拟损坏文件。
 */
export const mockBackend: Backend = {
  kind: "mock",
  getCapabilities: async (force) => {
    const stages = force ? STAGES : ["读取缓存"];
    for (let i = 0; i < stages.length; i++) {
      probeListeners.forEach((cb) => cb({ stage: stages[i]!, done: i, total: stages.length }));
      if (force) await sleep(350);
    }
    return { ...MOCK_CAPABILITIES, probedAt: new Date().toISOString() };
  },
  onProbeProgress: (cb) => listen(probeListeners, cb),
  getSettings: async () => ({ ...settings }),
  saveSettings: async (s) => {
    settings = { ...s };
    return { ...settings };
  },
  importMedia: async (paths) => {
    const result: ImportResult = { media: [], failures: [], skipped: 0 };
    paths.forEach((path, i) => {
      const sample = MOCK_MEDIA.find((m) => m.path === path || m.name === path);
      if (sample) result.media.push(sample);
      else if (path.includes("broken")) result.failures.push({ path, reason: "文件不完整或已损坏（moov atom not found）" });
      else result.failures.push({ path, reason: "浏览器预览无法读取本地文件" });
      importListeners.forEach((cb) => cb({ done: i + 1, total: paths.length, current: path.split(/[\\/]/).pop() ?? path }));
    });
    return result;
  },
  onImportProgress: (cb) => listen(importListeners, cb),
  onFileDrop: () => () => undefined,
  pickDirectory: async () => null,
  pickFiles: async () => [],
  openUrl: async (url) => {
    window.open(url, "_blank", "noopener");
  },
  revealPath: async () => undefined,
  confirm: async (message) => window.confirm(message),
  getQueue: async () => mockQueue.snapshot(),
  queueAdd: async (items) => mockQueue.add(items),
  queueControl: async (op) => mockQueue.control(op),
  onQueueSnapshot: (cb) => mockQueue.onSnapshot(cb),
  onQueueProgress: (cb) => mockQueue.onProgress(cb),
};
