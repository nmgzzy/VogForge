//! 命令构建的技术事实断言（每条对应技术事实文档里的一条结论）与关键组合的快照。
//!
//! 输入复用前端生成的黄金样本（见 golden_engine.rs），这样 Rust 端独立地钉住事实，而不只是"和 TS 一样"。

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use vidforge_core::model::{
    ArgSegment, Capabilities, Container, DoviAction, EncoderId, MediaInfo, StreamAction, TranscodePlan,
};
use vidforge_core::pipeline::args::{build_arg_segments, flatten};

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
}

struct Built<'a> {
    case: &'a Case,
    segs: Vec<ArgSegment>,
    args: Vec<String>,
}

fn golden() -> Golden {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/golden/engine.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn build_all(g: &Golden) -> Vec<Built<'_>> {
    g.cases
        .iter()
        .map(|c| {
            let segs = build_arg_segments(&c.media, &c.plan, &g.caps[&c.caps], Path::new(&c.output));
            let args = flatten(&segs);
            Built { case: c, segs, args }
        })
        .collect()
}

fn has(args: &[String], seq: &[&str]) -> bool {
    args.windows(seq.len()).any(|w| w.iter().zip(seq).all(|(a, b)| a == b))
}

/// 由推荐或一键修正得到的计划（其余是故意构造的变体，可能绕过了自洽整理）
fn recommended(c: &Case) -> bool {
    c.name.starts_with("dev/") || c.name.starts_with("mac/")
}

#[test]
fn facts_hold_for_every_case() {
    let g = golden();
    for b in build_all(&g) {
        let (c, a, name) = (b.case, &b.args, &b.case.name);
        let vp = &c.plan.video;
        // 4.1：-vsync 在 9.0 已移除
        assert!(!a.iter().any(|x| x == "-vsync"), "{name}: 出现 -vsync");
        // 4.3：CFR 不用 fps 滤镜（会丢最后一帧）
        assert!(!a.iter().any(|x| x.contains("fps=")), "{name}: 用了 fps 滤镜");
        // 7.4：默认不传 -low_power
        assert!(!a.iter().any(|x| x == "-low_power"), "{name}: 出现 -low_power");
        // 12：不依赖不生效的色彩输出选项
        assert!(!a.iter().any(|x| x == "-color_primaries" || x == "-color_trc"), "{name}: 用了色彩输出选项");
        // 输出是临时文件，必须显式 -f，并带 -y
        assert!(has(a, &["-f", c.plan.container.muxer()]), "{name}: 缺少 -f");
        assert!(has(a, &["-nostdin", "-y"]), "{name}: 缺少 -y");
        assert!(has(a, &["-progress", "pipe:1"]), "{name}: 缺少进度输出");

        let v = c.media.video.first();
        let encode = vp.action == StreamAction::Encode;
        // 3.5：MP4 / MOV 输出 HEVC 必带 hvc1
        let hevc_out =
            if encode { vp.codec == vidforge_core::model::Codec::Hevc } else { v.is_some_and(|v| v.codec == "hevc") };
        if c.plan.container != Container::Mkv && hevc_out {
            assert!(has(a, &["-tag:v", "hvc1"]), "{name}: MP4 HEVC 缺少 hvc1");
        }
        // 3.5：MP4 输出杜比视界必带 -strict unofficial
        if c.plan.container != Container::Mkv && encode && vp.dovi == DoviAction::Preserve {
            assert!(has(a, &["-strict", "unofficial"]), "{name}: MP4 DV 缺少 -strict unofficial");
        }
        // 3.1：源含 DV 且用 libx265 / libsvtav1 重编码时必须显式表态
        if encode
            && v.is_some_and(|v| v.dolby_vision.is_some())
            && matches!(vp.encoder, EncoderId::Libx265 | EncoderId::Libsvtav1)
        {
            let want = if vp.dovi == DoviAction::Preserve { "1" } else { "0" };
            assert!(has(a, &["-dolbyvision", want]), "{name}: 缺少 -dolbyvision {want}");
        }
        // 7.4：QSV 10bit 必须 p010le（HEVC 还要 main10），并显式指定质量
        if encode && vp.encoder.vendor() == vidforge_core::model::Vendor::Intel {
            assert!(has(a, &["-global_quality"]), "{name}: QSV 没有显式码率控制");
            if vp.bit_depth == 10 {
                assert!(has(a, &["-pix_fmt", "p010le"]), "{name}: QSV 10bit 不是 p010le");
                if vp.encoder == EncoderId::HevcQsv {
                    assert!(has(a, &["-profile:v", "main10"]), "{name}: hevc_qsv 10bit 缺少 main10");
                }
            }
        }
        // 8.2：NVENC 的 -cq 必须配 -rc vbr -b:v 0
        if encode && vp.encoder.vendor() == vidforge_core::model::Vendor::Nvidia {
            assert!(has(a, &["-rc", "vbr", "-b:v", "0", "-cq"]), "{name}: NVENC 恒定质量写法不对");
        }
        // 2.5：AV1 不用 libaom
        assert!(!a.iter().any(|x| x == "libaom-av1"), "{name}: 出现 libaom");
        // 6.4：pan 降混的那条轨不再出现 -ac
        let audio = b.segs.iter().find(|s| s.label == "音频").map(|s| s.args.clone()).unwrap_or_default();
        for (idx, _) in c.plan.audio.iter().enumerate() {
            let filter = audio.iter().position(|x| x == &format!("-filter:a:{idx}")).map(|p| &audio[p + 1]);
            if filter.is_some_and(|f| f.starts_with("pan=")) {
                assert!(!audio.iter().any(|x| x == &format!("-ac:a:{idx}")), "{name}: pan 之后又出现 -ac");
            }
        }
        // 9：推荐出的计划不会把 TrueHD / DTS / PGS 原样放进 MP4 / MOV
        if recommended(c) && c.plan.container != Container::Mkv {
            for t in c.plan.audio.iter().filter(|t| t.action == StreamAction::Copy) {
                let src = c.media.audio.iter().find(|a| a.index == t.source_index).unwrap();
                assert!(!matches!(src.codec.as_str(), "truehd" | "dts"), "{name}: {} 原样进了 MP4", src.codec);
            }
            assert!(!a.iter().any(|x| x == "0:s?"), "{name}: MP4 映射了全部字幕（含图形字幕）");
        }
    }
}

#[test]
fn key_combinations_snapshot() {
    let g = golden();
    let built = build_all(&g);
    let pick = [
        ("iphone_dv84_archive_keep_dv", "dev/m-iphone/archive"),
        ("iphone_dv84_streaming_qsv_cfr", "dev/m-iphone/streaming"),
        ("iphone_dv84_mobile_tonemap_sdr", "dev/m-iphone/mobile"),
        ("iphone_dv84_streaming_fix_dv", "dev/m-iphone/streaming/fix:dovi_preserve"),
        ("bluray_collection_copy_all", "dev/m-bluray/collection"),
        ("bluray_streaming_compat_tracks", "dev/m-bluray/streaming"),
        ("bluray_remux", "dev/m-bluray/remux"),
        ("screen_editing_cfr", "dev/m-screen/editing"),
        ("drone_smallest_av1", "dev/m-drone/smallest"),
        ("camera_social_cfr_downmix", "dev/m-camera/social"),
        ("mac_iphone_streaming_videotoolbox", "mac/m-iphone/streaming"),
        ("tonemap_opencl_720", "tonemap/tonemap_opencl/720"),
        ("tonemap_scale_vt_hw_encoder", "tonemap/scale_vt/hw-encoder"),
        ("nvenc_10bit", "encoder/hevc_nvenc/10bit"),
        ("amf_10bit", "encoder/hevc_amf/10bit"),
    ];
    for (snap, case) in pick {
        let b = built.iter().find(|b| b.case.name == case).unwrap_or_else(|| panic!("缺少样本 {case}"));
        let text = b.segs.iter().map(|s| format!("{:<4} {}", s.label, s.args.join(" "))).collect::<Vec<_>>().join("\n");
        insta::assert_snapshot!(snap, text);
    }
}
