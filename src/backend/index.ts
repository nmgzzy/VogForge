import { isTauri } from "@tauri-apps/api/core";
import { mockBackend } from "./mock";
import { tauriBackend } from "./tauri";
import type { Backend } from "./types";

export type { Backend } from "./types";

/** 运行在 Tauri 窗口里用真实后端，浏览器预览与测试用 mock */
export const backend: Backend = isTauri() ? tauriBackend : mockBackend;
