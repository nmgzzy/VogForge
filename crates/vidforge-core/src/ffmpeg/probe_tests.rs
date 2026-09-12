//! probe.rs 的测试。fixture 说明见 tests/fixtures/probe/README.md。

use super::*;
use crate::ffmpeg::exec::ExecOutput;
use std::io;
use std::sync::Mutex;

fn fixture(name: &str) -> ProbeOutputs {
    let dir = format!("{}/tests/fixtures/probe/{name}", env!("CARGO_MANIFEST_DIR"));
    let read = |f: &str| std::fs::read_to_string(format!("{dir}/{f}")).unwrap_or_default();
    ProbeOutputs { info_json: read("info.json"), frame_json: read("frame.json"), packets_csv: read("packets.csv") }
}

fn media(name: &str, path: &str) -> MediaInfo {
    parse_media(path, None, &fixture(name)).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

#[test]
fn rational_values() {
    assert_eq!(rational("10000000/10000"), Some(1000.0));
    assert_eq!(rational("256000/256"), Some(1000.0), "AV1 的分母是 256，求值后与 HEVC 相同");
    assert_eq!(rational("1000/1"), Some(1000.0));
    assert!((rational("30000/1001").unwrap() - 29.97).abs() < 0.001);
    assert_eq!(rational("0/0"), None);
    assert_eq!(rational("N/A"), None);
    assert_eq!(rational("12.5"), Some(12.5));
}

#[test]
fn hdr10_hevc_and_av1_evaluate_to_the_same_nits() {
    let hevc = media("hdr10-hevc", "a.mkv");
    let av1 = media("hdr10-av1", "b.mkv");
    for m in [&hevc, &av1] {
        let v = &m.video[0];
        let md = v.hdr10.as_ref().expect("应读出 HDR10 元数据");
        assert!((md.max_luminance - 1000.0).abs() < 1e-6, "{}: {}", m.name, md.max_luminance);
        // AV1 的最低亮度是 Q18.14 定点（分母 16384），0.0001 nits 会量化成 2/16384 ≈ 0.000122
        assert!((md.min_luminance - 0.0001).abs() < 5e-5, "{}: {}", m.name, md.min_luminance);
        assert_eq!(md.max_cll, Some(1000.0));
        assert_eq!(md.max_fall, Some(400.0));
        assert_eq!(md.mastering_primaries, MasteringPrimaries::P3);
    }
    assert_eq!(av1.video[0].color.hdr_kind, HdrKind::Hdr10);
    assert_eq!(av1.video[0].bit_depth, 10);
    // x265 只把色彩写进码流 VUI、容器没记录：流级字段是 unknown，要从首帧读出 PQ，否则会误判成 SDR
    let info: serde_json::Value = serde_json::from_str(&fixture("hdr10-hevc").info_json).unwrap();
    assert!(info["streams"][0]["color_transfer"].is_null());
    assert_eq!(hevc.video[0].color.transfer, "smpte2084");
    assert_eq!(hevc.video[0].color.primaries, "bt2020");
    assert_eq!(hevc.video[0].color.hdr_kind, HdrKind::Hdr10);
}

#[test]
fn hlg_phone_clip_with_rotation_and_apple_tags() {
    let m = media("hlg-iphone", "D:/clips/IMG_0001.MOV");
    let v = &m.video[0];
    assert_eq!(v.color.hdr_kind, HdrKind::Hlg);
    assert_eq!(v.color.primaries, "bt2020");
    assert_eq!(v.rotation, -90);
    assert_eq!(v.bit_depth, 10);
    assert_eq!(m.source_hint, SourceHint::Iphone);
    assert_eq!(m.device.as_deref(), Some("Apple iPhone 15 Pro"));
    assert_eq!(m.container, "mov");
    assert!(v.hdr10.is_none(), "HLG 没有 MDCV/CLL");
}

#[test]
fn iphone_dolby_vision_84() {
    let m = media("iphone-dv84", "D:/素材/2026-08 京都/IMG_4521.MOV");
    let v = &m.video[0];
    let dv = v.dolby_vision.as_ref().expect("应读出杜比视界配置");
    assert_eq!((dv.profile, dv.bl_compat_id, dv.has_enhancement_layer), (8, 4, false));
    assert!(dv.rpu, "首帧带 RPU");
    assert_eq!(dv.el_type, None);
    assert!(v.is_vfr, "r_frame_rate 30 与平均 29.41 相差约 2%，判据 1 命中");
    assert_eq!(v.frame_count, Some(3959));
    assert_eq!(m.device.as_deref(), Some("Apple iPhone 16 Pro"));
    assert!((m.duration_sec - 134.6).abs() < 1e-6);
}

#[test]
fn bluray_remux_p7_fel_atmos_pgs() {
    let path = "E:/Movies/Dune.Part.Two.2024.2160p.UHD.BluRay.REMUX.DV.HDR.HEVC.TrueHD.Atmos.7.1.mkv";
    let m = media("bluray-p7", path);
    let v = &m.video[0];
    let dv = v.dolby_vision.as_ref().unwrap();
    assert_eq!((dv.profile, dv.has_enhancement_layer, dv.el_type), (7, true, Some(ElType::Fel)));
    let md = v.hdr10.as_ref().unwrap();
    assert!((md.max_luminance - 4000.0).abs() < 1e-6);
    assert_eq!(md.max_cll, Some(1017.0));
    assert_eq!(v.color.hdr_kind, HdrKind::Hdr10);
    assert_eq!(v.bitrate, Some(58_732_001), "MKV 的码率在 BPS 统计标签里");
    assert_eq!(v.frame_count, Some(209_870));

    assert_eq!(m.audio.len(), 3);
    let thd = &m.audio[0];
    assert!(thd.atmos && thd.lossless && thd.is_default);
    assert_eq!(thd.title.as_deref(), Some("TrueHD 7.1 Atmos"));
    let dts = &m.audio[1];
    assert!(dts.lossless && !dts.atmos && !dts.dts_x, "DTS-HD MA 是无损");
    assert!(!m.audio[2].lossless);

    assert_eq!(m.subtitle.iter().filter(|s| s.image_based).count(), 2);
    assert!(!m.subtitle[2].image_based);
    assert_eq!(m.chapters, 12);
    assert_eq!(m.attachments, 1);
    assert_eq!(m.source_hint, SourceHint::Bluray);
    assert_eq!(m.size_bytes, 65_850_000_000);
}

#[test]
fn vfr_mp4_hits_the_field_criterion() {
    let v = &media("vfr-mp4", "vfr.mp4").video[0];
    assert_eq!(v.fps_nominal, 30.0);
    assert!(v.fps_avg < 17.0);
    assert!(v.is_vfr);
}

#[test]
fn vfr_mkv_needs_the_timestamp_criterion() {
    let m = media("vfr-mkv", "vfr.mkv");
    let v = &m.video[0];
    // MKV 的两个帧率字段完全相同，判据 1 失效
    assert_eq!(v.fps_nominal, v.fps_avg);
    assert!(v.is_vfr, "必须靠包时间戳间隔判出");
}

#[test]
fn cfr_23976_mkv_with_millisecond_rounding_is_not_vfr() {
    let v = &media("cfr2398-mkv", "movie.mkv").video[0];
    assert!((v.fps_nominal - 23.976).abs() < 0.001);
    assert!(!v.is_vfr, "41/42 ms 的取整抖动不能被当成可变帧率");
}

#[test]
fn pts_criterion_edge_cases() {
    let cfr: Vec<f64> = (0..120).map(|i| f64::from(i) / 30.0).collect();
    assert!(!pts_is_vfr(&cfr));
    // 一次偶发丢帧不算 VFR
    let mut one_gap = cfr.clone();
    one_gap.remove(50);
    assert!(!pts_is_vfr(&one_gap));
    // B 帧导致包时间戳乱序，排序后仍是固定帧率
    let mut reordered = cfr.clone();
    reordered.swap(3, 4);
    reordered.swap(10, 12);
    assert!(!pts_is_vfr(&reordered));
    assert!(!pts_is_vfr(&[0.0, 0.1]), "样本太少不下结论");
}

#[test]
fn remux_like_mkv_tracks_chapters_attachments() {
    let m = media("remux-mkv", "remux.mkv");
    assert_eq!(m.container, "mkv");
    assert_eq!(m.audio.len(), 3);
    assert!(m.audio[0].lossless, "TrueHD 是无损");
    assert_eq!(m.audio[0].codec, "truehd");
    assert!(m.audio[2].lossless, "FLAC 是无损");
    assert!(!m.audio[1].lossless);
    assert_eq!(m.audio[2].language.as_deref(), Some("jpn"));
    assert_eq!(m.subtitle.len(), 1);
    assert!(!m.subtitle[0].image_based);
    assert_eq!(m.chapters, 2);
    assert_eq!(m.attachments, 1);
    assert!(m.duration_sec > 2.9);
}

#[test]
fn camera_make_tags() {
    let m = media("camera-mp4", "C0001.MP4");
    assert_eq!(m.source_hint, SourceHint::Camera);
    assert_eq!(m.device.as_deref(), Some("Sony ILCE-7M4"));
    assert_eq!(m.video[0].color.hdr_kind, HdrKind::None);
}

#[test]
fn source_hint_from_file_name() {
    let name = |n: &str| media("cfr2398-mkv", n).source_hint;
    assert_eq!(name("Show.S01E01.1080p.NF.WEB-DL.DDP5.1.H.264.mkv"), SourceHint::Streaming);
    assert_eq!(name("Movie.2019.1080p.BluRay.REMUX.mkv"), SourceHint::Bluray);
    assert_eq!(name("录屏 2026-09-11.mkv"), SourceHint::Screen);
    assert_eq!(name("holiday.mkv"), SourceHint::Unknown);
}

#[test]
fn ids_are_stable_and_case_insensitive_on_windows() {
    assert_eq!(media_id("D:/a/B.mov"), media_id("D:/a/B.mov"));
    assert_ne!(media_id("D:/a/B.mov"), media_id("D:/a/C.mov"));
    if cfg!(windows) {
        assert_eq!(media_id(r"D:\A\b.MOV"), media_id("d:/a/B.mov"));
    }
}

#[test]
fn missing_fields_and_na_do_not_panic() {
    let outs = ProbeOutputs {
        info_json: r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264","r_frame_rate":"0/0",
            "avg_frame_rate":"N/A","bit_rate":"N/A","tags":{}}],"format":{"duration":"N/A","format_name":"mov,mp4"}}"#
            .into(),
        ..Default::default()
    };
    let m = parse_media("x.bin", Some(1234), &outs).unwrap();
    let v = &m.video[0];
    assert_eq!((v.width, v.height, v.fps_nominal, v.is_vfr), (0, 0, 0.0, false));
    assert_eq!(v.bitrate, None);
    assert_eq!(m.size_bytes, 1234);
    assert_eq!(m.container, "mp4");
    assert_eq!(m.duration_sec, 0.0);
}

#[test]
fn audio_only_files_are_rejected_even_with_cover_art() {
    let outs = ProbeOutputs {
        info_json: r#"{"streams":[{"index":0,"codec_type":"audio","codec_name":"aac","channels":2},
            {"index":1,"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}}],
            "format":{"duration":"3.0"}}"#
            .into(),
        ..Default::default()
    };
    assert_eq!(parse_media("song.m4a", None, &outs).unwrap_err(), Unusable::AudioOnly);
}

/// 封面图排在真实视频流前面
const COVER_FIRST: &str = r#"{"streams":[
    {"index":0,"codec_type":"video","codec_name":"mjpeg","width":600,"height":600,"disposition":{"attached_pic":1}},
    {"index":1,"codec_type":"video","codec_name":"hevc","width":3840,"height":2160,"r_frame_rate":"30/1",
     "avg_frame_rate":"30/1","pix_fmt":"yuv420p10le","disposition":{"attached_pic":0}},
    {"index":2,"codec_type":"audio","codec_name":"aac","channels":2}],"format":{"duration":"3.0"}}"#;

#[test]
fn cover_art_before_the_real_video_is_skipped() {
    let info: Value = serde_json::from_str(COVER_FIRST).unwrap();
    assert_eq!(main_video_index(&info), Some(1));
    let m =
        parse_media("movie.mp4", None, &ProbeOutputs { info_json: COVER_FIRST.into(), ..Default::default() }).unwrap();
    assert_eq!(m.video.len(), 1);
    assert_eq!((m.video[0].codec.as_str(), m.video[0].width), ("hevc", 3840));
    assert_eq!((m.attachments, m.covers), (0, Some(1)), "封面图单独计数，不算附件");
}

#[test]
fn frame_and_packet_sampling_select_the_real_video_stream() {
    let fake = FakeProbe {
        outs: ProbeOutputs { info_json: COVER_FIRST.into(), ..Default::default() },
        fail: None,
        calls: Mutex::new(Vec::new()),
    };
    probe_file(Path::new("ffprobe"), Path::new("movie.mp4"), &fake).unwrap();
    let calls = fake.calls.lock().unwrap();
    for call in &calls[1..] {
        let i = call.iter().position(|a| a == "-select_streams").unwrap();
        assert_eq!(call[i + 1], "1", "应按流序号选中真实视频流，而不是 v:0 的封面图");
    }
}

#[test]
fn no_streams_is_an_error() {
    let outs = ProbeOutputs { info_json: r#"{"streams":[],"format":{}}"#.into(), ..Default::default() };
    assert_eq!(parse_media("x.txt", None, &outs).unwrap_err(), Unusable::NoStreams);
    assert_eq!(parse_frame_count("300\n"), Some(300));
    assert_eq!(parse_frame_count("300,\r\n"), Some(300), "某些构建在末尾多一个逗号");
    assert_eq!(parse_frame_count("N/A"), None);
    assert_eq!(Unusable::NoStreams.text(crate::i18n::Lang::En), "The file has no audio or video streams");
}

#[test]
fn probe_errors_are_explained_in_chinese_with_the_original() {
    let e = explain_probe_error(
        "[mov,mp4,m4a,3gp,3g2,mj2 @ 0000] moov atom not found\nC:/x.mp4: Invalid data found when processing input\n",
    );
    assert!(e.starts_with("文件不完整或已损坏"), "{e}");
    assert!(e.contains("moov atom not found"));
    assert!(explain_probe_error("x.bin: Invalid data found when processing input").starts_with("不是可识别的媒体文件"));
}

/// 记录调用、按参数返回 fixture 的假 ffprobe
struct FakeProbe {
    outs: ProbeOutputs,
    fail: Option<String>,
    calls: Mutex<Vec<Vec<String>>>,
}

impl Runner for FakeProbe {
    fn run(&self, _p: &Path, a: &[String], _t: Duration) -> io::Result<ExecOutput> {
        self.calls.lock().unwrap().push(a.to_vec());
        if let Some(err) = &self.fail {
            return Ok(ExecOutput { code: Some(1), stderr: err.clone(), ..Default::default() });
        }
        let stdout = if a.iter().any(|s| s == "-show_frames") {
            self.outs.frame_json.clone()
        } else if a.iter().any(|s| s == "-show_packets") {
            self.outs.packets_csv.clone()
        } else {
            self.outs.info_json.clone()
        };
        Ok(ExecOutput { code: Some(0), stdout, ..Default::default() })
    }
}

#[test]
fn probe_file_runs_three_ffprobe_calls_for_video() {
    let fake = FakeProbe { outs: fixture("vfr-mkv"), fail: None, calls: Mutex::new(Vec::new()) };
    let m = probe_file(Path::new("ffprobe"), Path::new("vfr.mkv"), &fake).unwrap();
    assert!(m.video[0].is_vfr);
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert!(calls[1].contains(&"%+#1".to_string()));
    assert!(calls[2].contains(&"%+#120".to_string()));
}

#[test]
fn probe_file_reports_broken_files() {
    let fake = FakeProbe {
        outs: ProbeOutputs::default(),
        fail: Some("[mov,mp4 @ 0x1] moov atom not found\nbroken.mp4: Invalid data found when processing input".into()),
        calls: Mutex::new(Vec::new()),
    };
    let err = probe_file(Path::new("ffprobe"), Path::new("broken.mp4"), &fake).unwrap_err();
    assert!(err.to_string().contains("文件不完整或已损坏"));
    let (reason, raw) = err.describe(crate::i18n::Lang::En);
    assert!(reason.starts_with("The file is incomplete or damaged"), "{reason}");
    assert_eq!(raw.as_deref(), Some("[mov,mp4 @ 0x1] moov atom not found"));
    assert_eq!(fake.calls.lock().unwrap().len(), 1, "第一步失败就不再继续");
}
