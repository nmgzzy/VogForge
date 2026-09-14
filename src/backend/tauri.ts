import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { tr } from "@/i18n";
import type {
  Capabilities,
  ImportProgress,
  ImportResult,
  JobProgressEvent,
  ProbeProgress,
  QueueSnapshot,
  Settings,
} from "@/lib/types";
import type { Backend } from "./types";

/** 与 vidforge-core 的 import::VIDEO_EXTENSIONS 保持一致 */
const VIDEO_EXTENSIONS = [
  "mp4", "m4v", "mov", "mkv", "webm", "avi", "ts", "m2ts", "mts", "mxf", "wmv", "flv", "3gp", "mpg", "mpeg", "vob",
  "hevc", "h265", "h264", "ivf",
  "rmvb", "rm", "f4v", "asf", "3g2", "ogv", "m2t", "m2v", "divx", "dv", "mod", "tod", "mk3d", "insv", "360",
];

/** 把 Tauri 异步的订阅包装成同步返回的取消函数，组件卸载早于订阅完成时也不会泄漏 */
function lazyUnlisten(start: Promise<() => void>): () => void {
  let unlisten: (() => void) | undefined;
  let cancelled = false;
  void start.then((fn) => {
    if (cancelled) fn();
    else unlisten = fn;
  });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}

const subscribe = <T,>(event: string, cb: (payload: T) => void) => lazyUnlisten(listen<T>(event, (e) => cb(e.payload)));

export const tauriBackend: Backend = {
  kind: "tauri",
  getCapabilities: (force, lang) => invoke<Capabilities>("get_capabilities", { force, lang }),
  onProbeProgress: (cb) => subscribe<ProbeProgress>("probe://progress", cb),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings) => invoke<Settings>("save_settings", { settings }),
  importMedia: (paths) => invoke<ImportResult>("import_media", { paths }),
  onImportProgress: (cb) => subscribe<ImportProgress>("import://progress", cb),
  onFileDrop: ({ over, leave, drop }) =>
    lazyUnlisten(
      getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "enter" || p.type === "over") over();
        else if (p.type === "leave") leave();
        else if (p.type === "drop") drop(p.paths);
      }),
    ),
  pickDirectory: async (title) => {
    const r = await open({ directory: true, multiple: false, title });
    return typeof r === "string" ? r : null;
  },
  pickFiles: async (title) => {
    const r = await open({ multiple: true, title, filters: [{ name: tr("视频", "Videos"), extensions: VIDEO_EXTENSIONS }] });
    return Array.isArray(r) ? r : typeof r === "string" ? [r] : [];
  },
  openUrl: (url) => openUrl(url),
  revealPath: (path) => revealItemInDir(path),
  confirm: (message, title) =>
    ask(message, { title, kind: "warning", okLabel: tr("确定", "OK"), cancelLabel: tr("取消", "Cancel") }),
  notify: async (title, body) => {
    // Windows 与 Linux 默认已授权；macOS 首次会弹系统授权
    const granted = (await isPermissionGranted()) || (await requestPermission()) === "granted";
    if (granted) sendNotification({ title, body });
  },
  ffmpegInstallDir: () => invoke<string>("ffmpeg_install_dir"),
  openFfmpegDir: () => invoke<void>("open_ffmpeg_dir"),
  trashSources: (ids) => invoke<string[]>("trash_sources", { ids }),
  getQueue: () => invoke<QueueSnapshot>("queue_snapshot"),
  queueAdd: (items) => invoke<string[]>("queue_add", { items }),
  queueControl: (op) => invoke<void>("queue_control", { op }),
  onQueueSnapshot: (cb) => subscribe<QueueSnapshot>("queue://snapshot", cb),
  onQueueProgress: (cb) => subscribe<JobProgressEvent>("queue://progress", cb),
};
