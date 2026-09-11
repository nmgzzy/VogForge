//! 输出校验与保真度报告（设计文档 4.8，需求 F-7.1 / F-7.2）：转码后用 ffprobe 分析输出，与源和计划逐项比对。
//!
//! 两部分：基础完整性（时长、帧数、视频编码、固定帧率与音画对齐、色彩、音轨与字幕数量、复制轨的编码、章节），
//! 以及用户勾选要保留的每一项（杜比视界、HDR10 / HLG、HDR10+、无损音轨、全部音轨、全部字幕、10bit）。
//! 结论只看输出文件本身，不信任计划：计划说保留、输出里没有，照样标红。
//! HDR10 元数据按有理数求值后的数值比较（容差 1e-3），不比字符串——HEVC 与 AV1 的定点分母不同。

use crate::i18n::{Lang, pick};
use crate::model::{
    Container, DoviAction, FpsPolicy, HdrAction, HdrKind, MediaInfo, ReportItem, StreamAction, SubtitleMode,
    TranscodePlan,
};
use crate::pipeline::fps::fps_insight;
use crate::pipeline::text::{format_fps, plain, thousands};
use crate::tr;

/// 时长允许的误差（秒）
pub const DURATION_TOLERANCE: f64 = 0.5;

fn item(label: &str, expected: impl Into<String>, actual: impl Into<String>, ok: bool) -> ReportItem {
    ReportItem { label: label.into(), expected: expected.into(), actual: actual.into(), ok }
}

fn clock(sec: f64) -> String {
    let t = sec.max(0.0);
    let (h, m, s) = ((t / 3600.0) as u64, (t % 3600.0 / 60.0) as u64, t % 60.0);
    if h > 0 { format!("{h}:{m:02}:{s:05.2}") } else { format!("{m}:{s:05.2}") }
}

/// 计划输出的字幕条数
fn expected_subtitles(src: &MediaInfo, plan: &TranscodePlan) -> usize {
    let text = src.subtitle.iter().filter(|s| !s.image_based).count();
    match plan.subtitles {
        SubtitleMode::None => 0,
        SubtitleMode::TextOnly => text,
        SubtitleMode::All if plan.container == Container::Mkv => src.subtitle.len(),
        SubtitleMode::All => text,
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-3 * a.abs().max(b.abs()).max(1.0)
}

fn yes_no(lang: Lang, v: bool) -> &'static str {
    match (lang, v) {
        (Lang::En, true) => "yes",
        (Lang::En, false) => "no",
        (_, true) => "有",
        (_, false) => "无",
    }
}

pub fn report(src: &MediaInfo, plan: &TranscodePlan, out: &MediaInfo, lang: Lang) -> Vec<ReportItem> {
    let mut items = Vec::new();
    let l = |zh: &'static str, en: &'static str| pick(lang, zh, en);
    let (sv, ov) = (src.video.first(), out.video.first());
    let copy = plan.video.action == StreamAction::Copy;

    // ── 基础完整性 ──
    items.push(item(
        l("时长", "Duration"),
        tr!(lang, "{}（误差 < {} 秒）", "{} (within {} s)", clock(src.duration_sec), plain(DURATION_TOLERANCE)),
        clock(out.duration_sec),
        (out.duration_sec - src.duration_sec).abs() < DURATION_TOLERANCE,
    ));

    // 帧数：原样复制时一帧不差；重新编码且保持帧率时应与源一致（容差 1 帧）；转固定帧率时应等于目标帧率 × 时长
    // （补尾与取整，容差 2 帧）。源或输出没有帧数记录时不核对
    let expected_frames = match (sv, plan.video.fps) {
        (Some(s), FpsPolicy::Cfr { fps }) if !copy => Some((fps_insight(s, src.duration_sec, fps).target_frames, 2)),
        (Some(s), _) => s.frame_count.map(|n| (n, if copy { 0 } else { 1 })),
        (None, _) => None,
    };
    if let (Some((want, tol)), Some(got)) = (expected_frames, ov.and_then(|v| v.frame_count)) {
        let expected = if tol == 0 {
            thousands(want)
        } else {
            tr!(lang, "{}（误差 ≤ {} 帧）", "{} (within {} frames)", thousands(want), tol)
        };
        items.push(item(l("帧数", "Frames"), expected, thousands(got), got.abs_diff(want) <= tol));
    }

    let want = if copy {
        sv.map(|v| v.codec.clone()).unwrap_or_default()
    } else {
        format!("{:?}", plan.video.codec).to_lowercase()
    };
    let got = ov.map(|v| v.codec.clone()).unwrap_or_else(|| l("没有视频", "no video").into());
    let expected = if copy { tr!(lang, "{}（原样复制）", "{} (copied)", want) } else { want.clone() };
    items.push(item(l("视频编码", "Video codec"), expected, got.clone(), got == want));

    if let (FpsPolicy::Cfr { fps }, false) = (plan.video.fps, copy) {
        // 严格固定帧率：r_frame_rate 与 avg_frame_rate 一致、帧间隔恒定
        let ok =
            ov.is_some_and(|v| !v.is_vfr && close(v.fps_avg, v.fps_nominal) && (v.fps_avg - fps).abs() < fps * 0.01);
        let actual = ov.map_or(l("没有视频", "no video").into(), |v| {
            let vfr = if v.is_vfr { l("，帧间隔不均匀", ", uneven frame intervals") } else { "" };
            format!("{} / {} fps{vfr}", format_fps(v.fps_nominal), format_fps(v.fps_avg))
        });
        items.push(item(
            l("固定帧率", "Constant frame rate"),
            tr!(
                lang,
                "{} fps，r_frame_rate = avg_frame_rate",
                "{} fps, r_frame_rate = avg_frame_rate",
                format_fps(fps)
            ),
            actual,
            ok,
        ));
        // 音画时长差小于 1 帧
        let video = ov.and_then(|v| v.duration_sec);
        let audio = out
            .audio
            .iter()
            .filter_map(|a| a.duration_sec)
            .fold(None, |m: Option<f64>, d| Some(m.map_or(d, |m| m.max(d))));
        if let (Some(v), Some(a)) = (video, audio) {
            let diff = (v - a).abs();
            items.push(item(
                l("音画对齐", "A/V alignment"),
                tr!(lang, "时长差 < 1 帧（{} 秒）", "difference < 1 frame ({} s)", format!("{:.3}", 1.0 / fps)),
                format!("{diff:.3} s"),
                diff < 1.0 / fps,
            ));
        }
    }

    if let (Some(s), Some(o), false) = (sv, ov, copy) {
        if s.color.hdr_kind != HdrKind::None {
            // 原色 / 传输特性 / 矩阵三项都要对：矩阵错了，播放器按错误的系数换算颜色
            let want = if plan.video.hdr_action == HdrAction::Tonemap {
                ["bt709".to_string(), "bt709".to_string(), "bt709".to_string()]
            } else {
                [s.color.primaries.clone(), s.color.transfer.clone(), s.color.space.clone()]
            };
            let got = [o.color.primaries.clone(), o.color.transfer.clone(), o.color.space.clone()];
            items.push(item(l("色彩标签", "Color tags"), want.join(" / "), got.join(" / "), got == want));
        }
    }

    let n = plan.audio.len();
    items.push(item(
        l("音轨数", "Audio tracks"),
        tr!(lang, "{} 条", "{}", n),
        tr!(lang, "{} 条", "{}", out.audio.len()),
        out.audio.len() == n,
    ));

    // 原样复制的音轨：输出里对应位置的编码必须与源一致
    let copied: Vec<(usize, String)> = plan
        .audio
        .iter()
        .enumerate()
        .filter(|(_, t)| t.action == StreamAction::Copy)
        .filter_map(|(i, t)| src.audio.iter().find(|a| a.index == t.source_index).map(|a| (i, a.codec.clone())))
        .collect();
    if !copied.is_empty() {
        let bad: Vec<String> = copied
            .iter()
            .filter(|(i, codec)| out.audio.get(*i).is_none_or(|o| &o.codec != codec))
            .map(|(i, codec)| format!("#{} {codec} → {}", i + 1, out.audio.get(*i).map_or("-", |o| o.codec.as_str())))
            .collect();
        items.push(item(
            l("复制的音轨", "Copied audio"),
            tr!(lang, "{} 条编码与源一致", "{} track(s) match the source", copied.len()),
            if bad.is_empty() { l("一致", "match").to_string() } else { bad.join(", ") },
            bad.is_empty(),
        ));
    }

    let subs = expected_subtitles(src, plan);
    if subs > 0 || !out.subtitle.is_empty() {
        let n = out.subtitle.len();
        items.push(item(
            l("字幕数", "Subtitles"),
            tr!(lang, "{} 条", "{}", subs),
            tr!(lang, "{} 条", "{}", n),
            n == subs,
        ));
    }
    if src.chapters > 0 {
        items.push(item(
            l("章节", "Chapters"),
            tr!(lang, "{} 个", "{}", src.chapters),
            tr!(lang, "{} 个", "{}", out.chapters),
            out.chapters == src.chapters,
        ));
    }

    // ── 用户勾选要保留的项 ──
    let want_it = &plan.fidelity;
    if let (Some(dv), true) = (sv.and_then(|v| v.dolby_vision.as_ref()), want_it.dolby_vision) {
        let kept = copy || plan.video.dovi == DoviAction::Preserve;
        let expected =
            tr!(lang, "Profile {} 配置记录与逐帧 RPU", "Profile {} configuration record and per-frame RPU", dv.profile);
        let (actual, ok) = match ov.and_then(|v| v.dolby_vision.as_ref()) {
            Some(o) => (
                tr!(lang, "Profile {}，RPU {}", "Profile {}, RPU {}", o.profile, yes_no(lang, o.rpu)),
                kept && o.rpu && o.profile == dv.profile || (o.rpu && dv.has_enhancement_layer && copy),
            ),
            None if !kept && dv.has_enhancement_layer => (
                l("未保留：双层 Profile 7 无法重编码保留，需原样封装", "not kept: dual-layer Profile 7 needs remux")
                    .into(),
                false,
            ),
            None if !kept => (l("未保留：计划没有保留杜比视界", "not kept: the plan drops Dolby Vision").into(), false),
            None => (l("没有杜比视界配置记录", "no Dolby Vision record").into(), false),
        };
        items.push(item(l("杜比视界", "Dolby Vision"), expected, actual, ok));
    }

    if let (Some(s), true) = (sv, want_it.hdr10) {
        let tonemapped = !copy && plan.video.hdr_action == HdrAction::Tonemap;
        match s.color.hdr_kind {
            HdrKind::Hdr10 => {
                let md = s.hdr10.as_ref();
                let expected = match md {
                    Some(m) => tr!(
                        lang,
                        "母版 {} nits{}",
                        "mastering {} nits{}",
                        plain(m.max_luminance),
                        m.max_cll.map_or(String::new(), |c| format!(" / MaxCLL {}", plain(c)))
                    ),
                    None => l("HDR10（PQ）", "HDR10 (PQ)").into(),
                };
                let o = ov.and_then(|v| v.hdr10.as_ref().map(|m| (v, m)));
                let (actual, ok) = if tonemapped {
                    (l("未保留：已转为 SDR", "not kept: converted to SDR").into(), false)
                } else {
                    match (o, md) {
                        (Some((v, om)), Some(m)) => {
                            // 源里有的每一项输出都要有且相等：母版色域、亮度范围、MaxCLL、MaxFALL
                            let kept = |src: Option<f64>, out: Option<f64>| {
                                src.is_none() || out.zip(src).is_some_and(|(a, b)| close(a, b))
                            };
                            let same = om.mastering_primaries == m.mastering_primaries
                                && close(om.max_luminance, m.max_luminance)
                                && close(om.min_luminance, m.min_luminance)
                                && kept(m.max_cll, om.max_cll)
                                && kept(m.max_fall, om.max_fall);
                            let text = tr!(
                                lang,
                                "母版 {} nits{}",
                                "mastering {} nits{}",
                                plain(om.max_luminance),
                                om.max_cll.map_or(String::new(), |c| format!(" / MaxCLL {}", plain(c)))
                            );
                            (text, same && v.color.hdr_kind == HdrKind::Hdr10)
                        }
                        (Some((v, _)), None) => ("HDR10".into(), v.color.hdr_kind == HdrKind::Hdr10),
                        (None, _) => (l("没有 HDR10 元数据", "no HDR10 metadata").into(), false),
                    }
                };
                items.push(item("HDR10", expected, actual, ok));
            }
            HdrKind::Hlg => {
                let got = ov.map(|v| v.color.transfer.clone()).unwrap_or_default();
                let actual = if tonemapped {
                    l("未保留：已转为 SDR", "not kept: converted to SDR").into()
                } else {
                    got.clone()
                };
                items.push(item("HLG", "arib-std-b67", actual, !tonemapped && got == "arib-std-b67"));
            }
            _ => {}
        }
    }

    // HDR10+ 的逐帧动态元数据只有原样封装能保留；重编码会丢，照实标红
    if want_it.hdr10plus && sv.is_some_and(|v| v.hdr10plus) {
        let kept = ov.is_some_and(|v| v.hdr10plus);
        let actual = if kept {
            l("首帧带 HDR10+ 动态元数据", "HDR10+ dynamic metadata on the first frame").to_string()
        } else if copy {
            l("没有 HDR10+ 动态元数据", "no HDR10+ dynamic metadata").into()
        } else {
            l("未保留：重编码无法透传 HDR10+", "not kept: re-encoding cannot pass HDR10+ through").into()
        };
        items.push(item("HDR10+", l("逐帧动态元数据", "per-frame dynamic metadata"), actual, kept));
    }

    if want_it.lossless {
        let hq: Vec<_> = src.audio.iter().filter(|a| a.lossless || a.atmos).collect();
        if !hq.is_empty() {
            let missing: Vec<String> = hq
                .iter()
                .filter(|a| {
                    !plan.audio.iter().enumerate().any(|(i, t)| {
                        t.source_index == a.index
                            && t.action == StreamAction::Copy
                            && out.audio.get(i).is_some_and(|o| o.codec == a.codec && (!a.atmos || o.atmos))
                    })
                })
                .map(|a| if a.atmos { format!("{} Atmos", a.codec) } else { a.codec.clone() })
                .collect();
            items.push(item(
                l("全景声与无损音轨", "Atmos & lossless audio"),
                tr!(lang, "{} 条原样保留", "{} kept as-is", hq.len()),
                if missing.is_empty() {
                    l("原样保留", "kept as-is").to_string()
                } else {
                    tr!(lang, "未保留：{}", "not kept: {}", missing.join(", "))
                },
                missing.is_empty(),
            ));
        }
    }

    if want_it.all_audio && src.audio.len() > 1 {
        let mapped = src.audio.iter().filter(|a| plan.audio.iter().any(|t| t.source_index == a.index)).count();
        items.push(item(
            l("全部音轨", "All audio tracks"),
            tr!(lang, "源的 {} 条都在", "all {} source tracks", src.audio.len()),
            tr!(lang, "带了 {} 条，输出共 {} 条", "{} carried, {} in output", mapped, out.audio.len()),
            mapped == src.audio.len() && out.audio.len() >= src.audio.len(),
        ));
    }
    if want_it.all_subtitles && !src.subtitle.is_empty() {
        items.push(item(
            l("全部字幕", "All subtitles"),
            tr!(lang, "{} 条", "{}", src.subtitle.len()),
            tr!(lang, "{} 条", "{}", out.subtitle.len()),
            out.subtitle.len() == src.subtitle.len(),
        ));
    }
    if want_it.ten_bit && !copy {
        let depth = ov.map_or(0, |v| v.bit_depth);
        items.push(item(l("10bit 色深", "10-bit"), "10bit", format!("{depth}bit"), depth == 10));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Capabilities, EnvStatus, Scenario};
    use crate::pipeline::recommend_plan;

    fn caps() -> Capabilities {
        Capabilities::placeholder(EnvStatus::Probing, "")
    }

    fn media(json_video: &str, audio: &[&str], duration: f64) -> MediaInfo {
        let audio: Vec<String> = audio
            .iter()
            .enumerate()
            .map(|(i, a)| {
                format!(
                    r#"{{"index":{},"codec":"{a}","channels":6,"channelLayout":"5.1","sampleRate":48000,"isDefault":true,
                    "lossless":{},"atmos":false,"dtsX":false,"durationSec":{duration}}}"#,
                    i + 1,
                    a == &"truehd"
                )
            })
            .collect();
        serde_json::from_str(&format!(
            r#"{{"id":"m","path":"/a/b.mkv","name":"b.mkv","container":"matroska","durationSec":{duration},"sizeBytes":1000,
            "bitrate":8000000,"video":[{json_video}],"audio":[{}],"subtitle":[],"chapters":0,"attachments":0,
            "sourceHint":"unknown"}}"#,
            audio.join(",")
        ))
        .unwrap()
    }

    fn video(codec: &str, hdr: &str, extra: &str) -> String {
        let color = match hdr {
            "hdr10" => {
                r#""primaries":"bt2020","transfer":"smpte2084","space":"bt2020nc","range":"tv","hdrKind":"hdr10""#
            }
            "sdr" => r#""primaries":"bt709","transfer":"bt709","space":"bt709","range":"tv","hdrKind":"none""#,
            _ => r#""primaries":"bt2020","transfer":"arib-std-b67","space":"bt2020nc","range":"tv","hdrKind":"hlg""#,
        };
        format!(
            r#"{{"index":0,"codec":"{codec}","width":3840,"height":2160,"fpsAvg":24,"fpsNominal":24,"isVfr":false,
            "bitDepth":10,"pixFmt":"yuv420p10le","color":{{{color}}},"hdr10plus":false,"rotation":0,"durationSec":10{extra}}}"#
        )
    }

    const MD_HEVC: &str = r#","hdr10":{"maxLuminance":1000.0,"minLuminance":0.005,"maxCll":1000.0,"maxFall":400.0,"masteringPrimaries":"p3"}"#;
    /// AV1 定点分母不同，求值后的数值应视为相同
    const MD_AV1: &str = r#","hdr10":{"maxLuminance":1000.0000001,"minLuminance":0.00500002,"maxCll":1000.0,"maxFall":400.0,"masteringPrimaries":"p3"}"#;
    const DV: &str = r#","dolbyVision":{"profile":8,"blCompatId":1,"hasEnhancementLayer":false,"rpu":true}"#;

    fn labels(r: &[ReportItem], ok: bool) -> Vec<&str> {
        r.iter().filter(|i| i.ok == ok).map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn hdr10_and_dolby_vision_kept_across_codecs() {
        let src = media(&video("hevc", "hdr10", &format!("{MD_HEVC}{DV}")), &["truehd"], 10.0);
        let mut plan = recommend_plan(&src, Scenario::Archive, &caps());
        plan.fidelity.dolby_vision = true;
        plan.fidelity.hdr10 = true;
        plan.fidelity.lossless = true;
        plan.fidelity.ten_bit = true;
        // 归档会给 TrueHD 追加一条兼容立体声
        let out = media(&video("hevc", "hdr10", &format!("{MD_AV1}{DV}")), &["truehd", "aac"], 10.1);
        let r = report(&src, &plan, &out, Lang::ZhCn);
        assert!(labels(&r, false).is_empty(), "{r:#?}");
        for l in ["杜比视界", "HDR10", "全景声与无损音轨", "10bit 色深", "色彩标签", "复制的音轨"]
        {
            assert!(r.iter().any(|i| i.label == l), "缺少 {l}");
        }
    }

    #[test]
    fn missing_metadata_rpu_and_lossy_tracks_are_red() {
        let src = media(&video("hevc", "hdr10", &format!("{MD_HEVC}{DV}")), &["truehd"], 10.0);
        let mut plan = recommend_plan(&src, Scenario::Archive, &caps());
        (plan.fidelity.dolby_vision, plan.fidelity.hdr10, plan.fidelity.lossless) = (true, true, true);
        plan.video.dovi = DoviAction::Preserve;
        // 输出：配置记录在但没有 RPU、没有 HDR10 元数据、TrueHD 被转成了 AC-3
        let out = media(
            &video("hevc", "hdr10", r#","dolbyVision":{"profile":8,"blCompatId":1,"hasEnhancementLayer":false}"#),
            &["ac3", "aac"],
            10.0,
        );
        let r = report(&src, &plan, &out, Lang::ZhCn);
        assert_eq!(labels(&r, false), ["复制的音轨", "杜比视界", "HDR10", "全景声与无损音轨"]);
        let dv = r.iter().find(|i| i.label == "杜比视界").unwrap();
        assert!(dv.actual.contains("RPU 无"), "{}", dv.actual);
    }

    #[test]
    fn tone_mapped_output_reports_hdr_as_not_kept_and_checks_bt709() {
        let src = media(&video("hevc", "hlg", ""), &["aac"], 10.0);
        let mut plan = recommend_plan(&src, Scenario::Mobile, &caps());
        plan.video.hdr_action = HdrAction::Tonemap;
        plan.fidelity.hdr10 = true;
        let out = media(&video("h264", "sdr", ""), &["aac"], 10.0);
        let r = report(&src, &plan, &out, Lang::En);
        let hlg = r.iter().find(|i| i.label == "HLG").unwrap();
        assert!(!hlg.ok && hlg.actual.contains("converted to SDR"));
        let color = r.iter().find(|i| i.label == "Color tags").unwrap();
        assert!(color.ok, "{color:?}");
    }

    #[test]
    fn color_matrix_max_fall_and_primaries_must_match_too() {
        let src = media(&video("hevc", "hdr10", MD_HEVC), &["aac"], 10.0);
        let mut plan = recommend_plan(&src, Scenario::Archive, &caps());
        plan.fidelity.hdr10 = true;
        let red = |out: &MediaInfo| {
            labels(&report(&src, &plan, out, Lang::ZhCn), false).into_iter().map(String::from).collect::<Vec<_>>()
        };
        // 矩阵写成了 bt709
        let mut out = media(&video("hevc", "hdr10", MD_HEVC), &["aac"], 10.0);
        out.video[0].color.space = "bt709".into();
        assert_eq!(red(&out), ["色彩标签"]);
        // MaxFALL 丢了、母版色域变了
        let mut out = media(&video("hevc", "hdr10", MD_HEVC), &["aac"], 10.0);
        out.video[0].hdr10.as_mut().unwrap().max_fall = None;
        assert_eq!(red(&out), ["HDR10"]);
        let mut out = media(&video("hevc", "hdr10", MD_HEVC), &["aac"], 10.0);
        out.video[0].hdr10.as_mut().unwrap().mastering_primaries = crate::model::MasteringPrimaries::Bt2020;
        assert_eq!(red(&out), ["HDR10"]);
    }

    #[test]
    fn frame_counts_follow_the_plan() {
        // 源 240 帧（24 fps × 10 秒）
        let src = media(&video("h264", "sdr", r#","frameCount":240"#), &["aac"], 10.0);
        let keep = recommend_plan(&src, Scenario::Archive, &caps());
        let with = |n: u64| {
            let mut o = media(&video("hevc", "sdr", ""), &["aac"], 10.0);
            o.video[0].frame_count = Some(n);
            o
        };
        let frames = |r: &[ReportItem]| r.iter().find(|i| i.label == "帧数").map(|i| i.ok);
        assert_eq!(frames(&report(&src, &keep, &with(240), Lang::ZhCn)), Some(true));
        assert_eq!(frames(&report(&src, &keep, &with(239), Lang::ZhCn)), Some(true), "重编码容差 1 帧");
        assert_eq!(frames(&report(&src, &keep, &with(236), Lang::ZhCn)), Some(false), "丢了 4 帧");
        // 原样复制一帧不差
        let mut copy = keep.clone();
        copy.video.action = StreamAction::Copy;
        assert_eq!(frames(&report(&src, &copy, &with(239), Lang::ZhCn)), Some(false));
        // 转 30 fps：目标 300 帧
        let mut cfr = keep.clone();
        cfr.video.fps = FpsPolicy::Cfr { fps: 30.0 };
        assert_eq!(frames(&report(&src, &cfr, &with(301), Lang::ZhCn)), Some(true));
        assert_eq!(frames(&report(&src, &cfr, &with(240), Lang::ZhCn)), Some(false));
        // 输出没有帧数记录时不核对
        assert_eq!(frames(&report(&src, &keep, &media(&video("hevc", "sdr", ""), &["aac"], 10.0), Lang::ZhCn)), None);
    }

    #[test]
    fn hdr10plus_is_red_when_re_encoding_drops_it() {
        let src = media(
            &video("hevc", "hdr10", MD_HEVC).replace(r#""hdr10plus":false"#, r#""hdr10plus":true"#),
            &["aac"],
            10.0,
        );
        let mut plan = recommend_plan(&src, Scenario::Archive, &caps());
        plan.fidelity.hdr10plus = true;
        let out = media(&video("hevc", "hdr10", MD_HEVC), &["aac"], 10.0);
        let r = report(&src, &plan, &out, Lang::En);
        let item = r.iter().find(|i| i.label == "HDR10+").expect("勾选了 HDR10+ 就要核对");
        assert!(!item.ok && item.actual.contains("re-encoding"), "{item:?}");
        // 原样封装保留下来
        plan.video.action = StreamAction::Copy;
        let kept = media(
            &video("hevc", "hdr10", MD_HEVC).replace(r#""hdr10plus":false"#, r#""hdr10plus":true"#),
            &["aac"],
            10.0,
        );
        let r = report(&src, &plan, &kept, Lang::En);
        assert!(r.iter().any(|i| i.label == "HDR10+" && i.ok), "{r:#?}");
    }

    #[test]
    fn strict_cfr_and_av_alignment() {
        let src = media(&video("h264", "sdr", "").replace(r#""isVfr":false"#, r#""isVfr":true"#), &["aac"], 10.0);
        let mut plan = recommend_plan(&src, Scenario::Editing, &caps());
        plan.video.fps = FpsPolicy::Cfr { fps: 24.0 };
        let good = media(&video("h264", "sdr", ""), &["aac"], 10.0);
        let r = report(&src, &plan, &good, Lang::ZhCn);
        assert!(r.iter().any(|i| i.label == "固定帧率" && i.ok) && r.iter().any(|i| i.label == "音画对齐" && i.ok));
        // 帧间隔不均匀、音频长了 0.1 秒（大于 1/24）
        let mut bad = media(&video("h264", "sdr", "").replace(r#""isVfr":false"#, r#""isVfr":true"#), &["aac"], 10.1);
        bad.video[0].duration_sec = Some(10.0);
        let r = report(&src, &plan, &bad, Lang::ZhCn);
        assert_eq!(labels(&r, false), ["固定帧率", "音画对齐"]);
        assert_eq!(clock(3725.5), "1:02:05.50");
    }
}
