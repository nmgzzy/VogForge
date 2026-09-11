import { DEFAULT_SETTINGS } from "@/lib/defaults";
import type { ProbeProgress, Settings } from "@/lib/types";
import { MOCK_CAPABILITIES } from "@/mock/capabilities";
import type { Backend } from "./types";

const progressListeners = new Set<(p: ProbeProgress) => void>();
let settings: Settings = { ...DEFAULT_SETTINGS };

const STAGES = ["检测编译能力", "初始化硬件设备", "试编码", "检测色调映射"];
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** 浏览器预览：强制探测时模拟约 1.4 秒的四个阶段，否则当作命中缓存立即返回 */
export const mockBackend: Backend = {
  kind: "mock",
  getCapabilities: async (force) => {
    const stages = force ? STAGES : ["读取缓存"];
    for (let i = 0; i < stages.length; i++) {
      progressListeners.forEach((cb) => cb({ stage: stages[i]!, done: i, total: stages.length }));
      if (force) await sleep(350);
    }
    return { ...MOCK_CAPABILITIES, probedAt: new Date().toISOString() };
  },
  onProbeProgress: (cb) => {
    progressListeners.add(cb);
    return () => progressListeners.delete(cb);
  },
  getSettings: async () => ({ ...settings }),
  saveSettings: async (s) => {
    settings = { ...s };
    return { ...settings };
  },
  pickDirectory: async () => null,
  openUrl: async (url) => {
    window.open(url, "_blank", "noopener");
  },
};
