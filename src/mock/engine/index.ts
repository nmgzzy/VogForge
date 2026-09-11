/**
 * 前端 mock 引擎。
 *
 * 仅用于浏览器预览与界面开发阶段。真实的决策逻辑在 crates/vidforge-core，
 * 接入 Tauri 后由后端提供同名能力，前端不再做任何编码决策（见 docs/design.md 6.5）。
 * 这里的规则与设计文档保持一致，同时充当 Rust 实现的对照原型。
 */
import type { Capabilities, MediaInfo, PlanResult, Scenario, TranscodePlan } from "@/lib/types";
import { formatBitrate } from "@/lib/format";
import { buildArgSegments, flattenArgs } from "./args";
import { estimate } from "./estimate";
import { explain } from "./explain";
import { applyFix as applyFixRaw, resolveFidelity } from "./fidelity";
import { computeFpsInsight } from "./fps";
import { normalizePlan, recommend } from "./recommend";

export { SCENARIOS, suggestScenario } from "./recommend";
export { FIDELITY_META } from "./fidelity";
export { buildArgSegments } from "./args";

export function recommendPlan(media: MediaInfo, scenario: Scenario, caps: Capabilities): TranscodePlan {
  return recommend(media, scenario, caps);
}

export function updatePlan(plan: TranscodePlan, media: MediaInfo, caps: Capabilities): TranscodePlan {
  return normalizePlan(plan, media, caps);
}

export function applyFix(plan: TranscodePlan, fixId: string, media: MediaInfo, caps: Capabilities): TranscodePlan {
  return normalizePlan(applyFixRaw(plan, fixId, media), media, caps);
}

/** 重编码后能省下的比例低于这个值，就提示"不建议转码" */
const WORTH_IT_THRESHOLD = 0.25;

export function evaluate(media: MediaInfo, plan: TranscodePlan, caps: Capabilities): PlanResult {
  const v = media.video[0];
  const fpsInsight =
    v && plan.video.action === "encode" && plan.video.fps.kind === "cfr"
      ? computeFpsInsight(v, media.durationSec, plan.video.fps.fps)
      : undefined;

  const est = estimate(media, plan);
  let notWorthIt: string | undefined;
  const saving = 1 - est.videoBps / est.sourceVideoBps;
  if (plan.video.action === "encode" && plan.scenario !== "editing" && saving < WORTH_IT_THRESHOLD) {
    notWorthIt =
      `源视频只有 ${formatBitrate(est.sourceVideoBps)}，已经高度压缩。按当前设置重新编码` +
      `${saving > 0.02 ? `只能省下约 ${Math.round(saving * 100)}%` : "几乎省不下空间"}，还会叠加一次画质损失。` +
      `建议改为「原样封装」，或调低画质档位。`;
  }

  return {
    plan,
    decisions: explain(media, plan, caps, fpsInsight, est.estimate),
    fidelity: resolveFidelity(media, plan, caps),
    args: flattenArgs(buildArgSegments(media, plan, caps)),
    estimate: est.estimate,
    fpsInsight,
    notWorthIt,
  };
}
