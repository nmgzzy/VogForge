import { useMemo } from "react";
import { effectiveCaps } from "@/lib/engine";
import type { Capabilities } from "@/lib/types";
import { useCapabilities } from "./capability";
import { useSettings } from "./settings";

let memo: { caps: Capabilities; hw: string; out: Capabilities } | undefined;

/**
 * 决策引擎用的能力：探测结果按设置里的硬件编码 / 硬件解码开关收紧（与后端执行时一致）。
 * 环境页展示的仍是原始探测结果
 */
export function engineCaps(): Capabilities {
  const caps = useCapabilities.getState().caps;
  const settings = useSettings.getState().settings;
  // 不可用原因的文字随语言变化，语言也算进缓存键
  const hw = `${settings.hwEncode}/${settings.hwDecode}/${settings.language}`;
  if (memo?.caps !== caps || memo.hw !== hw) memo = { caps, hw, out: effectiveCaps(caps, settings) };
  return memo.out;
}

export function useEngineCaps(): Capabilities {
  const caps = useCapabilities((s) => s.caps);
  const hwEncode = useSettings((s) => s.settings.hwEncode);
  const hwDecode = useSettings((s) => s.settings.hwDecode);
  const language = useSettings((s) => s.settings.language);
  // 依赖项就是 engineCaps 读取的全部状态
  return useMemo(() => engineCaps(), [caps, hwEncode, hwDecode, language]);
}
