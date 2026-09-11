import type {
  Capabilities,
  ImportProgress,
  ImportResult,
  JobProgressEvent,
  Lang,
  ProbeProgress,
  QueueItem,
  QueueOp,
  QueueSnapshot,
  Settings,
} from "@/lib/types";

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

  /** 探测环境能力。force 为 false 时后端优先用缓存；说明文字按 lang 生成（切换语言时不必等设置保存完） */
  getCapabilities(force: boolean, lang: Lang): Promise<Capabilities>;
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
  /** 在文件管理器里定位这个文件 */
  revealPath(path: string): Promise<void>;
  /** 需要用户明确同意的操作（例如改为覆盖同名文件）；返回是否同意 */
  confirm(message: string, title: string): Promise<boolean>;
  /** 系统通知（需求 F-6.11） */
  notify(title: string, body: string): Promise<void>;

  /** 应用自己的 ffmpeg 目录（引导下载后放这里）；浏览器预览没有，返回 null */
  ffmpegInstallDir(): Promise<string | null>;
  /** 在文件管理器里打开这个目录 */
  openFfmpegDir(): Promise<void>;
  /** 把这些已完成任务的源文件移到回收站；后端只处理校验通过的任务，返回实际移走的文件 */
  trashSources(jobIds: string[]): Promise<string[]>;

  /** 队列的完整状态；之后的变化通过 onQueueSnapshot / onQueueProgress 推送 */
  getQueue(): Promise<QueueSnapshot>;
  /** 加入队列，返回新任务的 id。后端按计划重新生成命令，不使用界面上的预览命令 */
  queueAdd(items: QueueItem[]): Promise<string[]>;
  /** 暂停、继续、取消、重试、移除、换序、全部暂停、清除已完成；不合法的操作抛出原因 */
  queueControl(op: QueueOp): Promise<void>;
  /** 结构或状态变化：整体替换 */
  onQueueSnapshot(cb: (s: QueueSnapshot) => void): () => void;
  /** 运行中的进度 */
  onQueueProgress(cb: (e: JobProgressEvent) => void): () => void;
}
