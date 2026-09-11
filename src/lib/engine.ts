/**
 * 决策引擎。实现只有一份：crates/vidforge-core，编译成 WebAssembly 后在前端同步调用。
 * 桌面应用与浏览器预览、组件测试走的是同一段 Rust 代码（见 docs/design.md 6.5）。
 *
 * 使用前必须先初始化：应用启动时 `await initEngine()`，测试里 `initEngineSync(bytes)`。
 */
import init, * as wasm from "@/wasm/pkg/vidforge_wasm";
import wasmUrl from "@/wasm/pkg/vidforge_wasm_bg.wasm?url";
import type {
  Capabilities,
  EncoderId,
  EncoderMeta,
  EngineMeta,
  MediaInfo,
  PlanResult,
  Scenario,
  Settings,
  TranscodePlan,
  VideoHints,
} from "./types";
import { DEFAULT_SETTINGS } from "./defaults";

let ready = false;

export async function initEngine(): Promise<void> {
  if (ready) return;
  await init({ module_or_path: wasmUrl });
  ready = true;
}

export function initEngineSync(module: BufferSource | WebAssembly.Module): void {
  if (ready) return;
  wasm.initSync({ module });
  ready = true;
}

const j = JSON.stringify;

function call<T>(f: () => string): T {
  if (!ready) throw new Error("决策引擎尚未初始化（initEngine）");
  return JSON.parse(f()) as T;
}

/** 本地日期 YYYY-MM-DD，用于命名模板的 {date} */
export function today(d = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export const suggestScenario = (media: MediaInfo): Scenario => call(() => wasm.suggest_scenario(j(media)));

export const recommendPlan = (media: MediaInfo, scenario: Scenario, caps: Capabilities): TranscodePlan =>
  call(() => wasm.recommend_plan(j(media), scenario, j(caps)));

/** 用户改了参数后让计划重新自洽（换编码器、重建音轨、拉回越界值……） */
export const updatePlan = (plan: TranscodePlan, media: MediaInfo, caps: Capabilities): TranscodePlan =>
  call(() => wasm.update_plan(j(plan), j(media), j(caps)));

export const applyFix = (plan: TranscodePlan, fixId: string, media: MediaInfo, caps: Capabilities): TranscodePlan =>
  call(() => wasm.apply_fix(j(plan), fixId, j(media), j(caps)));

/** 派生界面需要的全部内容：理由、保真度、命令、预估。输出路径按设置里的目录与命名模板计算 */
export const evaluate = (
  media: MediaInfo,
  plan: TranscodePlan,
  caps: Capabilities,
  settings: Settings = DEFAULT_SETTINGS,
  date = today(),
): PlanResult => call(() => wasm.evaluate(j(media), j(plan), j(caps), j(settings), date));

export const videoHints = (media: MediaInfo): VideoHints | null => call(() => wasm.video_hints(j(media)));

let meta: EngineMeta | undefined;
let byId: Map<EncoderId, EncoderMeta> | undefined;

/** 静态规则表，只取一次 */
export function engineMeta(): EngineMeta {
  meta ??= call<EngineMeta>(() => wasm.engine_meta());
  return meta;
}

export function encoderMeta(id: EncoderId): EncoderMeta {
  byId ??= new Map(engineMeta().encoders.map((e) => [e.id, e]));
  const m = byId.get(id);
  if (!m) throw new Error(`未知编码器 ${id}`);
  return m;
}
