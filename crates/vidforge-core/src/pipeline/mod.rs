//! 转码决策与命令构建。全部是纯函数，环境信息只通过 `Capabilities` 传入（设计文档 1.1 原则二）。
//!
//! 对外四个入口：[`recommend_plan`]（按场景生成计划）、[`update_plan`]（用户改参数后修回自洽）、
//! [`apply_fix_to_plan`]（执行保真度冲突的一键修正）、[`evaluate`]（派生界面需要的全部内容）。

pub mod args;
pub mod container;
pub mod encoders;
pub mod estimate;
pub mod explain;
pub mod fidelity;
pub mod fps;
pub mod meta;
pub mod strategy;
pub mod text;

use std::path::Path;

use crate::model::{Capabilities, FpsPolicy, MediaInfo, PlanResult, Scenario, StreamAction, TranscodePlan};

pub use strategy::suggest_scenario;

pub fn recommend_plan(media: &MediaInfo, scenario: Scenario, caps: &Capabilities) -> TranscodePlan {
    strategy::recommend(media, scenario, caps)
}

pub fn update_plan(plan: TranscodePlan, media: &MediaInfo, caps: &Capabilities) -> TranscodePlan {
    strategy::normalize_plan(plan, media, caps)
}

pub fn apply_fix_to_plan(plan: TranscodePlan, fix_id: &str, media: &MediaInfo, caps: &Capabilities) -> TranscodePlan {
    strategy::normalize_plan(fidelity::apply_fix(plan, fix_id, media), media, caps)
}

/// 重编码后能省下的比例低于这个值，就提示"不建议转码"
const WORTH_IT_THRESHOLD: f64 = 0.25;

/// 从计划派生界面所需的全部内容。`output` 是命令里写入的路径（通常是临时文件）
pub fn evaluate(media: &MediaInfo, plan: &TranscodePlan, caps: &Capabilities, output: &Path) -> PlanResult {
    let v = media.video.first();
    let encode = plan.video.action == StreamAction::Encode;
    let fps_insight = match (v, plan.video.fps) {
        (Some(v), FpsPolicy::Cfr { fps }) if encode => Some(fps::fps_insight(v, media.duration_sec, fps)),
        _ => None,
    };

    let est = estimate::estimate(media, plan);
    let saving = 1.0 - est.video_bps / est.source_video_bps;
    let not_worth_it = (encode && plan.scenario != Scenario::Editing && saving < WORTH_IT_THRESHOLD).then(|| {
        let how = if saving > 0.02 {
            format!("只能省下约 {}%", text::plain((saving * 100.0).round()))
        } else {
            "几乎省不下空间".to_string()
        };
        let lower = if plan.video.rate_control.target_kbps().is_some() { "目标码率" } else { "画质档位" };
        format!(
            "源视频只有 {}，已经高度压缩。按当前设置重新编码{how}，还会叠加一次画质损失。建议改为「原样封装」，或调低{lower}。",
            text::format_bitrate(est.source_video_bps)
        )
    });

    let segments = args::build_arg_segments(media, plan, caps, output);
    let first_pass = args::build_first_pass(media, plan, output).map(|segs| args::flatten(&segs));
    PlanResult {
        plan: plan.clone(),
        decisions: explain::explain(media, plan, caps, fps_insight.as_ref(), Some(&est.estimate)),
        fidelity: fidelity::resolve_fidelity(media, plan, caps),
        args: args::flatten(&segments),
        segments,
        first_pass,
        estimate: est.estimate,
        fps_insight,
        not_worth_it,
    }
}
