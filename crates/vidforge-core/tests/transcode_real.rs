//! 里程碑 M3：用 Rust 生成的命令在真实 ffmpeg 上转码合成素材，再用 ffprobe 核对输出。
//!
//! 素材 → probe_file（真实分析）→ 手工构造计划 → build_arg_segments → ffmpeg → probe_file 核对。
//! 找不到 ffmpeg 时跳过；需要特定能力（QSV、libplacebo）的用例在能力不满足时跳过。

use std::path::{Path, PathBuf};
use std::time::Duration;

use vidforge_core::ffmpeg::exec::{Runner, SystemRunner, args};
use vidforge_core::ffmpeg::probe::{probe_file, pts_is_vfr};
use vidforge_core::model::{
    AudioCodec, AudioMode, AudioTrackPlan, Capabilities, Container, DoviAction, EncoderId, EnvStatus, FidelityRequest,
    FpsPolicy, HdrAction, HdrKind, MediaInfo, Platform, QualityTier, RateControl, ResolutionPreset, Scenario,
    StreamAction, SubtitleMode, ToneMapPipeline, TrackRole, TranscodePlan, VideoPlan,
};
use vidforge_core::pipeline::args::{build_arg_segments, build_first_pass, flatten};

fn bin_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("VIDFORGE_TEST_FFMPEG")
        .map(PathBuf::from)
        .or_else(|| cfg!(windows).then(|| PathBuf::from(r"C:\Program1\ffmpeg\bin")))?;
    dir.join(exe_name("ffmpeg")).is_file().then_some(dir)
}

fn exe_name(n: &str) -> String {
    if cfg!(windows) { format!("{n}.exe") } else { n.to_string() }
}

struct Env {
    bin: PathBuf,
    dir: tempfile::TempDir,
    caps: Capabilities,
}

impl Env {
    fn new() -> Option<Env> {
        let bin = bin_dir()?;
        let app = tempfile::tempdir().unwrap();
        let ctx = vidforge_core::ffmpeg::capability::ProbeContext {
            env: &vidforge_core::ffmpeg::locate::SystemEnv,
            runner: &SystemRunner,
            platform: vidforge_core::model::Platform::current(),
            app_dir: app.path().to_path_buf(),
            user_path: Some(bin.clone()),
            lang: vidforge_core::i18n::Lang::ZhCn,
        };
        let caps = vidforge_core::ffmpeg::capability::probe(&ctx, true, &|_| {});
        assert_eq!(caps.status, EnvStatus::Ready);
        Some(Env { bin, dir: tempfile::tempdir().unwrap(), caps })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn ffmpeg(&self, a: &[&str]) {
        let mut full = args(["-hide_banner", "-nostdin", "-v", "error", "-y"]);
        full.extend(a.iter().map(|s| s.to_string()));
        let out = SystemRunner.run(&self.bin.join(exe_name("ffmpeg")), &full, Duration::from_secs(120)).unwrap();
        assert!(out.success(), "合成素材失败 {a:?}：{}", out.stderr);
    }

    fn probe(&self, p: &Path) -> MediaInfo {
        probe_file(&self.bin.join(exe_name("ffprobe")), p, &SystemRunner)
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    }

    /// 这台机器上命令里应当出现的硬解方式（技术事实文档 7.5）
    fn hwaccel(&self) -> &'static str {
        if self.caps.platform == Platform::Windows && self.caps.device_available("d3d11va") {
            "d3d11va"
        } else {
            "auto"
        }
    }

    /// 生成命令并真正执行，返回输出文件的分析结果与执行时的完整参数
    fn transcode(&self, media: &MediaInfo, plan: &TranscodePlan, out_name: &str) -> (MediaInfo, Vec<String>) {
        let out = self.path(out_name);
        let a = flatten(&build_arg_segments(media, plan, &self.caps, &out));
        assert_eq!(a[0], "ffmpeg");
        let r = SystemRunner.run(&self.bin.join(exe_name("ffmpeg")), &a[1..], Duration::from_secs(300)).unwrap();
        assert!(r.success(), "转码失败：{}\n命令：{}", r.stderr, a.join(" "));
        (self.probe(&out), a)
    }

    /// 执行一条完整命令（首项是 "ffmpeg"），返回 stderr；`verbose` 时把日志级别提到 verbose 以便看到实际码率控制方式
    fn run_command(&self, a: &[String], verbose: bool) -> String {
        assert_eq!(a[0], "ffmpeg");
        let mut a = a[1..].to_vec();
        if verbose {
            let i = a.iter().position(|x| x == "-loglevel").unwrap();
            a[i + 1] = "verbose".into();
        }
        let r = SystemRunner.run(&self.bin.join(exe_name("ffmpeg")), &a, Duration::from_secs(300)).unwrap();
        assert!(r.success(), "执行失败：{}\n命令：{}", r.stderr, a.join(" "));
        r.stderr
    }

    fn ffprobe_field(&self, p: &Path, select: &str, entry: &str) -> String {
        let a = args(["-v", "error", "-select_streams", select, "-show_entries", entry, "-of", "default=nw=1:nk=1"]);
        let mut a = a;
        a.push(p.to_string_lossy().to_string());
        SystemRunner
            .run(&self.bin.join(exe_name("ffprobe")), &a, Duration::from_secs(30))
            .unwrap()
            .stdout
            .trim()
            .to_string()
    }
}

const X265_HDR10: &str = "log-level=error:hdr10=1:repeat-headers=1:colorprim=bt2020:transfer=smpte2084:\
colormatrix=bt2020nc:master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1):max-cll=1000,400";

fn hdr10_source(e: &Env) -> MediaInfo {
    let p = e.path("src-hdr10.mkv");
    e.ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=640x360:r=24",
        "-t",
        "2",
        "-pix_fmt",
        "yuv420p10le",
        "-c:v",
        "libx265",
        "-x265-params",
        X265_HDR10,
        &p.to_string_lossy(),
    ]);
    e.probe(&p)
}

fn base_plan(encoder: EncoderId, container: Container) -> TranscodePlan {
    TranscodePlan {
        scenario: Scenario::Archive,
        video: VideoPlan {
            action: StreamAction::Encode,
            codec: encoder.codec(),
            encoder,
            encoder_auto: false,
            quality: QualityTier::Standard,
            quality_value: vidforge_core::pipeline::encoders::quality_value(encoder, QualityTier::Standard),
            rate_control: RateControl::Quality,
            preset: match encoder {
                EncoderId::Libsvtav1 => "10".into(),
                EncoderId::HevcQsv | EncoderId::H264Qsv | EncoderId::Av1Qsv => "veryfast".into(),
                _ => "ultrafast".into(),
            },
            bit_depth: 8,
            resolution: ResolutionPreset::Source,
            fps: FpsPolicy::Keep,
            hdr_action: HdrAction::Keep,
            tonemap: None,
            dovi: DoviAction::Disable,
            gop: None,
            extra_params: None,
            extra_args: None,
        },
        audio: Vec::new(),
        audio_mode: AudioMode::CopyAll,
        loudnorm: false,
        subtitles: SubtitleMode::None,
        container,
        fidelity: FidelityRequest::default(),
    }
}

fn copy_all_audio(m: &MediaInfo) -> Vec<AudioTrackPlan> {
    m.audio
        .iter()
        .map(|a| AudioTrackPlan {
            source_index: a.index,
            action: StreamAction::Copy,
            codec: None,
            bitrate_kbps: None,
            channels: None,
            title: a.title.clone(),
            role: TrackRole::Original,
        })
        .collect()
}

fn assert_hdr10_1000(m: &MediaInfo, what: &str) {
    let v = &m.video[0];
    assert_eq!(v.color.hdr_kind, HdrKind::Hdr10, "{what}: 应仍是 HDR10");
    let md = v.hdr10.as_ref().unwrap_or_else(|| panic!("{what}: 丢了 HDR10 元数据"));
    assert!((md.max_luminance - 1000.0).abs() < 1e-3, "{what}: 母版亮度 {}", md.max_luminance);
    assert_eq!(md.max_cll, Some(1000.0), "{what}: MaxCLL");
}

macro_rules! env_or_skip {
    () => {
        match Env::new() {
            Some(e) => e,
            None => {
                eprintln!("跳过：没有可用的 ffmpeg（设置 VIDFORGE_TEST_FFMPEG 指定目录）");
                return;
            }
        }
    };
}

#[test]
fn hdr10_is_kept_by_x265_and_svtav1_without_manual_master_display() {
    let e = env_or_skip!();
    let src = hdr10_source(&e);
    for (enc, name) in [(EncoderId::Libx265, "keep-x265.mkv"), (EncoderId::Libsvtav1, "keep-av1.mkv")] {
        let mut plan = base_plan(enc, Container::Mkv);
        plan.video.bit_depth = 10;
        let (out, a) = e.transcode(&src, &plan, name);
        assert!(!a.join(" ").contains("master-display"), "不应手写 master-display，靠自动透传");
        assert_hdr10_1000(&out, name);
        assert_eq!(out.video[0].bit_depth, 10);
    }
}

#[test]
fn tonemapped_output_is_tagged_bt709_by_the_filter() {
    let e = env_or_skip!();
    let src = hdr10_source(&e);
    for pipeline in [ToneMapPipeline::Zscale, ToneMapPipeline::Libplacebo, ToneMapPipeline::TonemapOpencl] {
        if !e.caps.tonemap_available(pipeline) {
            eprintln!("跳过 {pipeline:?}：当前环境不可用");
            continue;
        }
        let mut plan = base_plan(EncoderId::Libx264, Container::Mp4);
        plan.video.hdr_action = HdrAction::Tonemap;
        plan.video.tonemap = Some(pipeline);
        plan.video.resolution = ResolutionPreset::P480;
        let name = format!("tonemap-{pipeline:?}.mp4");
        let (out, _) = e.transcode(&src, &plan, &name);
        let v = &out.video[0];
        assert_eq!((v.color.primaries.as_str(), v.color.transfer.as_str()), ("bt709", "bt709"), "{name}: 色彩标签");
        assert_eq!(v.color.hdr_kind, HdrKind::None, "{name}: 应已是 SDR");
        assert_eq!((v.bit_depth, v.pix_fmt.as_str()), (8, "yuv420p"), "{name}");
        assert_eq!(v.height.min(v.width), 360, "{name}: 480p 档不放大 360p 源");
    }
}

#[test]
fn qsv_keeps_hdr10_and_mp4_gets_hvc1() {
    let e = env_or_skip!();
    let q = e.caps.encoder(EncoderId::HevcQsv).cloned();
    if !q.as_ref().is_some_and(|q| q.usable && q.ten_bit) {
        eprintln!("跳过：没有支持 10bit 的 hevc_qsv");
        return;
    }
    let src = hdr10_source(&e);
    let mut plan = base_plan(EncoderId::HevcQsv, Container::Mp4);
    plan.video.bit_depth = 10;
    let (out, a) = e.transcode(&src, &plan, "qsv.mp4");
    assert!(a.windows(2).any(|w| w == ["-hwaccel", e.hwaccel()]), "{a:?}");
    assert_hdr10_1000(&out, "hevc_qsv");
    assert_eq!(e.ffprobe_field(&e.path("qsv.mp4"), "v:0", "stream=codec_tag_string"), "hvc1");
}

#[test]
fn vfr_to_cfr_gives_strictly_constant_frame_rate_and_aligned_audio() {
    let e = env_or_skip!();
    let p = e.path("src-vfr.mp4");
    e.ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=640x360:r=30",
        "-f",
        "lavfi",
        "-i",
        "sine=f=300:sample_rate=48000",
        "-t",
        "6",
        "-vf",
        "select='lt(n,60)+not(mod(n,3))'",
        "-fps_mode",
        "vfr",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-c:a",
        "aac",
        "-ac",
        "2",
        &p.to_string_lossy(),
    ]);
    let src = e.probe(&p);
    assert!(src.video[0].is_vfr);
    let mut plan = base_plan(EncoderId::Libx264, Container::Mov);
    plan.scenario = Scenario::Editing;
    plan.video.fps = FpsPolicy::Cfr { fps: 30.0 };
    plan.audio = copy_all_audio(&src);
    let (out, a) = e.transcode(&src, &plan, "cfr.mov");
    assert!(a.windows(4).any(|w| w == ["-fps_mode:v", "cfr", "-r", "30"]));
    let v = &out.video[0];
    // 验收标准 9：r_frame_rate 与 avg_frame_rate 一致、帧间隔恒定、音视频时长差小于 1 帧
    assert_eq!((v.fps_nominal, v.fps_avg), (30.0, 30.0));
    assert!(!v.is_vfr);
    let pts: Vec<f64> = {
        let mut a =
            args(["-v", "error", "-select_streams", "v:0", "-show_entries", "packet=pts_time", "-of", "csv=p=0"]);
        a.push(e.path("cfr.mov").to_string_lossy().to_string());
        let o = SystemRunner.run(&e.bin.join(exe_name("ffprobe")), &a, Duration::from_secs(30)).unwrap();
        o.stdout.lines().filter_map(|l| l.trim().parse().ok()).collect()
    };
    assert!(!pts_is_vfr(&pts), "输出的包间隔应恒定");
    let dur = |sel: &str| e.ffprobe_field(&e.path("cfr.mov"), sel, "stream=duration").parse::<f64>().unwrap();
    let (vd, ad) = (dur("v:0"), dur("a:0"));
    assert!((vd - ad).abs() < 1.0 / 30.0, "音视频时长差 {:.4}s 超过 1 帧（视频 {vd}，音频 {ad}）", (vd - ad).abs());
}

#[test]
fn remux_copies_every_track_chapter_and_attachment() {
    let e = env_or_skip!();
    let d = e.dir.path();
    std::fs::write(d.join("s.srt"), "1\n00:00:00,000 --> 00:00:01,000\nHi\n").unwrap();
    std::fs::write(
        d.join("ch.txt"),
        ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=A\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=1000\nEND=2000\ntitle=B\n",
    )
    .unwrap();
    let src_path = e.path("src-remux.mkv");
    e.ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=640x360:r=24",
        "-f",
        "lavfi",
        "-i",
        "sine=f=220:sample_rate=48000",
        "-i",
        &d.join("s.srt").to_string_lossy(),
        "-i",
        &d.join("ch.txt").to_string_lossy(),
        "-t",
        "2",
        "-filter_complex",
        "[1:a]pan=5.1|c0=c0|c1=c0|c2=c0|c3=c0|c4=c0|c5=c0,asplit=2[a][b]",
        "-map",
        "0:v",
        "-map",
        "[a]",
        "-map",
        "[b]",
        "-map",
        "2:s",
        "-map_chapters",
        "3",
        "-c:v",
        "libx265",
        "-x265-params",
        "log-level=error",
        "-c:a:0",
        "truehd",
        "-strict",
        "-2",
        "-c:a:1",
        "eac3",
        "-c:s",
        "srt",
        "-attach",
        &d.join("s.srt").to_string_lossy(),
        "-metadata:s:t",
        "mimetype=application/x-subrip",
        &src_path.to_string_lossy(),
    ]);
    let src = e.probe(&src_path);
    let mut plan = base_plan(EncoderId::Libx265, Container::Mkv);
    plan.scenario = Scenario::Remux;
    plan.video.action = StreamAction::Copy;
    plan.video.dovi = DoviAction::Remux;
    plan.audio = copy_all_audio(&src);
    plan.subtitles = SubtitleMode::All;
    let (out, _) = e.transcode(&src, &plan, "remux-out.mkv");
    let codecs = |m: &MediaInfo| m.audio.iter().map(|a| a.codec.clone()).collect::<Vec<_>>();
    assert_eq!(codecs(&out), codecs(&src), "音轨编码必须原样");
    assert!(out.audio[0].lossless, "TrueHD 未被重编码");
    assert_eq!((out.subtitle.len(), out.chapters, out.attachments), (1, 2, 1));
    assert_eq!(out.video[0].codec, "hevc");
}

#[test]
fn downmix_to_stereo_uses_pan_and_yields_two_channels() {
    let e = env_or_skip!();
    let p = e.path("src-51.mp4");
    e.ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=320x180:r=24",
        "-f",
        "lavfi",
        "-i",
        "sine=f=220:sample_rate=48000",
        "-t",
        "2",
        "-filter_complex",
        "[1:a]pan=5.1|c0=c0|c1=c0|c2=c0|c3=c0|c4=c0|c5=c0[a]",
        "-map",
        "0:v",
        "-map",
        "[a]",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-c:a",
        "eac3",
        &p.to_string_lossy(),
    ]);
    let src = e.probe(&p);
    assert_eq!(src.audio[0].channels, 6);
    let mut plan = base_plan(EncoderId::Libx264, Container::Mp4);
    plan.audio = vec![AudioTrackPlan {
        source_index: src.audio[0].index,
        action: StreamAction::Encode,
        codec: Some(AudioCodec::Aac),
        bitrate_kbps: Some(160),
        channels: Some(2),
        title: None,
        role: TrackRole::Compat,
    }];
    let (out, a) = e.transcode(&src, &plan, "stereo.mp4");
    assert!(a.iter().any(|x| x.starts_with("pan=stereo|")));
    assert!(!a.iter().any(|x| x.starts_with("-ac")), "pan 之后不能再 -ac");
    assert_eq!((out.audio[0].codec.as_str(), out.audio[0].channels), ("aac", 2));
}

#[test]
fn hardware_decode_keeps_hdr10_side_data() {
    // 硬解（帧自动下载到内存）后编码，HDR10 帧级元数据是否还在：决定硬解能否用于保留 HDR 的任务
    let e = env_or_skip!();
    if !e.caps.device_available("qsv") {
        eprintln!("跳过：没有 QSV 设备");
        return;
    }
    let src = hdr10_source(&e);
    // libx265 的 HDR10 自动透传依赖解码帧上的 MDCV / CLL side data：硬解下载到内存后它们必须还在
    let mut plan = base_plan(EncoderId::Libx265, Container::Mkv);
    plan.video.bit_depth = 10;
    let (out, a) = e.transcode(&src, &plan, "hwdec.mkv");
    assert!(a.windows(2).any(|w| w == ["-hwaccel", e.hwaccel()]), "{a:?}");
    assert_hdr10_1000(&out, "硬解后 libx265 编码");
}

#[test]
fn rotated_phone_clip_is_scaled_in_display_orientation() {
    let e = env_or_skip!();
    let tmp = e.path("landscape.mp4");
    let src_path = e.path("portrait.mp4");
    e.ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=1920x1080:r=30",
        "-t",
        "1",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        &tmp.to_string_lossy(),
    ]);
    e.ffmpeg(&[
        "-display_rotation:v:0",
        "-90",
        "-i",
        &tmp.to_string_lossy(),
        "-c",
        "copy",
        &src_path.to_string_lossy(),
    ]);
    let src = e.probe(&src_path);
    assert_eq!((src.video[0].width, src.video[0].height, src.video[0].rotation), (1920, 1080, -90));
    let mut plan = base_plan(EncoderId::Libx264, Container::Mp4);
    plan.video.resolution = ResolutionPreset::P720;
    let (out, _) = e.transcode(&src, &plan, "portrait-720.mp4");
    // ffmpeg 自动旋转后编码，输出是竖的 720×1280、不再带旋转
    assert_eq!((out.video[0].width, out.video[0].height), (720, 1280), "竖拍缩放方向错了");
    assert_eq!(out.video[0].rotation, 0);
}

#[test]
fn cover_art_is_never_encoded_as_the_video() {
    let e = env_or_skip!();
    let video = e.path("plain.mp4");
    let cover = e.path("cover.png");
    let src_path = e.path("with-cover.mp4");
    e.ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=640x360:r=24",
        "-t",
        "1",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        &video.to_string_lossy(),
    ]);
    e.ffmpeg(&["-f", "lavfi", "-i", "color=c=red:s=300x300", "-frames:v", "1", &cover.to_string_lossy()]);
    // 带封面图的 MP4。mov 封装器会把封面挪成最后一路（covr），真实文件里封面排在前面的情况
    // 由 probe 与 args 的单元测试覆盖；这里核对真实文件端到端不会把封面当正片
    e.ffmpeg(&[
        "-i",
        &cover.to_string_lossy(),
        "-i",
        &video.to_string_lossy(),
        "-map",
        "0",
        "-map",
        "1",
        "-c",
        "copy",
        "-disposition:v:0",
        "attached_pic",
        &src_path.to_string_lossy(),
    ]);
    let src = e.probe(&src_path);
    assert_eq!((src.video.len(), src.attachments, src.covers), (1, 0, Some(1)), "封面图单独计数、不算视频流");
    let plan = base_plan(EncoderId::Libx264, Container::Mkv);
    let (out, a) = e.transcode(&src, &plan, "no-cover.mkv");
    let map = format!("0:{}", src.video[0].index);
    assert!(a.windows(2).any(|w| w[0] == "-map" && w[1] == map), "应按流序号映射 {map}");
    assert_eq!(out.video.len(), 1);
    assert_eq!((out.video[0].width, out.video[0].height), (640, 360), "编码的是封面而不是正片");
    // 现在的命令不带封面：校验要如实标出，免得报告全绿后源文件被移进回收站
    let report = vidforge_core::verify::report(&src, &plan, &out, vidforge_core::i18n::Lang::ZhCn);
    assert!(report.iter().any(|r| r.label == "封面图" && !r.ok), "{report:#?}");
}

#[test]
fn rate_control_modes_behave_as_documented() {
    // 技术事实文档 8.2：两遍编码的两条命令都能跑通且码率贴近目标；QSV 的目标码率与限峰值分别落到 VBR 与 QVBR
    let e = env_or_skip!();
    let src_path = e.path("rc-src.mkv");
    e.ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=1280x720:r=30:d=4",
        "-c:v",
        "libx264",
        "-crf",
        "10",
        &src_path.to_string_lossy(),
    ]);
    let src = e.probe(&src_path);
    for enc in [EncoderId::Libx264, EncoderId::Libx265, EncoderId::Libsvtav1] {
        let mut plan = base_plan(enc, Container::Mkv);
        plan.video.rate_control = RateControl::TwoPass { kbps: 1500 };
        let out = e.path(&format!("2pass-{}.mkv", enc.name()));
        let first = flatten(&build_first_pass(&src, &plan, &e.caps, &out).expect("两遍编码应有第一遍"));
        e.run_command(&first, false);
        let second = flatten(&build_arg_segments(&src, &plan, &e.caps, &out));
        e.run_command(&second, false);
        let kbps = e.probe(&out).bitrate as f64 / 1000.0;
        assert!((1200.0..=1800.0).contains(&kbps), "{}：两遍目标 1500k，实际 {kbps:.0}k", enc.name());
    }

    if !e.caps.encoder_usable(EncoderId::HevcQsv) {
        eprintln!("跳过 QSV 部分：没有可用的 hevc_qsv");
        return;
    }
    for (rc, method) in [
        (RateControl::Bitrate { kbps: 1500 }, "(VBR)"),
        (RateControl::Capped { kbps: 1000 }, "(QVBR)"),
        (RateControl::Quality, "(ICQ)"),
    ] {
        let mut plan = base_plan(EncoderId::HevcQsv, Container::Mkv);
        plan.video.rate_control = rc;
        let out = e.path("qsv-rc.mkv");
        let stderr = e.run_command(&flatten(&build_arg_segments(&src, &plan, &e.caps, &out)), true);
        assert!(stderr.contains(method), "{rc:?} 应落到 {method}：{stderr}");
    }
}
