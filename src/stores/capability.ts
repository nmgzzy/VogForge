import { create } from "zustand";
import { backend } from "@/backend";
import { pendingCapabilities } from "@/lib/defaults";
import type { Capabilities, ProbeProgress } from "@/lib/types";
import { MOCK_CAPABILITIES } from "@/mock/capabilities";
import { useSettings } from "./settings";

interface CapabilityState {
  caps: Capabilities;
  probing: boolean;
  progress?: ProbeProgress;
  /** 调用后端本身失败（不是"没找到 ffmpeg"，那种情况在 caps.status 里） */
  error?: string;
  /** 启动时调用：优先用后端缓存 */
  load: () => Promise<void>;
  /** 忽略缓存重新探测 */
  reprobe: () => Promise<void>;
}

async function run(set: (s: Partial<CapabilityState>) => void, force: boolean) {
  set({ probing: true, error: undefined, progress: undefined });
  const off = backend.onProbeProgress((progress) => set({ progress }));
  const lang = useSettings.getState().settings.language;
  try {
    const caps = await backend.getCapabilities(force, lang);
    set({ caps, probing: false, progress: undefined });
  } catch (e) {
    set({ probing: false, progress: undefined, error: e instanceof Error ? e.message : String(e) });
  } finally {
    off();
  }
  // 探测期间换了界面语言：结果的说明还是旧语言，再取一次（命中缓存，很快）
  if (useSettings.getState().settings.language !== lang) await run(set, false);
}

export const useCapabilities = create<CapabilityState>((set, get) => ({
  // 浏览器预览直接给出演示数据；桌面应用先用"探测中"占位，探测完成前界面按软编给方案
  caps: backend.kind === "mock" ? MOCK_CAPABILITIES : pendingCapabilities(),
  probing: false,
  load: async () => {
    if (get().probing) return;
    await run(set, false);
  },
  reprobe: async () => {
    if (get().probing) return;
    await run(set, true);
  },
}));
