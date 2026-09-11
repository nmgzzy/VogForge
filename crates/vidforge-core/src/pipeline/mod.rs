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
pub mod loudness;
pub mod meta;
pub mod strategy;
pub mod text;

use std::path::Path;

use crate::i18n::{Lang, pick};
use crate::model::{Capabilities, FpsPolicy, MediaInfo, PlanResult, Scenario, StreamAction, TranscodePlan};
use crate::tr;

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

/// 从计划派生界面所需的全部内容。`output` 是命令里写入的路径（通常是临时文件），说明文字按 `lang` 生成
pub fn evaluate(media: &MediaInfo, plan: &TranscodePlan, caps: &Capabilities, output: &Path, lang: Lang) -> PlanResult {
    let v = media.video.first();
    let encode = plan.video.action == StreamAction::Encode;
    let fps_insight = match (v, plan.video.fps) {
        (Some(v), FpsPolicy::Cfr { fps }) if encode => Some(fps::fps_insight(v, media.duration_sec, fps)),
        _ => None,
    };

    let est = estimate::estimate(media, plan);
    let saving = 1.0 - est.video_bps / est.source_video_bps;
    let not_worth_it = (encode && plan.scenario != Scenario::Editing && saving < WORTH_IT_THRESHOLD).then(|| {
        let source = text::format_bitrate(est.source_video_bps);
        let lower = if plan.video.rate_control.target_kbps().is_some() {
            pick(lang, "目标码率", "target bitrate")
        } else {
            pick(lang, "画质档位", "quality tier")
        };
        if saving > 0.02 {
            let pct = text::plain((saving * 100.0).round());
            tr!(
                lang,
                "源视频只有 {}，已经高度压缩。按当前设置重新编码只能省下约 {}%，还会叠加一次画质损失。建议改为「原样封装」，或调低{}。",
                "The source video is only {} and already highly compressed. Re-encoding with these settings saves only about {}% and adds another generation of quality loss. Switch to Remux, or lower the {}.",
                source,
                pct,
                lower
            )
        } else {
            tr!(
                lang,
                "源视频只有 {}，已经高度压缩。按当前设置重新编码几乎省不下空间，还会叠加一次画质损失。建议改为「原样封装」，或调低{}。",
                "The source video is only {} and already highly compressed. Re-encoding with these settings saves almost no space and adds another generation of quality loss. Switch to Remux, or lower the {}.",
                source,
                lower
            )
        }
    });

    let segments = args::build_arg_segments(media, plan, caps, output);
    let first_pass = args::build_first_pass(media, plan, caps, output).map(|segs| args::flatten(&segs));
    let measure: Vec<Vec<String>> = loudness::measure_all(media, plan).into_iter().map(|(_, a)| a).collect();
    PlanResult {
        plan: plan.clone(),
        decisions: explain::explain(media, plan, caps, fps_insight.as_ref(), Some(&est.estimate), lang),
        fidelity: fidelity::resolve_fidelity(media, plan, caps, lang),
        args: args::flatten(&segments),
        segments,
        first_pass,
        loudness_measure: (!measure.is_empty()).then_some(measure),
        estimate: est.estimate,
        fps_insight,
        not_worth_it,
    }
}
