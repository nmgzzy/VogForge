//! 引擎回归样本：约 200 个输入及其完整产出（推荐出的计划、一键修正后的计划、命令分段、`evaluate` 的全部结果）。
//!
//! 输入覆盖 6 个示例素材 × 8 个场景 × 两套环境、每个一键修正、全部编码器的 8/10bit、四条色调映射管线、
//! 字幕与容器组合等。样本最初由 TS 原型引擎生成，与 Rust 实现逐条对齐后原型删除，改由这里维护。
//!
//! 规则有意变更后：确认新输出正确，用 `UPDATE_GOLDEN=1 cargo test -p vidforge-core --test golden_engine`
//! 重写文件，把 diff 与规则改动一起提交。

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use vidforge_core::i18n::Lang;
use vidforge_core::model::{Capabilities, MediaInfo, Scenario, TranscodePlan};
use vidforge_core::pipeline::args::build_arg_segments;
use vidforge_core::pipeline::{apply_fix_to_plan, evaluate, recommend_plan};

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/golden/engine.json");

#[derive(Serialize, Deserialize)]
struct Golden {
    caps: BTreeMap<String, Capabilities>,
    cases: Vec<Case>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixFrom {
    plan: TranscodePlan,
    id: String,
}

/// 输入按模型类型读；产出按原始 JSON 读，模型加字段后旧文件仍能读进来再重写
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    caps: String,
    media: MediaInfo,
    /// 有 `scenario` 或 `fix_from` 时是产出，否则是输入（故意构造的变体）
    plan: TranscodePlan,
    output: String,
    /// 计划由推荐得到
    #[serde(skip_serializing_if = "Option::is_none")]
    scenario: Option<Scenario>,
    /// 计划由一键修正得到
    #[serde(skip_serializing_if = "Option::is_none")]
    fix_from: Option<FixFrom>,
    segments: Value,
    result: Value,
}

/// JSON 结构逐项比较，浮点按相对误差 1e-9（只防不同平台上的末位差异）
fn diff(path: &str, want: &Value, got: &Value, out: &mut Vec<String>) {
    match (want, got) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            if (x - y).abs() > 1e-9 * x.abs().max(y.abs()).max(1.0) {
                out.push(format!("{path}: 应为 {x}，实为 {y}"));
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                diff(&format!("{path}.{k}"), x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null), out);
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                out.push(format!("{path}: 长度应为 {}，实为 {}", x.len(), y.len()));
            }
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                diff(&format!("{path}[{i}]"), p, q, out);
            }
        }
        _ if want != got => out.push(format!("{path}: 应为 {want}，实为 {got}")),
        _ => {}
    }
}

#[test]
fn engine_output_matches_the_golden_file() {
    let mut golden: Golden = serde_json::from_str(&std::fs::read_to_string(PATH).expect("缺少 golden/engine.json"))
        .expect("golden 文件的输入部分与模型不一致");
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let (mut diffs, mut recommended, mut fixed) = (Vec::new(), 0, 0);

    for c in &mut golden.cases {
        let caps = &golden.caps[&c.caps];
        let plan = match (&c.scenario, &c.fix_from) {
            (Some(s), _) => {
                recommended += 1;
                recommend_plan(&c.media, *s, caps)
            }
            (None, Some(f)) => {
                fixed += 1;
                apply_fix_to_plan(f.plan.clone(), &f.id, &c.media, caps)
            }
            (None, None) => c.plan.clone(),
        };
        let out = Path::new(&c.output);
        let got = [
            ("plan", serde_json::to_value(&plan).unwrap()),
            ("segments", serde_json::to_value(build_arg_segments(&c.media, &plan, caps, out)).unwrap()),
            ("result", serde_json::to_value(evaluate(&c.media, &plan, caps, out, Lang::ZhCn)).unwrap()),
        ];
        if update {
            let [(_, _), (_, segments), (_, result)] = got;
            (c.plan, c.segments, c.result) = (plan, segments, result);
            continue;
        }
        let want = [serde_json::to_value(&c.plan).unwrap(), c.segments.clone(), c.result.clone()];
        for ((what, got), want) in got.iter().zip(&want) {
            let mut found = Vec::new();
            diff(what, want, got, &mut found);
            diffs.extend(found.into_iter().map(|d| format!("{}：{d}", c.name)));
        }
    }

    assert!(golden.cases.len() > 150, "样本太少：{}", golden.cases.len());
    assert!(recommended >= 96 && fixed > 10, "推荐 {recommended} 个、修正 {fixed} 个，样本不全");
    if update {
        // 每个样本一行：规则变更时 diff 只落在受影响的样本上
        let caps = serde_json::to_string(&golden.caps).unwrap();
        let cases: Vec<String> = golden.cases.iter().map(|c| serde_json::to_string(c).unwrap()).collect();
        let text = format!("{{\n\"caps\": {caps},\n\"cases\": [\n{}\n]\n}}\n", cases.join(",\n"));
        std::fs::write(PATH, text).unwrap();
        return;
    }
    assert!(
        diffs.is_empty(),
        "{} 处与 golden 不一致（前 40 条）。确认是有意的规则变更后用 UPDATE_GOLDEN=1 重写：\n{}",
        diffs.len(),
        diffs.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
    );
}
