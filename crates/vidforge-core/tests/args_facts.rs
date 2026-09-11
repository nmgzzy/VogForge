//! 命令构建的技术事实断言（每条对应技术事实文档里的一条结论）与关键组合的快照。
//!
//! 输入复用黄金样本（见 golden_engine.rs）：回归样本只保证"输出没变"，这里独立地钉住"输出是对的"。

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use vidforge_core::model::{
    ArgSegment, Capabilities, Container, DoviAction, EncoderId, EncoderProbe, FpsPolicy, MediaInfo, RateControl,
    StreamAction, TranscodePlan, Vendor,
};
use vidforge_core::pipeline::args::{build_arg_segments, build_first_pass, flatten};
use vidforge_core::pipeline::update_plan;

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
        // 4.3：CFR 只用 -fps_mode:v cfr 搭配 -r
        if encode && matches!(vp.fps, FpsPolicy::Cfr { .. }) {
            assert!(a.windows(3).any(|w| w[0] == "-fps_mode:v" && w[1] == "cfr" && w[2] == "-r"), "{name}: CFR 写法");
        }
        // 7.8 / 2.x：硬解一律 -hwaccel auto（scale_vt 管线除外）；保留杜比视界时不硬解（hwdownload 会丢 RPU）
        if let Some(i) = a.iter().position(|x| x == "-hwaccel") {
            assert!(matches!(a[i + 1].as_str(), "auto" | "videotoolbox"), "{name}: -hwaccel {}", a[i + 1]);
            assert!(!(encode && vp.dovi == DoviAction::Preserve), "{name}: 保留杜比视界却用了硬解");
        }
        // 7.4：QSV 10bit 必须 p010le（HEVC 还要 main10），并显式指定码率控制
        if encode && vp.encoder.vendor() == Vendor::Intel {
            assert!(has(a, &["-global_quality"]) || has(a, &["-b:v"]), "{name}: QSV 没有显式码率控制");
            if vp.bit_depth == 10 {
                assert!(has(a, &["-pix_fmt", "p010le"]), "{name}: QSV 10bit 不是 p010le");
                if vp.encoder == EncoderId::HevcQsv {
                    assert!(has(a, &["-profile:v", "main10"]), "{name}: hevc_qsv 10bit 缺少 main10");
                }
            }
        }
        // 8.2：NVENC 的 -cq 必须配 -rc vbr -b:v 0
        if encode && vp.encoder.vendor() == Vendor::Nvidia && vp.rate_control == RateControl::Quality {
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

fn value_of<'a>(a: &'a [String], key: &str) -> Option<&'a str> {
    a.iter().position(|x| x == key).map(|i| a[i + 1].as_str())
}

fn kbps(v: &str) -> u32 {
    v.trim_end_matches('k').parse().unwrap()
}

/// 各厂商编码器都可用（NVENC / AMF / VideoToolbox 只核对写法）
fn every_encoder(g: &Golden) -> Capabilities {
    let mut c = g.caps["dev"].clone();
    for id in [EncoderId::HevcNvenc, EncoderId::HevcAmf, EncoderId::HevcVideotoolbox] {
        c.encoders.retain(|e| e.id != id);
        let (vendor, codec) = (id.vendor(), id.codec());
        c.encoders.push(EncoderProbe { id, vendor, codec, usable: true, ten_bit: true, error: None, failure: None });
    }
    c
}

#[test]
fn rate_control_facts_hold_for_every_case() {
    // 技术事实文档 8.2：码率控制的写法错了往往不报错，只是静默换成别的模式
    let g = golden();
    let caps = every_encoder(&g);
    let mut checked = 0;
    for c in g.cases.iter().filter(|c| c.plan.video.action == StreamAction::Encode) {
        for rc in [
            RateControl::Bitrate { kbps: 6000 },
            RateControl::Capped { kbps: 8000 },
            RateControl::TwoPass { kbps: 5000 },
        ] {
            let mut plan = c.plan.clone();
            plan.video.rate_control = rc;
            let plan = update_plan(plan, &c.media, &caps);
            let out = Path::new(&c.output);
            let a = flatten(&build_arg_segments(&c.media, &plan, &caps, out));
            let (vp, name) = (&plan.video, format!("{} / {rc:?}", c.name));
            let family = vidforge_core::pipeline::encoders::family(vp.encoder);
            use vidforge_core::pipeline::encoders::Family;
            // x265 只给 maxrate 不给 bufsize 会静默忽略上限
            if has(&a, &["-maxrate"]) && family != Family::SvtAv1 {
                assert!(has(&a, &["-bufsize"]), "{name}: 有 maxrate 没有 bufsize");
            }
            // QSV：只给 global_quality + maxrate 会落到 CQP；目标码率等于峰值会落到 CBR
            if family == Family::Qsv {
                if let Some(max) = value_of(&a, "-maxrate") {
                    let target = value_of(&a, "-b:v").unwrap_or_else(|| panic!("{name}: QSV 有峰值没有目标码率"));
                    assert!(kbps(target) < kbps(max), "{name}: QSV 目标码率不小于峰值");
                }
            }
            match vp.rate_control {
                RateControl::Quality => panic!("{name}: 码率控制被丢掉了"),
                RateControl::Bitrate { kbps: k } | RateControl::TwoPass { kbps: k } => {
                    assert_eq!(value_of(&a, "-b:v"), Some(format!("{k}k").as_str()), "{name}");
                    for q in ["-crf", "-global_quality", "-cq", "-qp_i", "-q:v"] {
                        assert!(!has(&a, &[q]), "{name}: 按码率编码却出现 {q}");
                    }
                }
                RateControl::Capped { kbps: k } => {
                    assert_eq!(value_of(&a, "-maxrate"), Some(format!("{k}k").as_str()), "{name}");
                    assert!(
                        ["-crf", "-global_quality", "-cq"].iter().any(|q| has(&a, &[q])),
                        "{name}: 限峰值却没有质量参数"
                    );
                }
            }
            // 两遍只给软件编码器；第一遍与第二遍用同一个统计文件前缀
            let first = build_first_pass(&c.media, &plan, out);
            if let RateControl::TwoPass { .. } = vp.rate_control {
                assert!(!vp.encoder.is_hardware(), "{name}: 两遍用了硬件编码器");
                let first = flatten(&first.unwrap());
                assert_eq!(value_of(&first, "-passlogfile"), value_of(&a, "-passlogfile"), "{name}");
                assert_eq!((value_of(&first, "-pass"), value_of(&a, "-pass")), (Some("1"), Some("2")), "{name}");
            } else {
                assert!(first.is_none() && !has(&a, &["-pass"]), "{name}: 单遍命令出现了 -pass");
            }
            checked += 1;
        }
    }
    assert!(checked > 300, "只核对了 {checked} 个组合");
}

#[test]
fn rate_control_matrix_snapshot() {
    let g = golden();
    let caps = every_encoder(&g);
    let case = g.cases.iter().find(|c| c.name == "dev/m-drone/archive").unwrap();
    let out = Path::new(&case.output);
    let mut lines = Vec::new();
    for enc in [
        EncoderId::Libx265,
        EncoderId::Libx264,
        EncoderId::Libsvtav1,
        EncoderId::HevcQsv,
        EncoderId::Av1Qsv,
        EncoderId::HevcNvenc,
        EncoderId::HevcAmf,
        EncoderId::HevcVideotoolbox,
    ] {
        for rc in [
            RateControl::Quality,
            RateControl::Bitrate { kbps: 6000 },
            RateControl::Capped { kbps: 8000 },
            RateControl::TwoPass { kbps: 5000 },
        ] {
            let mut plan = case.plan.clone();
            (plan.video.codec, plan.video.encoder, plan.video.encoder_auto) = (enc.codec(), enc, false);
            plan.video.quality_value = vidforge_core::pipeline::encoders::quality_value(enc, plan.video.quality);
            plan.video.rate_control = rc;
            let plan = update_plan(plan, &case.media, &caps);
            let segs = build_arg_segments(&case.media, &plan, &caps, out);
            let video = segs.iter().find(|s| s.label == "视频").unwrap().args.join(" ");
            lines.push(format!("{:<18} {:<32} {video}", enc.name(), format!("{rc:?}")));
        }
    }
    insta::assert_snapshot!("rate_control_matrix", lines.join("\n"));
}
