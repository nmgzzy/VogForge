//! 输出校验（设计文档 4.8，需求 F-7.1）：转码后用 ffprobe 分析输出，与源和计划逐项比对。
//!
//! 这里是基础完整性：时长、视频编码、音轨与字幕数量、章节、固定帧率。保真度逐项核对
//! （HDR10 元数据、杜比视界、色彩标签等）在此基础上扩展。

use crate::model::{Container, FpsPolicy, MediaInfo, ReportItem, StreamAction, SubtitleMode, TranscodePlan};
use crate::pipeline::text::{format_fps, plain};

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

pub fn basic_report(src: &MediaInfo, plan: &TranscodePlan, out: &MediaInfo) -> Vec<ReportItem> {
    let mut items = Vec::new();
    let diff = (out.duration_sec - src.duration_sec).abs();
    items.push(item(
        "时长",
        format!("{}（误差 < {} 秒）", clock(src.duration_sec), plain(DURATION_TOLERANCE)),
        clock(out.duration_sec),
        diff < DURATION_TOLERANCE,
    ));

    let copy = plan.video.action == StreamAction::Copy;
    let want = if copy {
        src.video.first().map(|v| v.codec.clone()).unwrap_or_default()
    } else {
        format!("{:?}", plan.video.codec).to_lowercase()
    };
    let got = out.video.first().map(|v| v.codec.clone()).unwrap_or_else(|| "无视频".into());
    items.push(item(
        "视频编码",
        if copy { format!("{want}（原样复制）") } else { want.clone() },
        got.clone(),
        got == want,
    ));

    if let (FpsPolicy::Cfr { fps }, false) = (plan.video.fps, copy) {
        let v = out.video.first();
        let ok = v.is_some_and(|v| !v.is_vfr && (v.fps_avg - fps).abs() < fps * 0.01);
        let actual = v.map_or("无视频".into(), |v| {
            format!("{} fps{}", format_fps(v.fps_avg), if v.is_vfr { "，仍是可变帧率" } else { "" })
        });
        items.push(item("固定帧率", format!("{} fps", format_fps(fps)), actual, ok));
    }

    let audio = plan.audio.len();
    items.push(item("音轨数", format!("{audio} 条"), format!("{} 条", out.audio.len()), out.audio.len() == audio));

    let subs = expected_subtitles(src, plan);
    if subs > 0 || !out.subtitle.is_empty() {
        let n = out.subtitle.len();
        items.push(item("字幕数", format!("{subs} 条"), format!("{n} 条"), n == subs));
    }
    if src.chapters > 0 {
        items.push(item(
            "章节",
            format!("{} 个", src.chapters),
            format!("{} 个", out.chapters),
            out.chapters == src.chapters,
        ));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Capabilities, EnvStatus, Scenario};
    use crate::pipeline::recommend_plan;

    fn media(duration: f64, codec: &str, audio: usize, vfr: bool, fps: f64) -> MediaInfo {
        let track = r#"{"index":1,"codec":"aac","channels":2,"channelLayout":"stereo","sampleRate":48000,
            "isDefault":true,"lossless":false,"atmos":false,"dtsX":false}"#;
        let audio = vec![track; audio].join(",");
        serde_json::from_str(&format!(
            r#"{{"id":"m","path":"/a/b.mp4","name":"b.mp4","container":"mov","durationSec":{duration},"sizeBytes":1000,
            "bitrate":8000000,"video":[{{"index":0,"codec":"{codec}","width":1920,"height":1080,"fpsAvg":{fps},
            "fpsNominal":30,"isVfr":{vfr},"bitDepth":8,"pixFmt":"yuv420p","color":{{"primaries":"bt709",
            "transfer":"bt709","space":"bt709","range":"tv","hdrKind":"none"}},"hdr10plus":false,"rotation":0}}],
            "audio":[{audio}],"subtitle":[],"chapters":0,"attachments":0,"sourceHint":"unknown"}}"#
        ))
        .unwrap()
    }

    #[test]
    fn matching_output_passes_every_item() {
        let src = media(12.0, "h264", 1, false, 30.0);
        let plan = recommend_plan(&src, Scenario::Archive, &Capabilities::placeholder(EnvStatus::Probing, ""));
        let out = media(12.2, "hevc", 1, false, 30.0);
        let r = basic_report(&src, &plan, &out);
        assert!(r.iter().all(|i| i.ok), "{r:?}");
        assert_eq!(r.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["时长", "视频编码", "音轨数"]);
    }

    #[test]
    fn mismatches_are_reported_with_both_sides() {
        let src = media(12.0, "h264", 2, true, 29.4);
        let mut plan = recommend_plan(&src, Scenario::Editing, &Capabilities::placeholder(EnvStatus::Probing, ""));
        plan.video.fps = FpsPolicy::Cfr { fps: 30.0 };
        let out = media(11.0, "h264", 1, true, 29.4);
        let r = basic_report(&src, &plan, &out);
        let bad: Vec<&str> = r.iter().filter(|i| !i.ok).map(|i| i.label.as_str()).collect();
        assert_eq!(bad, ["时长", "固定帧率", "音轨数"]);
        let fps = r.iter().find(|i| i.label == "固定帧率").unwrap();
        assert!(fps.actual.contains("仍是可变帧率"), "{}", fps.actual);
        assert_eq!(clock(3725.5), "1:02:05.50");
    }
}
