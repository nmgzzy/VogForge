//! 引擎的行为测试：场景推荐、保真度判定与一键修正、常识保护、决策理由、编码器选择、码率控制。
//!
//! 素材与环境取自 `tests/fixtures/samples/`，浏览器预览的示例素材读的也是这份文件。

use std::path::Path;

use vidforge_core::i18n::Lang;
use vidforge_core::model::{
    AudioCodec, AudioMode, Capabilities, Codec, Container, Decision, DoviAction, EncoderId, EncoderProbe, EnvStatus,
    FailureKind, FidelityKind, FidelityState, FpsPolicy, HdrAction, MediaInfo, PlanResult, Platform, RateControl,
    ResolutionPreset, Scenario, SegmentKind, Severity, StreamAction, ToneMapPipeline, TranscodePlan, Vendor,
};
use vidforge_core::pipeline::args::{Dimensions, build_first_pass, downmix_filter, tail_pad, target_dimensions};
use vidforge_core::pipeline::encoders::{EncoderNeeds, pick_encoder, quality_value};
use vidforge_core::pipeline::fps::recommend_cfr_target;
use vidforge_core::pipeline::{apply_fix_to_plan, evaluate, recommend_plan, update_plan};

const OUT: &str = "/out/VidForge/clip.mkv";
const ALL: [Scenario; 8] = [
    Scenario::Archive,
    Scenario::Collection,
    Scenario::Streaming,
    Scenario::Mobile,
    Scenario::Social,
    Scenario::Editing,
    Scenario::Smallest,
    Scenario::Remux,
];

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/samples/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不了 {path}：{e}"))
}

fn samples() -> Vec<MediaInfo> {
    serde_json::from_str(&fixture("media.json")).unwrap()
}

fn media(id: &str) -> MediaInfo {
    samples().into_iter().find(|m| m.id == id).unwrap_or_else(|| panic!("没有样本 {id}"))
}

/// 开发机的真实探测结果：QSV 全部可用，NVENC / AMF 设备缺失
fn caps() -> Capabilities {
    serde_json::from_str(&fixture("capabilities.json")).unwrap()
}

fn probe(id: EncoderId, ten_bit: bool) -> EncoderProbe {
    EncoderProbe { id, vendor: id.vendor(), codec: id.codec(), usable: true, ten_bit, error: None, failure: None }
}

/// 典型 Mac：软件编码器 + VideoToolbox（不写 HDR10 元数据），只有 scale_vt
fn mac() -> Capabilities {
    let mut c = caps();
    c.platform = Platform::Macos;
    c.encoders.retain(|e| e.vendor == Vendor::Software);
    c.encoders.push(probe(EncoderId::HevcVideotoolbox, true));
    c.encoders.push(probe(EncoderId::H264Videotoolbox, false));
    for t in &mut c.tonemap {
        t.available = t.id == ToneMapPipeline::ScaleVt;
    }
    c
}

fn with_tonemap(available: &[ToneMapPipeline]) -> Capabilities {
    let mut c = caps();
    for t in &mut c.tonemap {
        t.available = available.contains(&t.id);
    }
    c
}

/// 模拟构建里没有这些编码器（例如 essentials 构建）
fn without(ids: &[EncoderId]) -> Capabilities {
    let mut c = caps();
    for e in c.encoders.iter_mut().filter(|e| ids.contains(&e.id)) {
        e.usable = false;
        e.ten_bit = false;
        e.failure = Some(FailureKind::NotBuilt);
    }
    c
}

fn eval(m: &MediaInfo, p: &TranscodePlan, c: &Capabilities) -> PlanResult {
    evaluate(m, p, c, Path::new(OUT), Lang::ZhCn)
}

fn run_with(m: &MediaInfo, s: Scenario, c: &Capabilities) -> PlanResult {
    eval(m, &recommend_plan(m, s, c), c)
}

fn run(m: &MediaInfo, s: Scenario) -> PlanResult {
    run_with(m, s, &caps())
}

fn has(args: &[String], seq: &[&str]) -> bool {
    args.windows(seq.len()).any(|w| w.iter().zip(seq).all(|(a, b)| a == b))
}

fn state(r: &PlanResult, kind: FidelityKind) -> FidelityState {
    r.fidelity.iter().find(|f| f.kind == kind).unwrap().state
}

fn decision<'a>(r: &'a PlanResult, field: &str) -> Option<&'a Decision> {
    r.decisions.iter().find(|d| d.field == field)
}

fn reason(r: &PlanResult, field: &str) -> String {
    decision(r, field).map(|d| d.reason.clone()).unwrap_or_default()
}

fn segment(r: &PlanResult, kind: SegmentKind) -> String {
    r.segments.iter().find(|s| s.kind == kind).map(|s| s.args.join(" ")).unwrap_or_default()
}

fn vf(r: &PlanResult) -> String {
    r.segments.iter().find(|s| s.kind == SegmentKind::Filter).map(|s| s.args[1].clone()).unwrap_or_default()
}

// ───────────────── iPhone 杜比视界 8.4 ─────────────────

#[test]
fn iphone_archive_uses_cpu_10bit_and_keeps_dv_and_vfr() {
    let r = run(&media("m-iphone"), Scenario::Archive);
    let vp = &r.plan.video;
    assert_eq!((vp.encoder, vp.bit_depth, vp.dovi), (EncoderId::Libx265, 10, DoviAction::Preserve));
    assert_eq!(vp.fps, FpsPolicy::Keep);
    assert_eq!(r.plan.container, Container::Mkv);
    assert!(has(&r.args, &["-dolbyvision", "1"]) && has(&r.args, &["-pix_fmt", "yuv420p10le"]));
    for k in [FidelityKind::DolbyVision, FidelityKind::Hdr10, FidelityKind::TenBit] {
        assert_eq!(state(&r, k), FidelityState::Achievable, "{k:?}");
    }
}

#[test]
fn iphone_streaming_uses_qsv_cfr_and_drops_dv() {
    let r = run(&media("m-iphone"), Scenario::Streaming);
    assert_eq!(r.plan.video.encoder, EncoderId::HevcQsv);
    assert_eq!(r.plan.video.dovi, DoviAction::Disable);
    assert_eq!(r.plan.video.fps, FpsPolicy::Cfr { fps: 30.0 });
    assert!(has(&r.args, &["-fps_mode:v", "cfr", "-r", "30"]));
    assert!(has(&r.args, &["-pix_fmt", "p010le"]) && has(&r.args, &["-profile:v", "main10"]));
    assert!(!r.args.iter().any(|a| a == "-strict"));
}

#[test]
fn iphone_streaming_dv_fix_switches_to_cpu() {
    let (m, c) = (media("m-iphone"), caps());
    let r = run(&m, Scenario::Streaming);
    let dv = r.fidelity.iter().find(|f| f.kind == FidelityKind::DolbyVision).unwrap();
    assert_eq!(dv.state, FidelityState::NeedsChange);
    assert_eq!(dv.fixes[0].id, "dovi_preserve");
    let fixed = eval(&m, &apply_fix_to_plan(r.plan.clone(), "dovi_preserve", &m, &c), &c);
    assert_eq!(fixed.plan.video.encoder, EncoderId::Libx265);
    assert_eq!(state(&fixed, FidelityKind::DolbyVision), FidelityState::Achievable);
    // MP4 输出杜比视界必须 -strict unofficial，否则配置 box 不写入
    assert!(has(&fixed.args, &["-strict", "unofficial"]));
}

#[test]
fn iphone_mobile_tonemaps_with_dv_metadata() {
    let r = run(&media("m-iphone"), Scenario::Mobile);
    assert_eq!(r.plan.video.hdr_action, HdrAction::Tonemap);
    let vf = vf(&r);
    assert!(vf.contains("libplacebo") && vf.contains("apply_dolbyvision=1") && vf.contains("color_trc=bt709"));
}

#[test]
fn iphone_mobile_keep_hdr_fix_goes_hevc_10bit() {
    let (m, c) = (media("m-iphone"), caps());
    let r = run(&m, Scenario::Mobile);
    let next = eval(&m, &apply_fix_to_plan(r.plan, "keep_hdr", &m, &c), &c);
    let vp = &next.plan.video;
    assert_eq!((vp.codec, vp.hdr_action, vp.bit_depth), (Codec::Hevc, HdrAction::Keep, 10));
    assert_eq!(state(&next, FidelityKind::Hdr10), FidelityState::Achievable);
}

// ───────────────── 蓝光 remux：DV P7 + TrueHD Atmos + PGS ─────────────────

#[test]
fn bluray_collection_p7_is_impossible_and_offers_remux() {
    let r = run(&media("m-bluray"), Scenario::Collection);
    let dv = r.fidelity.iter().find(|f| f.kind == FidelityKind::DolbyVision).unwrap();
    assert_eq!(dv.state, FidelityState::Impossible);
    assert!(dv.fixes.iter().any(|f| f.id == "remux"));
    assert_eq!(r.plan.video.dovi, DoviAction::Disable);
    assert!(has(&r.args, &["-dolbyvision", "0"]));
}

#[test]
fn bluray_remux_fix_keeps_dv_without_reencoding() {
    let (m, c) = (media("m-bluray"), caps());
    let r = eval(&m, &apply_fix_to_plan(recommend_plan(&m, Scenario::Collection, &c), "remux", &m, &c), &c);
    assert_eq!(r.plan.video.action, StreamAction::Copy);
    assert!(has(&r.args, &["-c:v", "copy"]));
    assert_eq!(state(&r, FidelityKind::DolbyVision), FidelityState::Achievable);
}

#[test]
fn bluray_collection_copies_every_track_sub_and_chapter() {
    let r = run(&media("m-bluray"), Scenario::Collection);
    assert_eq!(r.plan.audio.len(), 4);
    assert!(r.plan.audio.iter().all(|t| t.action == StreamAction::Copy));
    assert!(has(&r.args, &["-map", "0:s?"]) && has(&r.args, &["-map_chapters", "0"]));
    for k in [FidelityKind::Lossless, FidelityKind::AllAudio, FidelityKind::AllSubtitles] {
        assert_eq!(state(&r, k), FidelityState::Achievable, "{k:?}");
    }
}

#[test]
fn bluray_streaming_converts_atmos_and_maps_only_text_subs() {
    let r = run(&media("m-bluray"), Scenario::Streaming);
    let truehd: Vec<_> = r.plan.audio.iter().filter(|t| t.source_index == 1).map(|t| t.codec).collect();
    assert_eq!(truehd, [Some(AudioCodec::Eac3), Some(AudioCodec::Aac)]);
    assert_eq!(state(&r, FidelityKind::Lossless), FidelityState::NeedsChange);
    assert!(has(&r.args, &["-map", "0:8"]) && !has(&r.args, &["-map", "0:5"]));
    assert!(has(&r.args, &["-c:s", "mov_text"]));
}

#[test]
fn bluray_streaming_lossless_fix_switches_to_mkv_and_copies_truehd() {
    let (m, c) = (media("m-bluray"), caps());
    let r = run(&m, Scenario::Streaming);
    let next = eval(&m, &apply_fix_to_plan(r.plan, "keep_lossless", &m, &c), &c);
    assert_eq!(next.plan.container, Container::Mkv);
    assert!(next.plan.audio.iter().any(|t| t.source_index == 1 && t.action == StreamAction::Copy));
    assert_eq!(state(&next, FidelityKind::Lossless), FidelityState::Achievable);
}

#[test]
fn downmix_of_71_uses_side_and_back_channels() {
    let f = downmix_filter(&media("m-bluray").audio[0]);
    assert!(f.contains("SL") && f.contains("BL") && f.contains("alimiter"), "{f}");
    // pan 引用输入里没有的声道不报错而是静默忽略（实测）：布局对不上时改用默认矩阵，保证每个声道都混进去
    let mut a = media("m-bluray").audio[0].clone();
    for (layout, channels) in [("6.1", 7), ("7.1(wide)", 8), ("4.0", 4), ("quad", 4), ("", 6)] {
        (a.channel_layout, a.channels) = (layout.into(), channels);
        assert_eq!(downmix_filter(&a), "aformat=channel_layouts=stereo,alimiter=limit=0.97:level=false", "{layout}");
    }
}

// ───────────────── 录屏：极端可变帧率 ─────────────────

#[test]
fn screen_editing_goes_cfr60_with_size_warning() {
    let r = run(&media("m-screen"), Scenario::Editing);
    assert_eq!(r.plan.video.fps, FpsPolicy::Cfr { fps: 60.0 });
    assert!(r.fps_insight.as_ref().unwrap().duplicated > 10_000);
    assert_eq!(decision(&r, "帧率").unwrap().severity, Severity::Warn);
    assert!(has(&r.args, &["-g", "30"]));
}

#[test]
fn copied_audio_has_no_filter_encoded_audio_gets_resample_compensation() {
    let (m, c) = (media("m-screen"), caps());
    let r = run(&m, Scenario::Editing);
    // 实测 -c:a copy 配合 -fps_mode:v cfr 音视频时长完全一致，无需处理
    assert!(!r.args.iter().any(|a| a.contains("aresample")));
    // 录屏的 AAC 在"转为兼容格式"下仍会复制；借最小体积场景的 Opus 得到一条重编码的轨
    let mut p = r.plan.clone();
    (p.audio_mode, p.scenario) = (AudioMode::CompatOnly, Scenario::Smallest);
    let mut p = update_plan(p, &m, &c);
    (p.scenario, p.video) = (Scenario::Editing, r.plan.video.clone());
    let r2 = eval(&m, &p, &c);
    assert_eq!(r2.plan.audio[0].action, StreamAction::Encode);
    assert!(r2.args.iter().any(|a| a.contains("aresample=async=1")));
}

#[test]
fn portrait_scaling_uses_the_short_edge_and_never_upscales() {
    let v = &media("m-screen").video[0];
    assert_eq!(target_dimensions(v, ResolutionPreset::P720), Some(Dimensions { w: 720, h: 1600, portrait: true }));
    assert_eq!(target_dimensions(v, ResolutionPreset::P1080), None);
    assert_eq!(target_dimensions(v, ResolutionPreset::P2160), None);
}

#[test]
fn screen_archive_keeps_vfr_and_explains_how_to_convert() {
    let r = run(&media("m-screen"), Scenario::Archive);
    assert_eq!(r.plan.video.fps, FpsPolicy::Keep);
    let d = decision(&r, "帧率").unwrap();
    // 帧率控件已有醒目提示，推荐说明里是普通条目
    assert_eq!(d.severity, Severity::Info);
    assert!(d.reason.contains("转为固定帧率"));
}

#[test]
fn nominal_fps_outliers_fall_back_to_the_average() {
    let mut v = media("m-screen").video[0].clone();
    (v.fps_nominal, v.fps_avg) = (1000.0, 29.8);
    assert_eq!(recommend_cfr_target(&v), 30.0);
}

// ───────────────── 常识保护 ─────────────────

#[test]
fn already_compressed_source_is_not_worth_reencoding() {
    let m = media("m-stream");
    assert!(run(&m, Scenario::Archive).not_worth_it.is_some());
    // 剪辑预处理体积本来就会上升；原样封装不重新编码
    assert!(run(&m, Scenario::Editing).not_worth_it.is_none());
    assert!(run(&m, Scenario::Remux).not_worth_it.is_none());
}

#[test]
fn high_bitrate_drone_footage_is_worth_it() {
    let r = run(&media("m-drone"), Scenario::Archive);
    assert!(r.not_worth_it.is_none());
    assert!(r.estimate.ratio < 0.4, "{}", r.estimate.ratio);
}

#[test]
fn no_upscaling_and_no_fps_increase() {
    let (m, c) = (media("m-drone"), caps());
    let v = m.video[0].clone();
    let mut p = recommend_plan(&m, Scenario::Archive, &c);
    p.video.resolution = ResolutionPreset::P2160;
    p.video.fps = FpsPolicy::Cap { max: v.fps_nominal * 2.0 };
    let r = eval(&m, &update_plan(p, &m, &c), &c);
    assert!(v.width.min(v.height) <= 2160, "样本应不高于 4K");
    assert!(!r.args.iter().any(|a| a.contains("scale=")), "放大了");
    assert_eq!(decision(&r, "分辨率").unwrap().severity, Severity::Tip);
    assert!(!r.args.iter().any(|a| a == "-r"), "帧率上限高于源时不应改帧率");
}

#[test]
fn targets_never_exceed_the_source() {
    let c = caps();
    // 帧率：固定帧率的目标高于源时拉回源帧率，可变帧率源以名义帧率为准
    for (id, source) in [("m-bluray", 24000.0 / 1001.0), ("m-iphone", 30.0), ("m-drone", 60000.0 / 1001.0)] {
        let m = media(id);
        let mut p = recommend_plan(&m, Scenario::Archive, &c);
        p.video.fps = FpsPolicy::Cfr { fps: 120.0 };
        assert_eq!(update_plan(p, &m, &c).video.fps, FpsPolicy::Cfr { fps: source }, "{id}");
    }
    // 源帧率读不出（0/0）时不封顶，推荐目标用 30；不合法的目标换成推荐值。绝不生成 -r 0
    let mut unknown = media("m-drone");
    (unknown.video[0].fps_nominal, unknown.video[0].fps_avg) = (0.0, 0.0);
    assert_eq!(recommend_cfr_target(&unknown.video[0]), 30.0);
    let mut p = recommend_plan(&unknown, Scenario::Editing, &c);
    assert_eq!(p.video.fps, FpsPolicy::Cfr { fps: 30.0 });
    p.video.fps = FpsPolicy::Cfr { fps: 60.0 };
    let r = eval(&unknown, &update_plan(p.clone(), &unknown, &c), &c);
    assert!(has(&r.args, &["-r", "60"]), "{:?}", r.args);
    p.video.fps = FpsPolicy::Cfr { fps: 0.0 };
    assert_eq!(update_plan(p, &unknown, &c).video.fps, FpsPolicy::Cfr { fps: 30.0 });

    // 降帧率照常，理由里说明丢帧
    let m = media("m-drone");
    let mut p = recommend_plan(&m, Scenario::Archive, &c);
    p.video.fps = FpsPolicy::Cfr { fps: 30.0 };
    let r = eval(&m, &update_plan(p, &m, &c), &c);
    assert_eq!(r.plan.video.fps, FpsPolicy::Cfr { fps: 30.0 });
    let d = decision(&r, "帧率").unwrap();
    assert!(d.reason.contains("降到 30 fps"), "{}", d.reason);

    // 码率：平均码率不高于源视频码率，峰值不高于它的 1.5 倍；低于源的照常
    let m = media("m-stream");
    let source = (m.video[0].bitrate.unwrap() / 1000) as u32;
    let with = |rc: RateControl| {
        let mut p = recommend_plan(&m, Scenario::Archive, &c);
        p.video.rate_control = rc;
        update_plan(p, &m, &c)
    };
    assert_eq!(with(RateControl::Bitrate { kbps: 20_000 }).video.rate_control, RateControl::Bitrate { kbps: source });
    assert_eq!(with(RateControl::TwoPass { kbps: 20_000 }).video.rate_control, RateControl::TwoPass { kbps: source });
    assert_eq!(
        with(RateControl::Capped { kbps: 20_000 }).video.rate_control,
        RateControl::Capped { kbps: source * 3 / 2 }
    );
    assert_eq!(with(RateControl::Bitrate { kbps: 2000 }).video.rate_control, RateControl::Bitrate { kbps: 2000 });
    let r = eval(&m, &with(RateControl::Bitrate { kbps: 20_000 }), &c);
    let d = decision(&r, "码率").unwrap();
    assert_eq!(d.severity, Severity::Tip);
    assert!(d.reason.contains("不高于源视频码率"), "{}", d.reason);
    assert!(has(&r.args, &["-b:v", &format!("{source}k")]), "{:?}", r.args);

    // 源码率未知时只受输入范围约束
    let mut unknown = media("m-stream");
    (unknown.bitrate, unknown.video[0].bitrate) = (0, None);
    let mut p = recommend_plan(&unknown, Scenario::Archive, &c);
    p.video.rate_control = RateControl::Bitrate { kbps: 20_000 };
    assert_eq!(update_plan(p, &unknown, &c).video.rate_control, RateControl::Bitrate { kbps: 20_000 });
}

#[test]
fn reencoded_audio_never_exceeds_the_source() {
    for m in samples() {
        for s in ALL {
            let r = run(&m, s);
            for t in r.plan.audio.iter().filter(|t| t.action == StreamAction::Encode) {
                let a = m.audio.iter().find(|a| a.index == t.source_index).unwrap();
                if let Some(ch) = t.channels {
                    assert!(ch <= a.channels, "{}/{s:?}: 声道 {} → {ch}", m.id, a.channels);
                }
                if let (Some(k), Some(b), false) = (t.bitrate_kbps, a.bitrate, a.lossless) {
                    assert!(u64::from(k) <= (b / 1000).max(32), "{}/{s:?}: {b} → {k}k", m.id);
                }
            }
        }
    }
    // 单声道 MP3 64k 发社交平台：转 AAC 仍是单声道 64k
    let mut m = media("m-stream");
    let a = &mut m.audio[0];
    (a.codec, a.channels, a.channel_layout, a.bitrate) = ("mp3".into(), 1, "mono".into(), Some(64_000));
    let r = run(&m, Scenario::Social);
    let t = &r.plan.audio[0];
    assert_eq!((t.codec, t.channels, t.bitrate_kbps), (Some(AudioCodec::Aac), Some(1), Some(64)));
    assert!(!r.args.iter().any(|x| x.starts_with("-ac")), "单声道不升成立体声：{:?}", r.args);
    // 四声道有损轨进 MP4：E-AC-3 保持四声道
    let a = &mut m.audio[0];
    (a.codec, a.channels, a.channel_layout, a.bitrate) = ("dts".into(), 4, "4.0".into(), Some(768_000));
    let r = run(&m, Scenario::Streaming);
    let t = r.plan.audio.iter().find(|t| t.codec == Some(AudioCodec::Eac3)).unwrap();
    assert_eq!((t.channels, t.bitrate_kbps, t.title.as_deref()), (Some(4), Some(640), Some("DD+ 4ch")));
}

#[test]
fn extra_args_that_would_add_outputs_are_not_used() {
    let (m, c) = (media("m-drone"), caps());
    let mut p = recommend_plan(&m, Scenario::Archive, &c);
    p.video.extra_args = Some("-tune grain".into());
    let r = eval(&m, &update_plan(p.clone(), &m, &c), &c);
    assert!(has(&r.args, &["-tune", "grain"]), "{:?}", r.args);
    assert!(decision(&r, "附加参数").is_none());
    // 值里有空格却没加引号：Video 会变成又一个输出文件，整段不用并警告
    p.video.extra_args = Some("-metadata title=My Video".into());
    let r = eval(&m, &update_plan(p, &m, &c), &c);
    assert!(!r.args.iter().any(|a| a == "Video" || a == "title=My"), "{:?}", r.args);
    let d = decision(&r, "附加参数").unwrap();
    assert_eq!(d.severity, Severity::Warn);
    assert!(d.reason.contains("「Video」"), "{}", d.reason);
}

#[test]
fn hdr_to_sdr_always_goes_through_a_tone_mapper() {
    let mut tonemapped = 0;
    for id in ["m-iphone", "m-bluray", "m-camera"] {
        let m = media(id);
        for s in ALL {
            let r = run(&m, s);
            if r.plan.video.hdr_action == HdrAction::Tonemap {
                tonemapped += 1;
                let vf = vf(&r);
                assert!(
                    ["libplacebo", "tonemap_opencl", "tonemap=", "scale_vt"].iter().any(|t| vf.contains(t)),
                    "{id}/{s:?} 转 SDR 却没有色调映射：{vf}"
                );
            }
        }
    }
    assert!(tonemapped >= 3);
}

#[test]
fn high_value_content_is_checked_by_default() {
    let r = run(&media("m-bluray"), Scenario::Collection);
    let f = &r.plan.fidelity;
    assert!(f.lossless && f.all_audio && f.all_subtitles && f.chapters);
    assert!(run(&media("m-iphone"), Scenario::Archive).plan.fidelity.dolby_vision);
}

#[test]
fn dv_profile_5_warns_about_the_missing_fallback_layer() {
    let mut m = media("m-iphone");
    m.video[0].dolby_vision.as_mut().unwrap().profile = 5;
    let r = run(&m, Scenario::Archive);
    let d = decision(&r, "杜比视界").unwrap();
    assert_eq!(d.severity, Severity::Warn);
    assert!(d.reason.contains("回退层"), "{}", d.reason);
}

#[test]
fn sdr_sources_mark_hdr_and_dv_as_not_applicable() {
    let r = run(&media("m-drone"), Scenario::Archive);
    assert_eq!(state(&r, FidelityKind::Hdr10), FidelityState::NotApplicable);
    assert_eq!(state(&r, FidelityKind::DolbyVision), FidelityState::NotApplicable);
}

#[test]
fn camera_pcm_is_copied_into_mkv_but_encoded_for_mp4() {
    let m = media("m-camera");
    let s = run(&m, Scenario::Streaming);
    assert_eq!((s.plan.audio[0].action, s.plan.audio[0].codec), (StreamAction::Encode, Some(AudioCodec::Aac)));
    assert_eq!(run(&m, Scenario::Archive).plan.audio[0].action, StreamAction::Copy);
}

// ───────────────── 保真度：每个一键修正都能消除冲突 ─────────────────

#[test]
fn every_fix_resolves_its_conflict() {
    let mut applied = 0;
    for c in [caps(), mac()] {
        for m in samples() {
            for s in ALL {
                let r = run_with(&m, s, &c);
                for item in r.fidelity.iter().filter(|f| f.state == FidelityState::NeedsChange) {
                    for fix in &item.fixes {
                        let next = eval(&m, &apply_fix_to_plan(r.plan.clone(), &fix.id, &m, &c), &c);
                        applied += 1;
                        assert_eq!(
                            state(&next, item.kind),
                            FidelityState::Achievable,
                            "{}/{s:?}/{:?}：修正 {} 后仍有冲突",
                            m.id,
                            c.platform,
                            fix.id
                        );
                    }
                }
            }
        }
    }
    assert!(applied > 10, "只覆盖了 {applied} 个修正");
}

#[test]
fn every_state_appears_in_the_samples() {
    let mut seen = Vec::new();
    for m in samples() {
        for s in ALL {
            for f in run(&m, s).fidelity {
                seen.push((f.kind, f.state));
            }
        }
    }
    for k in [FidelityKind::DolbyVision, FidelityKind::Hdr10, FidelityKind::Lossless] {
        for st in [FidelityState::Achievable, FidelityState::NeedsChange, FidelityState::NotApplicable] {
            assert!(seen.contains(&(k, st)), "样本里没有 {k:?} 的 {st:?}");
        }
    }
    assert!(seen.contains(&(FidelityKind::DolbyVision, FidelityState::Impossible)));
}

// ───────────────── 编码器选择 ─────────────────

fn needs(prefer_hw: bool, need_10bit: bool) -> EncoderNeeds {
    EncoderNeeds { prefer_hw, need_10bit, need_dv: false, need_hdr10: false, need_two_pass: false }
}

#[test]
fn hardware_preference_skips_unusable_vendors() {
    let c = caps();
    assert_eq!(pick_encoder(Codec::Hevc, &needs(true, true), &c).encoder, EncoderId::HevcQsv);
    // h264_qsv 没有 10bit
    assert_eq!(pick_encoder(Codec::H264, &needs(true, true), &c).encoder, EncoderId::Libx264);
    let mut none = c.clone();
    for e in &mut none.encoders {
        e.usable = e.vendor == Vendor::Software;
    }
    let p = pick_encoder(Codec::Av1, &needs(true, false), &none);
    assert_eq!(p.encoder, EncoderId::Libsvtav1);
    assert!(p.reason.contains("没有可用"));
}

// ───────────────── 决策理由与实际计划一致 ─────────────────

#[test]
fn decisions_describe_the_actual_plan() {
    let (bluray, screen, iphone) = (media("m-bluray"), media("m-screen"), media("m-iphone"));
    assert!(!reason(&run(&bluray, Scenario::Collection), "音频").contains("兼容轨"));
    assert!(reason(&run(&bluray, Scenario::Archive), "音频").contains("兼容轨"));
    let editing = reason(&run(&screen, Scenario::Editing), "编码器");
    assert!(editing.contains("剪辑") && !editing.contains("长期保存"));
    let slow = decision(&run(&bluray, Scenario::Collection), "耗时").cloned().unwrap();
    assert_eq!(slow.severity, Severity::Tip);
    assert!(slow.reason.contains("medium"));
    assert!(decision(&run(&iphone, Scenario::Archive), "耗时").is_none());
}

// ───────────────── macOS：VideoToolbox 不写 HDR10 元数据 ─────────────────

#[test]
fn mac_streaming_hdr10_skips_videotoolbox() {
    let c = mac();
    let r = run_with(&media("m-bluray"), Scenario::Streaming, &c);
    assert_eq!(r.plan.video.encoder, EncoderId::Libx265);
    assert_eq!(state(&r, FidelityKind::Hdr10), FidelityState::Achievable);
    let n = EncoderNeeds { need_hdr10: true, ..needs(true, true) };
    assert!(pick_encoder(Codec::Hevc, &n, &c).reason.contains("HDR10"));
    // HLG 不依赖元数据，仍可用 VideoToolbox
    assert_eq!(run_with(&media("m-iphone"), Scenario::Streaming, &c).plan.video.encoder, EncoderId::HevcVideotoolbox);
}

#[test]
fn mac_keep_hdr_fix_really_resolves_the_conflict() {
    let (m, c) = (media("m-bluray"), mac());
    let r = run_with(&m, Scenario::Mobile, &c);
    let fixed = eval(&m, &apply_fix_to_plan(r.plan, "keep_hdr", &m, &c), &c);
    assert_ne!(fixed.plan.video.encoder, EncoderId::HevcVideotoolbox);
    assert_eq!(state(&fixed, FidelityKind::Hdr10), FidelityState::Achievable);
}

// ───────────────── 色调映射按能力选择 ─────────────────

#[test]
fn tone_mapping_degrades_by_availability() {
    let m = media("m-iphone");
    let opencl =
        run_with(&m, Scenario::Mobile, &with_tonemap(&[ToneMapPipeline::TonemapOpencl, ToneMapPipeline::Zscale]));
    assert_eq!(opencl.plan.video.tonemap, Some(ToneMapPipeline::TonemapOpencl));
    assert!(opencl.args.join(" ").contains("tonemap_opencl") && !opencl.args.join(" ").contains("libplacebo"));

    let zscale = run_with(&m, Scenario::Mobile, &with_tonemap(&[ToneMapPipeline::Zscale]));
    assert_eq!(zscale.plan.video.tonemap, Some(ToneMapPipeline::Zscale));
    assert!(zscale.args.join(" ").contains("zscale=t=linear"));

    let none = run_with(&m, Scenario::Mobile, &with_tonemap(&[]));
    assert_eq!(none.plan.video.hdr_action, HdrAction::Keep);
    assert!(!["libplacebo", "tonemap", "zscale"].iter().any(|t| none.args.join(" ").contains(t)));
    assert_eq!(decision(&none, "HDR").unwrap().severity, Severity::Warn);
}

#[test]
fn normalize_swaps_an_unavailable_tone_mapper() {
    let m = media("m-iphone");
    let plan = recommend_plan(&m, Scenario::Mobile, &caps());
    assert_eq!(plan.video.tonemap, Some(ToneMapPipeline::Libplacebo));
    let next = update_plan(plan, &m, &with_tonemap(&[ToneMapPipeline::Zscale]));
    assert_eq!(next.video.tonemap, Some(ToneMapPipeline::Zscale));
}

#[test]
fn scale_vt_builds_a_real_videotoolbox_chain() {
    let mut c = with_tonemap(&[ToneMapPipeline::ScaleVt]);
    c.platform = Platform::Macos;
    let r = run_with(&media("m-iphone"), Scenario::Mobile, &c);
    let cmd = r.args.join(" ");
    assert_eq!(r.plan.video.tonemap, Some(ToneMapPipeline::ScaleVt));
    assert!(cmd.contains("scale_vt=") && !cmd.contains("zscale"));
    assert!(has(&r.args, &["-hwaccel", "videotoolbox", "-hwaccel_output_format", "videotoolbox_vld"]));
    // 软件编码器需要先把硬件帧下载回内存
    assert!(cmd.contains("hwdownload,format=nv12"));
}

#[test]
fn extra_args_respect_quotes() {
    let (m, c) = (media("m-drone"), caps());
    let mut plan = recommend_plan(&m, Scenario::Archive, &c);
    plan.video.extra_args = Some(r#"-metadata title="My Video""#.into());
    assert!(has(&eval(&m, &plan, &c).args, &["-metadata", "title=My Video"]));
}

// ───────────────── 编码器缺失时不生成跑不起来的命令 ─────────────────

#[test]
fn missing_svtav1_falls_back_to_av1_qsv_with_a_warning() {
    let c = without(&[EncoderId::Libsvtav1]);
    let r = run_with(&media("m-camera"), Scenario::Smallest, &c);
    assert_eq!((r.plan.video.codec, r.plan.video.encoder), (Codec::Av1, EncoderId::Av1Qsv));
    assert!(!r.args.iter().any(|a| a == "libsvtav1"));
    assert!(
        r.decisions
            .iter()
            .any(|d| d.field == "编码器" && d.severity == Severity::Warn && d.reason.contains("软件编码器"))
    );
}

#[test]
fn no_av1_encoder_at_all_switches_to_hevc_and_says_why() {
    let c = without(&[EncoderId::Libsvtav1, EncoderId::Av1Qsv]);
    let r = run_with(&media("m-camera"), Scenario::Smallest, &c);
    assert_eq!(r.plan.video.codec, Codec::Hevc);
    assert!(!r.args.join(" ").contains("av1"));
    let d = decision(&r, "编码格式").unwrap();
    assert_eq!(d.severity, Severity::Warn);
    assert!(d.reason.contains("没有可用的 AV1 编码器"));
    // 用户手动切到编不了的格式，normalize 换回能编的
    let m = media("m-drone");
    let mut plan = recommend_plan(&m, Scenario::Archive, &c);
    plan.video.codec = Codec::Av1;
    assert_eq!(update_plan(plan, &m, &c).video.codec, Codec::Hevc);
}

#[test]
fn manual_encoder_missing_in_the_new_environment_returns_to_auto() {
    let m = media("m-drone");
    let mut plan = recommend_plan(&m, Scenario::Streaming, &caps());
    (plan.video.encoder_auto, plan.video.encoder) = (false, EncoderId::HevcQsv);
    let next = update_plan(plan, &m, &without(&[EncoderId::HevcQsv, EncoderId::H264Qsv, EncoderId::Av1Qsv]));
    assert!(next.video.encoder_auto);
    assert_ne!(next.video.encoder, EncoderId::HevcQsv);
}

#[test]
fn missing_ffmpeg_does_not_crash_or_reshuffle() {
    let mut c = caps();
    c.status = EnvStatus::Missing;
    for e in &mut c.encoders {
        e.usable = false;
    }
    let m = media("m-drone");
    for s in ALL {
        assert!(!run_with(&m, s, &c).args.is_empty());
    }
}

// ───────────────── 容器与流映射 ─────────────────

#[test]
fn mkv_brings_font_attachments_mp4_does_not() {
    let (mut m, c) = (media("m-bluray"), caps());
    m.attachments = 2;
    assert!(has(&run_with(&m, Scenario::Collection, &c).args, &["-map", "0:t?"]));
    assert!(!has(&run_with(&m, Scenario::Mobile, &c).args, &["-map", "0:t?"]));
}

#[test]
fn amf_and_videotoolbox_10bit_state_p010le() {
    let (m, c) = (media("m-drone"), caps());
    for enc in [EncoderId::HevcAmf, EncoderId::HevcVideotoolbox] {
        let mut p = recommend_plan(&m, Scenario::Archive, &c);
        (p.video.encoder, p.video.encoder_auto, p.video.bit_depth) = (enc, false, 10);
        assert!(has(&eval(&m, &p, &c).args, &["-pix_fmt", "p010le"]), "{enc:?}");
    }
}

#[test]
fn tracks_that_do_not_fit_the_container_are_reencoded() {
    let (m, c) = (media("m-bluray"), caps());
    let r = run(&m, Scenario::Editing);
    assert_eq!(r.plan.container, Container::Mov);
    for t in &r.plan.audio {
        let src = m.audio.iter().find(|a| a.index == t.source_index).unwrap();
        if src.codec == "truehd" || src.codec.starts_with("dts") {
            assert_eq!((t.action, t.codec), (StreamAction::Encode, Some(AudioCodec::PcmS24le)));
        }
    }
    let mut plan = recommend_plan(&m, Scenario::Collection, &c);
    plan.container = Container::Mp4;
    let next = update_plan(plan, &m, &c);
    let thd =
        next.audio.iter().find(|t| m.audio.iter().any(|a| a.index == t.source_index && a.codec == "truehd")).unwrap();
    assert_eq!((thd.action, thd.codec), (StreamAction::Encode, Some(AudioCodec::Eac3)));
    assert_ne!(state(&eval(&m, &next, &c), FidelityKind::Lossless), FidelityState::Achievable);
}

#[test]
fn cfr_pads_the_tail_only_when_video_is_half_a_frame_short() {
    let (camera, c) = (media("m-camera"), caps());
    let mut plan = recommend_plan(&camera, Scenario::Editing, &c);
    plan.video.fps = FpsPolicy::Cfr { fps: 30.0 };
    let with = |v: f64, a: f64| {
        let mut m = camera.clone();
        m.video.iter_mut().for_each(|x| x.duration_sec = Some(v));
        m.audio.iter_mut().for_each(|x| x.duration_sec = Some(a));
        m
    };
    assert_eq!(tail_pad(&with(9.933, 10.0), &plan).as_deref(), Some("tpad=stop_mode=clone:stop_duration=0.067"));
    assert_eq!(tail_pad(&with(9.99, 10.0), &plan), None);
    assert_eq!(tail_pad(&with(10.0, 9.9), &plan), None);
    assert_eq!(tail_pad(&camera, &plan), None);
    // 没有音轨（航拍常见）时无从对齐，不补
    let drone = media("m-drone");
    assert_eq!(tail_pad(&drone, &recommend_plan(&drone, Scenario::Editing, &c)), None);
    let mut keep = plan.clone();
    keep.video.fps = FpsPolicy::Keep;
    assert_eq!(tail_pad(&with(9.933, 10.0), &keep), None);
}

#[test]
fn maps_by_real_stream_index_and_scales_by_display_orientation() {
    let c = caps();
    let mut cover = media("m-camera");
    cover.video[0].index = 1;
    let a = run(&cover, Scenario::Archive).args;
    assert!(has(&a, &["-map", "0:1"]) && !a.iter().any(|x| x == "0:v:0"));

    let mut rotated = media("m-camera");
    (rotated.video[0].width, rotated.video[0].height, rotated.video[0].rotation) = (1920, 1080, -90);
    let mut plan = recommend_plan(&rotated, Scenario::Social, &c);
    plan.video.resolution = ResolutionPreset::P720;
    assert_eq!(
        target_dimensions(&rotated.video[0], ResolutionPreset::P720),
        Some(Dimensions { w: 720, h: 1280, portrait: true })
    );
    // 相机样本是 HLG，社交分享会色调映射：libplacebo 的目标尺寸也按显示方向
    assert!(eval(&rotated, &plan, &c).args.iter().any(|x| x.contains("libplacebo=w=720:h=1280")));
    (plan.video.hdr_action, plan.video.tonemap) = (HdrAction::Keep, None);
    assert!(eval(&rotated, &plan, &c).args.iter().any(|x| x.starts_with("scale=720:-2")));
}

// ───────────────── 码率控制（需求 F-3.3，技术事实文档 8.2） ─────────────────

/// 手选编码器并指定码率控制，经过 normalize
fn rc_plan(m: &MediaInfo, enc: EncoderId, rc: RateControl, c: &Capabilities) -> TranscodePlan {
    let mut p = recommend_plan(m, Scenario::Archive, c);
    (p.video.codec, p.video.encoder, p.video.encoder_auto, p.video.bit_depth) = (enc.codec(), enc, false, 8);
    p.video.quality_value = quality_value(enc, p.video.quality);
    p.video.rate_control = rc;
    update_plan(p, m, c)
}

/// 所有编码器都可用的环境（NVENC / AMF / VideoToolbox 只看命令写法）
fn every_encoder() -> Capabilities {
    let mut c = caps();
    for id in [EncoderId::HevcNvenc, EncoderId::HevcAmf, EncoderId::HevcVideotoolbox] {
        c.encoders.retain(|e| e.id != id);
        c.encoders.push(probe(id, true));
    }
    c
}

#[test]
fn recommendations_default_to_constant_quality() {
    for m in samples() {
        for s in ALL {
            assert_eq!(run(&m, s).plan.video.rate_control, RateControl::Quality, "{}/{s:?}", m.id);
        }
    }
}

#[test]
fn bitrate_mode_arguments_per_encoder() {
    let (m, c) = (media("m-drone"), every_encoder());
    let rc = RateControl::Bitrate { kbps: 6000 };
    let video = |enc| {
        let r = eval(&m, &rc_plan(&m, enc, rc, &c), &c);
        assert_eq!(r.plan.video.rate_control, rc, "{enc:?}");
        segment(&r, SegmentKind::Video)
    };
    for enc in [EncoderId::Libx265, EncoderId::Libx264, EncoderId::HevcQsv] {
        let v = video(enc);
        assert!(v.contains("-b:v 6000k -maxrate 9000k -bufsize 18000k"), "{enc:?}: {v}");
        assert!(!v.contains("-crf") && !v.contains("-global_quality"), "{enc:?}: {v}");
    }
    let nvenc = video(EncoderId::HevcNvenc);
    assert!(nvenc.contains("-rc vbr -b:v 6000k -maxrate 9000k") && !nvenc.contains("-cq"), "{nvenc}");
    let amf = video(EncoderId::HevcAmf);
    assert!(amf.contains("-rc vbr_peak -b:v 6000k -maxrate 9000k") && !amf.contains("-qp_i"), "{amf}");
    let svt = video(EncoderId::Libsvtav1);
    assert!(svt.contains("-b:v 6000k") && !svt.contains("-maxrate") && !svt.contains("-crf"), "{svt}");
    let vt = video(EncoderId::HevcVideotoolbox);
    assert!(vt.ends_with("-b:v 6000k") && !vt.contains("-q:v"), "{vt}");
}

#[test]
fn capped_quality_arguments_per_encoder() {
    let (m, c) = (media("m-drone"), every_encoder());
    let rc = RateControl::Capped { kbps: 8000 };
    let r = |enc| eval(&m, &rc_plan(&m, enc, rc, &c), &c);
    let q = |enc| quality_value(enc, recommend_plan(&m, Scenario::Archive, &c).video.quality);
    let x265 = segment(&r(EncoderId::Libx265), SegmentKind::Video);
    // x265 只给 maxrate 不给 bufsize 会静默忽略上限（实测）
    let want = format!("-crf {} -maxrate 8000k -bufsize 16000k", q(EncoderId::Libx265));
    assert!(x265.contains(&want), "{x265}");
    // QSV 只给 global_quality + maxrate 会静默落到 CQP，必须带目标码率走 QVBR，且目标 < 峰值（相等会变 CBR）
    let qsv = segment(&r(EncoderId::HevcQsv), SegmentKind::Video);
    let want = format!("-global_quality {} -b:v 5333k -maxrate 8000k -bufsize 16000k", q(EncoderId::HevcQsv));
    assert!(qsv.contains(&want), "{qsv}");
    let nvenc = segment(&r(EncoderId::HevcNvenc), SegmentKind::Video);
    let want = format!("-rc vbr -b:v 0 -cq {} -maxrate 8000k -bufsize 16000k", q(EncoderId::HevcNvenc));
    assert!(nvenc.contains(&want), "{nvenc}");
    // 做不到"质量 + 峰值"的编码器换成峰值 1.5 倍对应的目标码率
    for enc in [EncoderId::Av1Qsv, EncoderId::HevcAmf, EncoderId::HevcVideotoolbox] {
        assert_eq!(r(enc).plan.video.rate_control, RateControl::Bitrate { kbps: 5333 }, "{enc:?}");
    }
    assert!(decision(&r(EncoderId::Libx265), "画质").unwrap().value.contains("峰值"));
    assert!(decision(&r(EncoderId::HevcQsv), "画质").unwrap().reason.contains("QVBR"));
}

#[test]
fn two_pass_needs_a_software_encoder() {
    let (m, c) = (media("m-drone"), caps());
    // 自动选择：流媒体本来偏好硬编，要两遍时换软件编码
    let mut p = recommend_plan(&m, Scenario::Streaming, &c);
    assert!(p.video.encoder.is_hardware());
    p.video.rate_control = RateControl::TwoPass { kbps: 5000 };
    let p = update_plan(p, &m, &c);
    assert_eq!(p.video.encoder, EncoderId::Libx265);
    assert_eq!(p.video.rate_control, RateControl::TwoPass { kbps: 5000 });
    // 手选了硬件编码器：保留编码器，换成单遍目标码率
    let hw = rc_plan(&m, EncoderId::HevcQsv, RateControl::TwoPass { kbps: 5000 }, &c);
    assert_eq!((hw.video.encoder, hw.video.rate_control), (EncoderId::HevcQsv, RateControl::Bitrate { kbps: 5000 }));
    assert!(eval(&m, &hw, &c).first_pass.is_none());
}

#[test]
fn two_pass_commands_share_everything_but_the_pass_number() {
    let c = caps();
    let prefix = format!("{OUT}.2pass");
    for (id, s) in [("m-drone", Scenario::Archive), ("m-screen", Scenario::Editing), ("m-iphone", Scenario::Mobile)] {
        let m = media(id);
        for enc in [EncoderId::Libx265, EncoderId::Libx264, EncoderId::Libsvtav1] {
            let mut p = recommend_plan(&m, s, &c);
            (p.video.codec, p.video.encoder_auto) = (enc.codec(), true);
            p.video.rate_control = RateControl::TwoPass { kbps: 4000 };
            let p = update_plan(p, &m, &c);
            assert_eq!(p.video.encoder, enc, "{id}");
            let r = eval(&m, &p, &c);
            let first = r.first_pass.clone().expect("两遍编码缺少第一遍");
            assert!(has(&first, &["-pass", "1", "-passlogfile", &prefix]), "{id}/{enc:?}");
            assert!(has(&r.args, &["-pass", "2", "-passlogfile", &prefix]), "{id}/{enc:?}");
            assert!(first.ends_with(&["-f".to_string(), "null".into(), "-".into()]));
            // 第一遍只编码视频
            assert_eq!(first.iter().filter(|a| *a == "-map").count(), 1);
            assert!(!first.iter().any(|a| a.starts_with("-c:a") || a.starts_with("-c:s")));
            // 两遍必须看到完全相同的帧：输入、编码参数、滤镜、帧率逐段一致
            let segs = build_first_pass(&m, &p, &c, Path::new(OUT)).unwrap();
            for kind in [SegmentKind::Input, SegmentKind::Video, SegmentKind::Filter, SegmentKind::Fps] {
                let a = segs.iter().find(|x| x.kind == kind).map(|x| x.args.join(" ")).unwrap_or_default();
                assert_eq!(a.replace("-pass 1", "-pass 2"), segment(&r, kind), "{id}/{enc:?} 的 {kind:?} 段两遍不一致");
            }
        }
    }
}

#[test]
fn bitrate_modes_shape_the_estimate_and_explanations() {
    let (m, c) = (media("m-drone"), caps());
    assert!(m.audio.is_empty(), "航拍样本应没有音轨，下面的体积算式不含音频");
    let mut p = run(&m, Scenario::Archive).plan;
    p.video.rate_control = RateControl::Bitrate { kbps: 20_000 };
    let r = eval(&m, &update_plan(p.clone(), &m, &c), &c);
    let expect = 20_000_000.0 * m.duration_sec / 8.0 * 1.01;
    assert!((r.estimate.size_min..=r.estimate.size_max).contains(&expect));
    assert!(r.estimate.size_max / r.estimate.size_min < 1.3, "按码率编码时体积范围应收窄");
    assert!(decision(&r, "码率").unwrap().value.contains("平均 20 Mbps"));

    p.video.rate_control = RateControl::TwoPass { kbps: 20_000 };
    let two = eval(&m, &update_plan(p, &m, &c), &c);
    assert!(decision(&two, "码率").unwrap().value.starts_with("两遍"));
    assert!((two.estimate.time_max_sec / r.estimate.time_max_sec - 1.7).abs() < 1e-9);
}

#[test]
fn bitrate_values_are_clamped_and_advice_follows_the_mode() {
    // 输入范围 100k–400000k：相机素材 598 Mbps，上限落在输入范围而不是源码率上
    let (camera, c) = (media("m-camera"), caps());
    let mut p = recommend_plan(&camera, Scenario::Archive, &c);
    p.video.rate_control = RateControl::Bitrate { kbps: 5 };
    assert_eq!(update_plan(p.clone(), &camera, &c).video.rate_control, RateControl::Bitrate { kbps: 100 });
    p.video.rate_control = RateControl::Capped { kbps: 9_000_000 };
    assert_eq!(update_plan(p, &camera, &c).video.rate_control, RateControl::Capped { kbps: 400_000 });
    let m = media("m-stream");
    let mut p = recommend_plan(&m, Scenario::Archive, &c);
    // 已高度压缩的片源按目标码率编码：建议调低目标码率而不是画质档位
    p.video.rate_control = RateControl::Bitrate { kbps: 8000 };
    let r = eval(&m, &update_plan(p, &m, &c), &c);
    assert!(r.not_worth_it.unwrap().contains("目标码率"));
}

#[test]
fn a_foreign_preset_is_replaced_by_the_encoders_default() {
    let (m, c) = (media("m-drone"), caps());
    let mut p = recommend_plan(&m, Scenario::Archive, &c);
    (p.video.encoder, p.video.encoder_auto) = (EncoderId::HevcQsv, false);
    // ultrafast 只有 x264 / x265 有，QSV 不认
    p.video.preset = "ultrafast".into();
    assert_eq!(update_plan(p.clone(), &m, &c).video.preset, "slow");
    // 两个编码器都有的档位保留用户的选择
    p.video.preset = "veryfast".into();
    assert_eq!(update_plan(p, &m, &c).video.preset, "veryfast");
}

/// 一行概括一份计划，供推荐快照使用
fn plan_line(p: &TranscodePlan) -> String {
    let v = &p.video;
    let video = if v.action == StreamAction::Copy {
        "copy".to_string()
    } else {
        let fps = match v.fps {
            FpsPolicy::Keep => "keep".to_string(),
            FpsPolicy::Cfr { fps } => format!("cfr {fps:.3}"),
            FpsPolicy::Cap { max } => format!("cap {max}"),
        };
        format!(
            "{} q{} {} {}bit {:?} {fps} {:?}{} dv:{:?}{}",
            v.encoder.name(),
            v.quality_value,
            v.preset,
            v.bit_depth,
            v.resolution,
            v.hdr_action,
            v.tonemap.map(|t| format!("({t:?})")).unwrap_or_default(),
            v.dovi,
            v.gop.map(|g| format!(" gop{g}")).unwrap_or_default(),
        )
    };
    let audio: Vec<String> = p
        .audio
        .iter()
        .map(|t| match t.codec {
            Some(c) => {
                format!("{}:{c:?}{}", t.source_index, t.bitrate_kbps.map(|b| format!("@{b}")).unwrap_or_default())
            }
            None => format!("{}:copy", t.source_index),
        })
        .collect();
    format!("{video} | {:?} | {} | subs:{:?}", p.container, audio.join(","), p.subtitles)
}

#[test]
fn scenario_recommendations_snapshot() {
    // 场景推荐一览：规则改动时在 diff 里直接看到哪些素材的哪些决定变了
    for (name, c) in [("dev", caps()), ("mac", mac())] {
        let mut lines = Vec::new();
        for m in samples() {
            for s in ALL {
                lines.push(format!("{:<9} {:<10} {}", m.id, format!("{s:?}"), plan_line(&recommend_plan(&m, s, &c))));
            }
        }
        insta::assert_snapshot!(format!("recommendations_{name}"), lines.join("\n"));
    }
}

#[test]
fn loudness_normalization_is_explained_and_only_touches_encoded_tracks() {
    let (m, c) = (media("m-bluray"), caps());
    // 收藏：全部原样复制，无法标准化
    let mut p = recommend_plan(&m, Scenario::Collection, &c);
    p.loudnorm = true;
    let r = eval(&m, &update_plan(p, &m, &c), &c);
    assert_eq!(decision(&r, "响度").unwrap().severity, Severity::Warn);
    assert!(r.loudness_measure.is_none());
    // 流媒体：E-AC-3 5.1 与兼容立体声两条重编码的轨各测一次
    let mut p = recommend_plan(&m, Scenario::Streaming, &c);
    p.loudnorm = true;
    let r = eval(&m, &update_plan(p, &m, &c), &c);
    let d = decision(&r, "响度").unwrap();
    assert_eq!(d.severity, Severity::Info);
    assert!(d.value.contains("-16 LUFS") && d.reason.contains("2 条"), "{d:?}");
    assert_eq!(r.loudness_measure.as_ref().map(Vec::len), Some(2));
    // 默认不开
    assert!(!recommend_plan(&m, Scenario::Mobile, &c).loudnorm);
}

// ───────────────── 界面语言 ─────────────────

/// 英文界面下引擎产出的说明里不能残留中文（素材自带的音轨、字幕标题与文件名除外）
#[test]
fn english_output_has_no_chinese_left() {
    let han = |t: &str| t.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c) || "，。；：（）「」、".contains(c));
    let variants = [caps(), mac(), with_tonemap(&[]), without(&[EncoderId::Libx265, EncoderId::Libsvtav1])];
    let mut checked = 0;
    for m in samples() {
        let own: Vec<String> = m
            .audio
            .iter()
            .filter_map(|a| a.title.clone())
            .chain(m.subtitle.iter().filter_map(|s| s.title.clone()))
            .collect();
        let clean = |t: &str| own.iter().fold(t.to_string(), |acc, o| acc.replace(o.as_str(), ""));
        for c in &variants {
            for s in ALL {
                let mut plans = vec![recommend_plan(&m, s, c)];
                // 每个一键修正之后的状态也要检查
                let first = evaluate(&m, &plans[0], c, Path::new(OUT), Lang::En);
                for f in first.fidelity.iter().flat_map(|f| f.fixes.iter()) {
                    plans.push(apply_fix_to_plan(plans[0].clone(), &f.id, &m, c));
                }
                for p in &plans {
                    let r = evaluate(&m, p, c, Path::new(OUT), Lang::En);
                    let zh = evaluate(&m, p, c, Path::new(OUT), Lang::ZhCn);
                    assert_eq!(r.decisions.len(), zh.decisions.len(), "两种语言的决策条数应一致");
                    for d in &r.decisions {
                        for t in [&d.field, &d.value, &d.reason] {
                            assert!(!han(&clean(t)), "{} / {s:?}：决策「{t}」", m.id);
                        }
                    }
                    for f in &r.fidelity {
                        assert!(!han(&f.label) && !han(&clean(&f.detail)), "{} / {s:?}：保真度 {f:?}", m.id);
                        assert!(f.fixes.iter().all(|x| !han(&x.label)), "{} / {s:?}：修正 {f:?}", m.id);
                    }
                    if let Some(w) = &r.not_worth_it {
                        assert!(!han(w), "{} / {s:?}：{w}", m.id);
                    }
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 100, "只检查了 {checked} 个组合");
}

#[test]
fn english_decisions_read_naturally() {
    let m = media("m-iphone");
    let c = caps();
    let p = apply_fix_to_plan(recommend_plan(&m, Scenario::Archive, &c), "dovi_preserve", &m, &c);
    let r = evaluate(&m, &p, &c, Path::new(OUT), Lang::En);
    let dv = r.decisions.iter().find(|d| d.field == "Dolby Vision").expect("有杜比视界的决策");
    assert_eq!(dv.value, "Keep Profile 8.4");
    let item = r.fidelity.iter().find(|f| f.kind == FidelityKind::DolbyVision).unwrap();
    assert_eq!(item.label, "Dolby Vision");
    assert!(item.detail.starts_with("Keeps Profile 8.4."), "{}", item.detail);
}
