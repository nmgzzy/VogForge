import { create } from "zustand";
import type { Capabilities } from "@/lib/types";
import { MOCK_CAPABILITIES } from "@/mock/capabilities";

interface CapabilityState {
  caps: Capabilities;
  probing: boolean;
  reprobe: () => void;
}

export const useCapabilities = create<CapabilityState>((set) => ({
  caps: MOCK_CAPABILITIES,
  probing: false,
  // 浏览器预览下模拟一次探测过程；接入后端后改为调用 Tauri command
  reprobe: () => {
    set({ probing: true });
    window.setTimeout(() => {
      set({ probing: false, caps: { ...MOCK_CAPABILITIES, probedAt: new Date().toISOString() } });
    }, 1400);
  },
}));
