//! 与前端 TS 引擎对照：两边对同一批输入必须产出完全相同的命令。
//!
//! 样本由 `src/mock/engine/golden.test.ts` 生成（`UPDATE_GOLDEN=1` 时重写），覆盖 6 个示例素材 × 8 个场景
//! × 两套环境、每个一键修正、全部编码器的 8/10bit、四条色调映射管线、字幕与容器组合等。

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use vidforge_core::model::{ArgSegment, Capabilities, MediaInfo, TranscodePlan};
use vidforge_core::pipeline::args::build_arg_segments;

#[derive(Deserialize)]
struct Golden {
    caps: HashMap<String, Capabilities>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    caps: String,
    media: MediaInfo,
    plan: TranscodePlan,
    output: String,
    segments: Vec<ArgSegment>,
}

fn load() -> Golden {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/golden/engine.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("缺少 golden/engine.json，先运行前端的 golden 测试"))
        .expect("golden 文件结构与 Rust 模型不一致")
}

fn render(segs: &[ArgSegment]) -> String {
    segs.iter().map(|s| format!("[{}] {}", s.label, s.args.join(" "))).collect::<Vec<_>>().join("\n")
}

#[test]
fn args_match_the_ts_engine_for_every_golden_case() {
    let golden = load();
    assert!(golden.cases.len() > 150, "样本太少：{}", golden.cases.len());
    let mut diffs = Vec::new();
    for c in &golden.cases {
        let caps = &golden.caps[&c.caps];
        let got = build_arg_segments(&c.media, &c.plan, caps, Path::new(&c.output));
        if got != c.segments {
            diffs.push(format!("── {}\nTS:\n{}\nRust:\n{}", c.name, render(&c.segments), render(&got)));
        }
    }
    assert!(diffs.is_empty(), "{} 个样本不一致：\n{}", diffs.len(), diffs.join("\n"));
}
