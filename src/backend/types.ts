import type { Capabilities, ImportProgress, ImportResult, ProbeProgress, Settings } from "@/lib/types";

/** 拖放到窗口上的文件：进入、离开与松开 */
export interface DropHandlers {
  over: () => void;
  leave: () => void;
  drop: (paths: string[]) => void;
}

/**
 * 前端与后端之间的全部交互。桌面应用里由 Tauri 命令实现；
 * 浏览器预览与组件测试里由 mock 实现（见 ./mock.ts）。
 */
export interface Backend {
  readonly kind: "tauri" | "mock";

  /** 探测环境能力。force 为 false 时后端优先用缓存 */
  getCapabilities(force: boolean): Promise<Capabilities>;
  /** 订阅探测进度，返回取消订阅函数 */
  onProbeProgress(cb: (p: ProbeProgress) => void): () => void;

  getSettings(): Promise<Settings>;
  saveSettings(settings: Settings): Promise<Settings>;

  /** 分析文件与文件夹（文件夹递归扫描） */
  importMedia(paths: string[]): Promise<ImportResult>;
  onImportProgress(cb: (p: ImportProgress) => void): () => void;
  /** 订阅窗口上的文件拖放。浏览器预览拿不到本地路径，返回空操作 */
  onFileDrop(handlers: DropHandlers): () => void;

  /** 系统目录选择框；取消时返回 null */
  pickDirectory(title: string): Promise<string | null>;
  /** 系统文件选择框（可多选视频）；取消时返回空数组 */
  pickFiles(title: string): Promise<string[]>;
  /** 用系统浏览器打开链接 */
  openUrl(url: string): Promise<void>;
}
