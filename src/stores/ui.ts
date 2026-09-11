import { create } from "zustand";

export type View = "transcode" | "queue" | "environment" | "presets" | "settings";
export type ThemePref = "system" | "light" | "dark";

interface UiState {
  view: View;
  theme: ThemePref;
  commandExpanded: boolean;
  detailsOpen: boolean;
  setView: (v: View) => void;
  setTheme: (t: ThemePref) => void;
  toggleCommand: () => void;
  setDetailsOpen: (open: boolean) => void;
}

const THEME_KEY = "vidforge.theme";

function readTheme(): ThemePref {
  try {
    const t = localStorage.getItem(THEME_KEY);
    return t === "light" || t === "dark" || t === "system" ? t : "system";
  } catch {
    return "system";
  }
}

export function resolveTheme(pref: ThemePref): "light" | "dark" {
  if (pref !== "system") return pref;
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function applyTheme(pref: ThemePref): void {
  document.documentElement.dataset.theme = resolveTheme(pref);
}

export const useUi = create<UiState>((set) => ({
  view: "transcode",
  theme: readTheme(),
  commandExpanded: false,
  detailsOpen: false,
  setView: (view) => set({ view }),
  setTheme: (theme) => {
    try {
      localStorage.setItem(THEME_KEY, theme);
    } catch {
      /* 隐私模式等场景下存储不可用，主题仅在本次会话生效 */
    }
    applyTheme(theme);
    set({ theme });
  },
  toggleCommand: () => set((s) => ({ commandExpanded: !s.commandExpanded })),
  setDetailsOpen: (detailsOpen) => set({ detailsOpen }),
}));
