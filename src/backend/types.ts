import type { Capabilities, ProbeProgress, Settings } from "@/lib/types";

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

  /** 系统目录选择框；取消时返回 null */
  pickDirectory(title: string): Promise<string | null>;
  /** 用系统浏览器打开链接 */
  openUrl(url: string): Promise<void>;
}
