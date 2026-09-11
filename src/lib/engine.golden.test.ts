/**
 * 提交的 wasm 包（src/wasm/pkg）必须与 Rust 源码同步：改了引擎规则却忘了 `pnpm wasm`，界面会跑旧逻辑。
 * 回归样本由 Rust 测试维护（golden_engine.rs），这里用 wasm 重算样本里与输出路径无关的部分逐项核对。
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import type { Capabilities, MediaInfo, PlanResult, Scenario, TranscodePlan } from "./types";
import { applyFix, evaluate, recommendPlan } from "./engine";

interface Case {
  name: string;
  caps: string;
  media: MediaInfo;
  plan: TranscodePlan;
  scenario?: Scenario;
  fixFrom?: { plan: TranscodePlan; id: string };
  result: PlanResult;
}

const golden = JSON.parse(
  readFileSync(resolve("crates/vidforge-core/tests/fixtures/golden/engine.json"), "utf8"),
) as { caps: Record<string, Capabilities>; cases: Case[] };

/** 结构逐项比较，浮点按相对误差 1e-9 */
function diff(path: string, want: unknown, got: unknown, out: string[]): void {
  if (typeof want === "number" && typeof got === "number") {
    if (Math.abs(want - got) > 1e-9 * Math.max(Math.abs(want), Math.abs(got), 1)) out.push(`${path}: ${want} ≠ ${got}`);
  } else if (Array.isArray(want) && Array.isArray(got)) {
    if (want.length !== got.length) out.push(`${path}: 长度 ${want.length} ≠ ${got.length}`);
    want.forEach((w, i) => diff(`${path}[${i}]`, w, got[i], out));
  } else if (want && got && typeof want === "object" && typeof got === "object") {
    const keys = new Set([...Object.keys(want), ...Object.keys(got)]);
    for (const k of keys) diff(`${path}.${k}`, (want as Record<string, unknown>)[k], (got as Record<string, unknown>)[k], out);
  } else if (want !== got) {
    out.push(`${path}: ${JSON.stringify(want)} ≠ ${JSON.stringify(got)}`);
  }
}

describe("wasm 包与 Rust 回归样本一致", () => {
  it("推荐、一键修正与评估结果（不含输出路径）逐项相同", () => {
    const out: string[] = [];
    for (const c of golden.cases) {
      const caps = golden.caps[c.caps]!;
      const plan = c.scenario
        ? recommendPlan(c.media, c.scenario, caps)
        : c.fixFrom
          ? applyFix(c.fixFrom.plan, c.fixFrom.id, c.media, caps)
          : c.plan;
      diff(`${c.name} plan`, c.plan, plan, out);
      const r = evaluate(c.media, plan, caps);
      for (const k of ["decisions", "fidelity", "estimate", "fpsInsight", "notWorthIt"] as const) {
        diff(`${c.name} ${k}`, c.result[k], r[k], out);
      }
    }
    expect(golden.cases.length).toBeGreaterThan(150);
    expect(out.slice(0, 20), "wasm 包过期？运行 pnpm wasm 重新生成").toEqual([]);
  });
});
