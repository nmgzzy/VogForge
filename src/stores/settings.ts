import { create } from "zustand";
import { backend } from "@/backend";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import type { Settings } from "@/lib/types";

interface SettingsState {
  settings: Settings;
  loaded: boolean;
  error?: string;
  load: () => Promise<void>;
  /** 修改并保存；后端会把越界值拉回合理范围，以返回值为准 */
  update: (patch: Partial<Settings>) => Promise<Settings>;
}

export const useSettings = create<SettingsState>((set, get) => ({
  settings: DEFAULT_SETTINGS,
  loaded: false,
  load: async () => {
    try {
      set({ settings: await backend.getSettings(), loaded: true, error: undefined });
    } catch (e) {
      set({ loaded: true, error: e instanceof Error ? e.message : String(e) });
    }
  },
  update: async (patch) => {
    const next = { ...get().settings, ...patch };
    // 先乐观更新，保存失败再回滚
    const prev = get().settings;
    set({ settings: next });
    try {
      const saved = await backend.saveSettings(next);
      set({ settings: saved, error: undefined });
      return saved;
    } catch (e) {
      set({ settings: prev, error: e instanceof Error ? e.message : String(e) });
      return prev;
    }
  },
}));
