import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Capabilities, ProbeProgress, Settings } from "@/lib/types";
import type { Backend } from "./types";

/** 把 Tauri 异步的 listen 包装成同步返回的取消函数，组件卸载早于订阅完成时也不会泄漏 */
function subscribe<T>(event: string, cb: (payload: T) => void): () => void {
  let unlisten: (() => void) | undefined;
  let cancelled = false;
  void listen<T>(event, (e) => cb(e.payload)).then((fn) => {
    if (cancelled) fn();
    else unlisten = fn;
  });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}

export const tauriBackend: Backend = {
  kind: "tauri",
  getCapabilities: (force) => invoke<Capabilities>("get_capabilities", { force }),
  onProbeProgress: (cb) => subscribe<ProbeProgress>("probe://progress", cb),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings) => invoke<Settings>("save_settings", { settings }),
  pickDirectory: async (title) => {
    const r = await open({ directory: true, multiple: false, title });
    return typeof r === "string" ? r : null;
  },
  openUrl: (url) => openUrl(url),
};
