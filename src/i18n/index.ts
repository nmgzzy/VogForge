/**
 * 界面语言（需求 F-9.1）：中文为主，可切换英文。
 *
 * 文案写在调用处：`tr("中文", "English")`，不维护键值表，改一处就能看到两种语言的上下文。
 * 当前语言由 App 在渲染时按设置同步；语言变化时界面整体以新 key 重新挂载，所以渲染期直接调用即可。
 * 引擎、队列与环境探测的说明由 Rust 按同一个设置生成，这里只管前端自己的文字。
 */
import type { Lang } from "@/lib/types";

let current: Lang = "zh-CN";

export function setLang(lang: Lang) {
  current = lang;
  if (typeof document !== "undefined") document.documentElement.lang = lang;
}

export const getLang = (): Lang => current;

export const isEnglish = (): boolean => current === "en";

/** 按界面语言二选一 */
export function tr(zh: string, en: string): string {
  return current === "en" ? en : zh;
}

/** 英文按数量选单复数：`plural(n, "track", "tracks")` */
export function plural(n: number, one: string, many: string): string {
  return n === 1 ? one : many;
}
