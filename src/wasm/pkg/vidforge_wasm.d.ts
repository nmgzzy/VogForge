/* tslint:disable */
/* eslint-disable */

export function apply_fix(plan: string, fix_id: string, media: string, caps: string): string;

/**
 * 决策引擎实际使用的能力：按设置关掉硬件编码 / 硬件解码（需求 F-5.6）
 */
export function effective_caps(caps: string, settings: string): string;

/**
 * 界面需要的静态规则表（质量刻度、preset、码率控制支持、标准帧率档）
 */
export function engine_meta(): string;

/**
 * 派生界面所需的全部内容；输出路径按设置计算
 */
export function evaluate(media: string, plan: string, caps: string, settings: string, date: string): string;

export function recommend_plan(media: string, scenario_id: string, caps: string): string;

/**
 * 按素材特征推荐的起始场景
 */
export function suggest_scenario(media: string): string;

export function update_plan(plan: string, media: string, caps: string): string;

/**
 * 帧率控件的建议（推荐 CFR 目标、是否极端可变帧率）；没有视频流时返回 "null"
 */
export function video_hints(media: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly apply_fix: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number, number, number];
    readonly effective_caps: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly engine_meta: () => [number, number, number, number];
    readonly evaluate: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number) => [number, number, number, number];
    readonly recommend_plan: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number, number];
    readonly suggest_scenario: (a: number, b: number) => [number, number, number, number];
    readonly update_plan: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number, number];
    readonly video_hints: (a: number, b: number) => [number, number, number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
