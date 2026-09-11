import { tr } from "@/i18n";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { localizeCaps } from "@/lib/engine";
import type { ImportProgress, ImportResult, ProbeProgress, Settings } from "@/lib/types";
import { MOCK_CAPABILITIES } from "@/mock/capabilities";
import { MOCK_MEDIA } from "@/mock/media";
import { seedJobs } from "@/mock/queue";
import { MockQueue } from "./mock-queue";
import type { Backend } from "./types";

const probeListeners = new Set<(p: ProbeProgress) => void>();
const importListeners = new Set<(p: ImportProgress) => void>();
let settings: Settings = { ...DEFAULT_SETTINGS };

/** 浏览器预览不发系统通知、不动文件，只记下来供测试检查 */
export const notifications: { title: string; body: string }[] = [];
export const trashed: string[] = [];

/** 浏览器预览的模拟队列；测试里直接调它的 tick 推进 */
export const mockQueue = new MockQueue(() => settings, seedJobs);

const stages = () => [
  tr("检测编译能力", "Checking build features"),
  tr("初始化硬件设备", "Initializing hardware devices"),
  tr("试编码", "Test encoding"),
  tr("检测色调映射", "Checking tone mapping"),
];
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
  getCapabilities: async (force, lang) => {
    const list = force ? stages() : [tr("读取缓存", "Reading cache")];
    for (let i = 0; i < list.length; i++) {
      probeListeners.forEach((cb) => cb({ stage: list[i]!, done: i, total: list.length }));
      if (force) await sleep(350);
    }
    // 示例能力里的说明是中文原文，与后端缓存一样按语言换
    return localizeCaps({ ...MOCK_CAPABILITIES, probedAt: new Date().toISOString() }, lang);
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
      else if (path.includes("broken")) {
        const reason = tr(
          "文件不完整或已损坏。常见于拍摄中断或复制没有完成。用原设备重新导出，或用 untrunc 之类的工具修复",
          "The file is incomplete or damaged. This usually comes from an interrupted recording or copy. Export it again or repair it with a tool like untrunc",
        );
        result.failures.push({ path, reason, detail: "[mov,mp4 @ 0x1] moov atom not found" });
      } else result.failures.push({ path, reason: tr("浏览器预览无法读取本地文件", "The browser preview cannot read local files") });
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
  notify: async (title, body) => {
    notifications.push({ title, body });
  },
  ffmpegInstallDir: async () => null,
  openFfmpegDir: async () => undefined,
  trashSources: async (ids) => {
    trashed.push(...ids);
    return ids;
  },
  getQueue: async () => mockQueue.snapshot(),
  queueAdd: async (items) => mockQueue.add(items),
  queueControl: async (op) => mockQueue.control(op),
  onQueueSnapshot: (cb) => mockQueue.onSnapshot(cb),
  onQueueProgress: (cb) => mockQueue.onProgress(cb),
};
