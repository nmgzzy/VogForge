//! 阶段 6 验收：真实队列 + 真实 ffmpeg。批量 10 个合成素材跑通并通过基础校验；取消不留半成品；
//! 暂停真的挂起进程；NVENC 预检失败时沿回退链换到 QSV 并在日志里说明。找不到 ffmpeg 时跳过。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vidforge_core::config::Settings;
use vidforge_core::ffmpeg::exec::{Runner, SystemRunner, args};
use vidforge_core::ffmpeg::probe::probe_file;
use vidforge_core::i18n::Lang;
use vidforge_core::model::{
    Capabilities, EncoderId, EnvStatus, EventLevel, Job, JobProgressEvent, JobStatus, MediaInfo, QueueItem, QueueOp,
    QueueSnapshot, RateControl, Scenario, TranscodePlan,
};
use vidforge_core::pipeline::recommend_plan;
use vidforge_core::pipeline::strategy::switch_encoder;
use vidforge_core::queue::process::SystemTools;
use vidforge_core::queue::{EventSink, Queue, QueueDeps, SystemClock};

fn bin_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("VIDFORGE_TEST_FFMPEG")
        .map(PathBuf::from)
        .or_else(|| cfg!(windows).then(|| PathBuf::from(r"C:\Program1\ffmpeg\bin")))?;
    dir.join(exe("ffmpeg")).is_file().then_some(dir)
}

fn exe(n: &str) -> String {
    if cfg!(windows) { format!("{n}.exe") } else { n.to_string() }
}

#[derive(Default)]
struct Sink {
    progress: Mutex<Vec<JobProgressEvent>>,
}

impl EventSink for Sink {
    fn snapshot(&self, _s: &QueueSnapshot) {}
    fn progress(&self, e: &JobProgressEvent) {
        self.progress.lock().unwrap().push(e.clone());
    }
}

struct Env {
    bin: PathBuf,
    caps: Capabilities,
    dir: tempfile::TempDir,
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
            lang: Lang::ZhCn,
        };
        let caps = vidforge_core::ffmpeg::capability::probe(&ctx, true, &|_| {});
        assert_eq!(caps.status, EnvStatus::Ready);
        Some(Env { bin, caps, dir: tempfile::tempdir().unwrap() })
    }

    fn synth(&self, name: &str, a: &[&str]) -> MediaInfo {
        let path = self.dir.path().join("src").join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut full = args(["-hide_banner", "-nostdin", "-v", "error", "-y"]);
        full.extend(a.iter().map(|s| s.to_string()));
        full.push(path.to_string_lossy().to_string());
        let out = SystemRunner.run(&self.bin.join(exe("ffmpeg")), &full, Duration::from_secs(120)).unwrap();
        assert!(out.success(), "合成 {name} 失败：{}", out.stderr);
        probe_file(&self.bin.join(exe("ffprobe")), &path, &SystemRunner).unwrap()
    }

    fn settings(&self) -> Settings {
        Settings {
            output_dir: Some(self.dir.path().join("out").to_string_lossy().to_string()),
            cpu_slots: 2,
            ..Settings::default()
        }
    }

    fn queue(&self, sink: Arc<Sink>) -> Queue {
        Queue::start(QueueDeps {
            tools: Arc::new(SystemTools),
            clock: Arc::new(SystemClock),
            sink,
            store: None,
            lang: Lang::ZhCn,
        })
    }

    fn leftovers(&self) -> Vec<String> {
        let mut names = Vec::new();
        for e in walk(&self.dir.path().join("out")) {
            let n = e.file_name().unwrap().to_string_lossy().to_string();
            if n.contains(".vidforge-part") || n.contains(".2pass") {
                names.push(n);
            }
        }
        names
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.flat_map(|e| {
        let p = e.unwrap().path();
        if p.is_dir() { walk(&p) } else { vec![p] }
    })
    .collect()
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

/// 推荐计划，编码速度调到最快，让测试几秒内跑完
fn fast(media: &MediaInfo, scenario: Scenario, caps: &Capabilities) -> TranscodePlan {
    let mut p = recommend_plan(media, scenario, caps);
    p.video.preset = match p.video.encoder {
        EncoderId::Libx265 | EncoderId::Libx264 => "ultrafast".into(),
        EncoderId::Libsvtav1 => "12".into(),
        EncoderId::HevcQsv | EncoderId::H264Qsv | EncoderId::Av1Qsv => "veryfast".into(),
        _ => p.video.preset,
    };
    p
}

fn job(q: &Queue, id: &str) -> Job {
    q.snapshot().jobs.into_iter().find(|j| j.id == id).unwrap()
}

fn wait_until(what: &str, timeout: Duration, f: impl Fn() -> bool) {
    let deadline = Instant::now() + timeout;
    while !f() {
        assert!(Instant::now() < deadline, "等不到：{what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_batch_of_ten_synthetic_clips_runs_through_the_real_queue() {
    let e = env_or_skip!();
    let lavfi = |src: &str| ["-f", "lavfi", "-i", src].map(String::from).to_vec();
    let clip = |name: &str, extra: &[&str]| {
        let mut a: Vec<String> = lavfi("testsrc2=s=640x360:r=30:d=2");
        a.extend(lavfi("sine=f=440:d=2"));
        a.extend(extra.iter().map(|s| s.to_string()));
        let refs: Vec<&str> = a.iter().map(String::as_str).collect();
        e.synth(name, &refs)
    };
    let h264 = ["-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", "-shortest"];
    let media = [
        clip("01.mp4", &h264),
        clip("02.mov", &h264),
        clip("03.mkv", &["-c:v", "libx265", "-preset", "ultrafast", "-c:a", "flac", "-shortest"]),
        clip(
            "04.mp4",
            &["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p10le", "-c:a", "aac", "-shortest"],
        ),
        clip("05.mkv", &["-c:v", "libx264", "-preset", "ultrafast", "-c:a", "ac3", "-ac", "6", "-shortest"]),
        clip("06.mp4", &h264),
        clip("07.mp4", &h264),
        clip("08.mkv", &h264),
        clip("09.mp4", &h264),
        clip("10.mp4", &h264),
    ];
    let caps = e.caps.clone();
    let scenarios = [
        Scenario::Archive,
        Scenario::Streaming,
        Scenario::Collection,
        Scenario::Mobile,
        Scenario::Smallest,
        Scenario::Social,
        Scenario::Editing,
        Scenario::Remux,
        Scenario::Archive,
        Scenario::Archive,
    ];
    let mut items: Vec<QueueItem> =
        media.iter().zip(scenarios).map(|(m, s)| QueueItem { media: m.clone(), plan: fast(m, s, &caps) }).collect();
    // 第 9 个两遍编码，第 10 个换成 x264 目标码率
    items[8].plan.video.rate_control = RateControl::TwoPass { kbps: 800 };
    items[9].plan = switch_encoder(items[9].plan.clone(), EncoderId::Libx264, &media[9], &caps);
    items[9].plan.video.preset = "ultrafast".into();
    items[9].plan.video.rate_control = RateControl::Bitrate { kbps: 600 };

    let sink = Arc::new(Sink::default());
    let q = e.queue(sink.clone());
    q.set_environment(caps, e.settings());
    let ids = q.add(items);
    assert!(q.wait_idle(Duration::from_secs(300)), "10 个任务没在 5 分钟内跑完");

    for id in &ids {
        let j = job(&q, id);
        assert_eq!(j.status, JobStatus::Done, "{} 未完成：{:#?}", j.media.name, j.events);
        let report = j.report.as_ref().expect("完成的任务应有校验结果");
        assert!(report.iter().all(|r| r.ok), "{} 校验未通过：{report:#?}", j.media.name);
        assert!(Path::new(&j.output_path).exists());
        assert!(j.output_size.unwrap() > 0);
    }
    assert!(e.leftovers().is_empty(), "输出目录有残留：{:?}", e.leftovers());
    let two_pass = job(&q, &ids[8]);
    assert!(two_pass.first_pass.is_some());
    let passes: Vec<Option<u8>> =
        sink.progress.lock().unwrap().iter().filter(|p| p.id == ids[8]).map(|p| p.progress.pass).collect();
    assert!(passes.contains(&Some(1)) && passes.contains(&Some(2)), "{passes:?}");
}

#[test]
fn cancel_pause_and_hardware_fallback_on_real_processes() {
    let e = env_or_skip!();
    // 足够长的素材：慢速 x265 需要几十秒，留出暂停与取消的时间
    let long = e.synth(
        "long.mp4",
        &["-f", "lavfi", "-i", "testsrc2=s=1920x1080:r=30:d=30", "-c:v", "libx264", "-preset", "ultrafast"],
    );
    let caps = e.caps.clone();
    let sink = Arc::new(Sink::default());
    let q = e.queue(sink.clone());
    q.set_environment(caps.clone(), e.settings());

    // ── 暂停：进度停住；继续：接着跑 ──
    let mut slow = recommend_plan(&long, Scenario::Archive, &caps);
    slow.video.preset = "medium".into();
    let paused = q.add(vec![QueueItem { media: long.clone(), plan: slow.clone() }]).remove(0);
    let progressed = |id: &str| sink.progress.lock().unwrap().iter().filter(|p| p.id == id).count();
    wait_until("开始有进度", Duration::from_secs(60), || progressed(&paused) >= 2);
    q.apply(QueueOp::Pause { id: paused.clone() }).unwrap();
    std::thread::sleep(Duration::from_millis(600));
    let (n, t) = (progressed(&paused), job(&q, &paused).progress.out_time_sec);
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(progressed(&paused), n, "挂起后不应再有进度");
    assert_eq!(job(&q, &paused).progress.out_time_sec, t);
    q.apply(QueueOp::Resume { id: paused.clone() }).unwrap();
    wait_until("继续后进度前进", Duration::from_secs(30), || progressed(&paused) > n);

    // ── 取消：进程结束，临时文件删除 ──
    q.apply(QueueOp::Cancel { id: paused.clone() }).unwrap();
    wait_until("取消完成", Duration::from_secs(30), || job(&q, &paused).status == JobStatus::Cancelled);
    assert!(e.leftovers().is_empty(), "取消后有残留：{:?}", e.leftovers());
    assert!(!Path::new(&job(&q, &paused).output_path).exists());

    // ── NVENC 预检失败 → 回退到 QSV（本机没有 NVIDIA 卡，报错是真实的）──
    if !caps.encoder_usable(EncoderId::HevcQsv) {
        eprintln!("跳过回退部分：没有可用的 hevc_qsv");
        return;
    }
    let mut pretend = caps.clone();
    for enc in pretend.encoders.iter_mut().filter(|x| x.id == EncoderId::HevcNvenc) {
        (enc.usable, enc.ten_bit) = (true, true);
    }
    q.set_environment(pretend.clone(), e.settings());
    let short = e.synth("short.mp4", &["-f", "lavfi", "-i", "testsrc2=s=640x360:r=30:d=2", "-c:v", "libx264"]);
    let nvenc =
        switch_encoder(recommend_plan(&short, Scenario::Streaming, &pretend), EncoderId::HevcNvenc, &short, &pretend);
    let id = q.add(vec![QueueItem { media: short, plan: nvenc }]).remove(0);
    assert!(q.wait_idle(Duration::from_secs(120)));
    let j = job(&q, &id);
    assert_eq!(j.status, JobStatus::Done, "{:#?}", j.events);
    assert_eq!(j.encoder_used, EncoderId::HevcQsv);
    let warn: Vec<&str> = j.events.iter().filter(|x| x.level == EventLevel::Warn).map(|x| x.message.as_str()).collect();
    assert!(warn.iter().any(|m| m.contains("回退到 hevc_qsv")), "{warn:?}");
}

#[test]
fn loudness_normalization_reaches_the_target_through_the_queue() {
    // 技术事实文档 6.5：两遍 loudnorm 让安静的素材达到 -16 LUFS，输出采样率回到 48 kHz
    let e = env_or_skip!();
    let quiet = e.synth(
        "quiet.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=640x360:r=30:d=8",
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:d=8:sample_rate=48000",
            "-af",
            "volume=-30dB",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a",
            "aac",
            "-shortest",
        ],
    );
    let caps = e.caps.clone();
    // 最小体积把音频转成 Opus（AAC 立体声源在其他场景里会原样复制，无法标准化）
    let mut plan = fast(&quiet, Scenario::Smallest, &caps);
    plan.loudnorm = true;
    let q = e.queue(Arc::new(Sink::default()));
    q.set_environment(caps, e.settings());
    let id = q.add(vec![QueueItem { media: quiet, plan }]).remove(0);
    assert!(q.wait_idle(Duration::from_secs(120)));
    let j = job(&q, &id);
    assert_eq!(j.status, JobStatus::Done, "{:#?}", j.events);
    assert!(j.events.iter().any(|x| x.message.contains("测量 1 条音轨的响度")));
    assert!(j.args.iter().any(|a| a.contains("measured_I=") && a.contains("linear=true")), "{:?}", j.args);

    let out = PathBuf::from(&j.output_path);
    let measured = SystemRunner
        .run(
            &e.bin.join(exe("ffmpeg")),
            &args([
                "-hide_banner",
                "-nostdin",
                "-i",
                &out.to_string_lossy(),
                "-map",
                "0:a:0",
                "-af",
                "loudnorm=print_format=json",
                "-f",
                "null",
                "-",
            ]),
            Duration::from_secs(60),
        )
        .unwrap();
    let m = vidforge_core::pipeline::loudness::parse_measure(&measured.stderr).expect("测不到输出的响度");
    assert!((m.input_i + 16.0).abs() < 1.5, "输出响度 {} LUFS，目标 -16", m.input_i);
    let info = probe_file(&e.bin.join(exe("ffprobe")), &out, &SystemRunner).unwrap();
    assert_eq!(info.audio[0].sample_rate, 48_000, "loudnorm 之后应降回 48 kHz");
}

/// 长任务的资源观察（阶段 6 验证项）：1 小时的素材跑完整个队列流程。耗时约两分钟，默认不跑：
/// `cargo test -p vidforge-core --test queue_real long_job -- --ignored --nocapture`，同时在外部观察进程内存
#[test]
#[ignore]
fn long_job_keeps_bounded_state() {
    let e = env_or_skip!();
    let long = e.synth(
        "hour.mp4",
        &["-f", "lavfi", "-i", "testsrc2=s=320x180:r=30:d=3600", "-c:v", "libx264", "-preset", "ultrafast"],
    );
    let caps = e.caps.clone();
    let mut plan = switch_encoder(recommend_plan(&long, Scenario::Archive, &caps), EncoderId::Libx264, &long, &caps);
    plan.video.preset = "ultrafast".into();
    let sink = Arc::new(Sink::default());
    let q = e.queue(sink.clone());
    q.set_environment(caps, e.settings());
    let started = Instant::now();
    let id = q.add(vec![QueueItem { media: long, plan }]).remove(0);
    assert!(q.wait_idle(Duration::from_secs(1800)));
    let j = job(&q, &id);
    assert_eq!(j.status, JobStatus::Done, "{:#?}", j.events);
    let events = sink.progress.lock().unwrap().len();
    eprintln!(
        "1 小时素材用时 {:.0} 秒，进度推送 {events} 次，任务事件 {} 条，日志 {} 行",
        started.elapsed().as_secs_f64(),
        j.events.len(),
        j.log.len()
    );
    assert!(j.events.len() < 10 && j.log.len() <= 500, "任务状态不应随时长增长");
}

#[test]
fn hdr10_fidelity_report_passes_for_x265_qsv_and_svtav1() {
    // 计划验收第 8 条：同一 HDR10 源分别用 libx265、hevc_qsv、libsvtav1 输出，报告都判定 HDR10 已保留。
    // AV1 的母版亮度定点分母与 HEVC 不同（256 对 10000），按数值比较才不会误报
    let e = env_or_skip!();
    let src = e.synth(
        "hdr10.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=640x360:r=24:d=2",
            "-pix_fmt",
            "yuv420p10le",
            "-c:v",
            "libx265",
            "-x265-params",
            "log-level=error:hdr10=1:repeat-headers=1:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:\
master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1):max-cll=1000,400",
        ],
    );
    let caps = e.caps.clone();
    let mut encoders = vec![EncoderId::Libx265, EncoderId::Libsvtav1];
    if caps.encoder_usable(EncoderId::HevcQsv) {
        encoders.push(EncoderId::HevcQsv);
    }
    let items: Vec<QueueItem> = encoders
        .iter()
        .map(|&enc| {
            let mut plan = switch_encoder(fast(&src, Scenario::Archive, &caps), enc, &src, &caps);
            plan.video.preset = fast(&src, Scenario::Archive, &caps).video.preset;
            if enc == EncoderId::Libsvtav1 {
                plan.video.preset = "12".into();
            }
            plan.video.bit_depth = 10;
            plan.fidelity.hdr10 = true;
            QueueItem { media: src.clone(), plan }
        })
        .collect();
    let q = e.queue(Arc::new(Sink::default()));
    q.set_environment(caps, e.settings());
    let ids = q.add(items);
    assert!(q.wait_idle(Duration::from_secs(180)));
    for (id, enc) in ids.iter().zip(&encoders) {
        let j = job(&q, id);
        assert_eq!(j.status, JobStatus::Done, "{enc:?}：{:#?}", j.events);
        let report = j.report.unwrap();
        let hdr = report.iter().find(|r| r.label == "HDR10").unwrap_or_else(|| panic!("{enc:?} 报告里没有 HDR10 项"));
        assert!(hdr.ok, "{enc:?}：{hdr:?}");
        assert!(report.iter().all(|r| r.ok), "{enc:?}：{report:#?}");
    }
}
