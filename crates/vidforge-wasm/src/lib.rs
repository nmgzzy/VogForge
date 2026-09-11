//! vidforge-core 决策引擎的 WebAssembly 入口。
//!
//! 浏览器预览（`pnpm dev`）与前端组件测试通过它调用和桌面应用完全相同的 Rust 逻辑，
//! 前端因此不再维护第二份引擎。所有参数与返回值都是 JSON 字符串，结构即 `src/bindings/` 的类型。
//! wasm32 上拿不到系统时间，`{date}` 由调用方传入。

use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;
use vidforge_core::config::{Lang, Settings};
use vidforge_core::ffmpeg::capability::localize;
use vidforge_core::model::{Capabilities, MediaInfo, Scenario, TranscodePlan};
use vidforge_core::{output, pipeline};
use wasm_bindgen::prelude::*;

fn parse<T: DeserializeOwned>(what: &str, json: &str) -> Result<T, JsError> {
    serde_json::from_str(json).map_err(|e| JsError::new(&format!("{what}格式不对：{e}")))
}

fn json<T: Serialize>(v: &T) -> Result<String, JsError> {
    serde_json::to_string(v).map_err(|e| JsError::new(&e.to_string()))
}

fn scenario(s: &str) -> Result<Scenario, JsError> {
    parse("场景", &format!("\"{s}\""))
}

/// 按素材特征推荐的起始场景
#[wasm_bindgen]
pub fn suggest_scenario(media: &str) -> Result<String, JsError> {
    let m: MediaInfo = parse("媒体信息", media)?;
    json(&pipeline::suggest_scenario(&m))
}

#[wasm_bindgen]
pub fn recommend_plan(media: &str, scenario_id: &str, caps: &str) -> Result<String, JsError> {
    let (m, c): (MediaInfo, Capabilities) = (parse("媒体信息", media)?, parse("环境能力", caps)?);
    json(&pipeline::recommend_plan(&m, scenario(scenario_id)?, &c))
}

#[wasm_bindgen]
pub fn update_plan(plan: &str, media: &str, caps: &str) -> Result<String, JsError> {
    let (p, m, c): (TranscodePlan, MediaInfo, Capabilities) =
        (parse("计划", plan)?, parse("媒体信息", media)?, parse("环境能力", caps)?);
    json(&pipeline::update_plan(p, &m, &c))
}

#[wasm_bindgen]
pub fn apply_fix(plan: &str, fix_id: &str, media: &str, caps: &str) -> Result<String, JsError> {
    let (p, m, c): (TranscodePlan, MediaInfo, Capabilities) =
        (parse("计划", plan)?, parse("媒体信息", media)?, parse("环境能力", caps)?);
    json(&pipeline::apply_fix_to_plan(p, fix_id, &m, &c))
}

/// 派生界面所需的全部内容；输出路径按设置计算
#[wasm_bindgen]
pub fn evaluate(media: &str, plan: &str, caps: &str, settings: &str, date: &str) -> Result<String, JsError> {
    let (m, p, c, s): (MediaInfo, TranscodePlan, Capabilities, Settings) =
        (parse("媒体信息", media)?, parse("计划", plan)?, parse("环境能力", caps)?, parse("设置", settings)?);
    let out = output::output_path(&m, &p, &s, date);
    json(&pipeline::evaluate(&m, &p, &c, Path::new(&out), s.language))
}

/// 界面需要的静态规则表（质量刻度、preset、码率控制支持、标准帧率档）
#[wasm_bindgen]
pub fn engine_meta() -> Result<String, JsError> {
    json(&pipeline::meta::engine_meta())
}

/// 帧率控件的建议（推荐 CFR 目标、是否极端可变帧率）；没有视频流时返回 "null"
#[wasm_bindgen]
pub fn video_hints(media: &str) -> Result<String, JsError> {
    let m: MediaInfo = parse("媒体信息", media)?;
    json(&pipeline::meta::video_hints(&m))
}

/// 把能力里由结构化字段决定的说明换成指定语言。桌面端由后端直接按语言返回，浏览器预览的示例能力靠它切换
#[wasm_bindgen]
pub fn localize_caps(caps: &str, lang: &str) -> Result<String, JsError> {
    let mut c: Capabilities = parse("环境能力", caps)?;
    let l: Lang = parse("语言", &format!("\"{lang}\""))?;
    localize(&mut c, l);
    json(&c)
}

/// 决策引擎实际使用的能力：按设置关掉硬件编码 / 硬件解码（需求 F-5.6）
#[wasm_bindgen]
pub fn effective_caps(caps: &str, settings: &str) -> Result<String, JsError> {
    let (c, s): (Capabilities, Settings) = (parse("环境能力", caps)?, parse("设置", settings)?);
    json(&c.restricted(s.hw_encode, s.hw_decode, &[], s.language))
}
