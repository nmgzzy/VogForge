//! 队列调度的行为测试：外部进程换成脚本化的假实现，逐条核对设计文档 4.2 / 4.7 的规则——
//! 并发票据、取消不留残留、暂停与全部暂停、按失败分类回退、运行很久后失败改软编重跑、
//! 两遍编码、同名冲突、崩溃后恢复。真实 ffmpeg 上的端到端见 `queue_real.rs`。

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use vidforge_core::config::{ConflictPolicy, Settings};
use vidforge_core::ffmpeg::probe::ProbeError;
use vidforge_core::i18n::Lang;
use vidforge_core::model::{
    Capabilities, EncoderId, EncoderProbe, EventLevel, Job, JobProgressEvent, JobStatus, MediaInfo, QueueItem, QueueOp,
    QueueSnapshot, RateControl, Scenario, TranscodePlan, Vendor,
};
use vidforge_core::pipeline::recommend_plan;
use vidforge_core::pipeline::strategy::switch_encoder;
use vidforge_core::queue::process::{Process, ProcessControl, ProcessExit, Tools};
use vidforge_core::queue::{Clock, EventSink, Queue, QueueDeps};

const WAIT: Duration = Duration::from_secs(10);

// ───────────────── 假的时钟、进程与工具 ─────────────────

struct FakeClock(AtomicU64);

impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

impl FakeClock {
    fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

/// 关着的闸门让假进程停在最后一块进度之前，测试借此观察"正在运行"的状态
#[derive(Default)]
struct Gate {
    closed: Mutex<bool>,
    cv: Condvar,
}

impl Gate {
    fn set(&self, closed: bool) {
        *self.closed.lock().unwrap() = closed;
        self.cv.notify_all();
    }
}

#[derive(Default)]
struct Flags {
    killed: bool,
    suspended: bool,
}

struct FakeControl {
    flags: Mutex<Flags>,
    cv: Condvar,
    suspends: Arc<AtomicUsize>,
}

impl ProcessControl for FakeControl {
    fn kill(&self) {
        self.flags.lock().unwrap().killed = true;
        self.cv.notify_all();
    }
    fn suspend(&self) -> bool {
        self.flags.lock().unwrap().suspended = true;
        self.suspends.fetch_add(1, Ordering::SeqCst);
        true
    }
    fn resume(&self) -> bool {
        self.flags.lock().unwrap().suspended = false;
        self.cv.notify_all();
        true
    }
}

struct FakeProcess {
    lines: Vec<String>,
    /// 没有输出的命令（预检、响度测量）在这里停住，直到闸门打开或被结束
    hold: Option<Arc<Gate>>,
    pos: usize,
    control: Arc<FakeControl>,
    gate: Arc<Gate>,
    clock: Arc<FakeClock>,
    /// 失败时的 stderr 与失败前推进的时间
    fail: Option<(String, u64)>,
    output: Option<PathBuf>,
    passlog: Option<PathBuf>,
    live: Arc<AtomicUsize>,
}

impl Process for FakeProcess {
    fn next_line(&mut self) -> Option<String> {
        let c = self.control.clone();
        let mut f = c.flags.lock().unwrap();
        while f.suspended && !f.killed {
            f = c.cv.wait(f).unwrap();
        }
        if f.killed {
            return None;
        }
        drop(f);
        if self.pos >= self.lines.len() {
            if let Some(gate) = self.hold.take() {
                let mut closed = gate.closed.lock().unwrap();
                while *closed && !self.control.flags.lock().unwrap().killed {
                    closed = gate.cv.wait_timeout(closed, Duration::from_millis(20)).unwrap().0;
                }
            }
            return None;
        }
        if self.pos == self.lines.len() - 1 {
            // 最后一行（progress=end）之前停在闸门处
            let mut closed = self.gate.closed.lock().unwrap();
            while *closed && !self.control.flags.lock().unwrap().killed {
                closed = self.gate.cv.wait_timeout(closed, Duration::from_millis(20)).unwrap().0;
            }
            if self.control.flags.lock().unwrap().killed {
                return None;
            }
        }
        self.clock.advance(100);
        self.pos += 1;
        Some(self.lines[self.pos - 1].clone())
    }

    fn wait(&mut self) -> io::Result<ProcessExit> {
        self.live.fetch_sub(1, Ordering::SeqCst);
        if self.control.flags.lock().unwrap().killed {
            return Ok(ProcessExit {
                success: false,
                code: None,
                stderr: vec!["Exiting normally, received signal".into()],
            });
        }
        if let Some((stderr, advance)) = self.fail.take() {
            self.clock.advance(advance);
            return Ok(ProcessExit { success: false, code: Some(1), stderr: vec![stderr] });
        }
        if let Some(p) = &self.passlog {
            std::fs::write(p, b"stats").unwrap();
        }
        if let Some(out) = &self.output {
            std::fs::write(out, b"video").unwrap();
        }
        Ok(ProcessExit { success: true, code: Some(0), stderr: vec!["done".into()] })
    }

    fn control(&self) -> Arc<dyn ProcessControl> {
        self.control.clone()
    }
}

#[derive(Default)]
struct Record {
    spawned: Vec<Vec<String>>,
    dry_runs: Vec<Vec<String>>,
}

struct FakeTools {
    clock: Arc<FakeClock>,
    gate: Arc<Gate>,
    record: Mutex<Record>,
    /// 预检失败：命令里含这个编码器名时返回的 stderr
    dry_fail: Mutex<HashMap<String, String>>,
    /// 关着时预检停住不结束
    dry_gate: Arc<Gate>,
    /// 编码失败：编码器名 → (stderr, 失败前推进的毫秒)
    run_fail: Mutex<HashMap<String, (String, u64)>>,
    live: Arc<AtomicUsize>,
    max_live: AtomicUsize,
    suspends: Arc<AtomicUsize>,
    probe: Mutex<Option<MediaInfo>>,
    /// 数帧（ffprobe -count_packets）返回的帧数与它的闸门：关着时停在输出帧数之前
    count: Mutex<Option<u64>>,
    count_gate: Arc<Gate>,
    counted: AtomicUsize,
}

fn encoder_of(args: &[String]) -> String {
    args.iter().position(|a| a == "-c:v").map(|i| args[i + 1].clone()).unwrap_or_default()
}

impl Tools for FakeTools {
    fn spawn(&self, _program: &Path, args: &[String]) -> io::Result<Box<dyn Process>> {
        let control =
            Arc::new(FakeControl { flags: Mutex::default(), cv: Condvar::new(), suspends: self.suspends.clone() });
        // 预检：输入是测试图，没有进度输出
        if args.iter().any(|a| a.starts_with("testsrc2=")) {
            self.record.lock().unwrap().dry_runs.push(args.to_vec());
            self.live.fetch_add(1, Ordering::SeqCst);
            let err = self.dry_fail.lock().unwrap().get(&encoder_of(args)).cloned();
            return Ok(Box::new(FakeProcess {
                lines: Vec::new(),
                hold: Some(self.dry_gate.clone()),
                pos: 0,
                control,
                gate: self.gate.clone(),
                clock: self.clock.clone(),
                fail: err.map(|e| (e, 0)),
                output: None,
                passlog: None,
                live: self.live.clone(),
            }));
        }
        // 校验时数帧：一行输出（帧数），在闸门处可以停住
        if args.iter().any(|a| a == "-count_packets") {
            self.counted.fetch_add(1, Ordering::SeqCst);
            self.live.fetch_add(1, Ordering::SeqCst);
            return Ok(Box::new(FakeProcess {
                lines: self.count.lock().unwrap().iter().map(|n| n.to_string()).collect(),
                hold: None,
                pos: 0,
                control,
                gate: self.count_gate.clone(),
                clock: self.clock.clone(),
                fail: None,
                output: None,
                passlog: None,
                live: self.live.clone(),
            }));
        }
        self.record.lock().unwrap().spawned.push(args.to_vec());
        let n = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_live.fetch_max(n, Ordering::SeqCst);
        let last = args.last().cloned().unwrap_or_default();
        let first_pass = args.windows(2).any(|w| w[0] == "-pass" && w[1] == "1");
        let passlog =
            args.iter().position(|a| a == "-passlogfile").map(|i| PathBuf::from(format!("{}-0.log", args[i + 1])));
        let mut lines = Vec::new();
        for i in 1..=5 {
            lines.push(format!("frame={}", i * 30));
            lines.push(format!("out_time_us={}", i * 2_000_000));
            lines.push("speed=2.0x".into());
            lines.push(format!("progress={}", if i == 5 { "end" } else { "continue" }));
        }
        Ok(Box::new(FakeProcess {
            lines,
            hold: None,
            pos: 0,
            control,
            gate: self.gate.clone(),
            clock: self.clock.clone(),
            fail: self.run_fail.lock().unwrap().get(&encoder_of(args)).cloned(),
            output: (!first_pass && last != "-").then(|| PathBuf::from(last)),
            passlog: first_pass.then_some(passlog).flatten(),
            live: self.live.clone(),
        }))
    }

    fn probe(&self, _ffprobe: &Path, _file: &Path) -> Result<MediaInfo, ProbeError> {
        self.probe.lock().unwrap().clone().ok_or_else(|| ProbeError::Failed("测试里不分析输出".into()))
    }
}

#[derive(Default)]
struct Sink {
    snapshots: AtomicUsize,
    progress: Mutex<Vec<JobProgressEvent>>,
}

impl EventSink for Sink {
    fn snapshot(&self, _s: &QueueSnapshot) {
        self.snapshots.fetch_add(1, Ordering::SeqCst);
    }
    fn progress(&self, e: &JobProgressEvent) {
        self.progress.lock().unwrap().push(e.clone());
    }
}

// ───────────────── 测试环境 ─────────────────

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/samples/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn sample(id: &str) -> MediaInfo {
    let all: Vec<MediaInfo> = serde_json::from_str(&fixture("media.json")).unwrap();
    all.into_iter().find(|m| m.id == id).unwrap()
}

/// 开发机能力，另外假装 NVENC 可用（用来测回退链）
fn caps() -> Capabilities {
    let mut c: Capabilities = serde_json::from_str(&fixture("capabilities.json")).unwrap();
    c.ffmpeg_path = "ffmpeg".into();
    for e in c.encoders.iter_mut().filter(|e| e.vendor == Vendor::Nvidia) {
        (e.usable, e.ten_bit, e.error, e.failure) = (true, true, None, None);
    }
    if !c.encoders.iter().any(|e| e.id == EncoderId::HevcNvenc) {
        let id = EncoderId::HevcNvenc;
        c.encoders.push(EncoderProbe {
            id,
            vendor: id.vendor(),
            codec: id.codec(),
            usable: true,
            ten_bit: true,
            error: None,
            failure: None,
        });
    }
    c
}

struct Harness {
    queue: Queue,
    tools: Arc<FakeTools>,
    sink: Arc<Sink>,
    dir: PathBuf,
    settings: Settings,
    _tmp: Option<tempfile::TempDir>,
}

fn fake_tools(clock: Arc<FakeClock>) -> Arc<FakeTools> {
    Arc::new(FakeTools {
        clock: clock.clone(),
        gate: Arc::default(),
        record: Mutex::default(),
        dry_fail: Mutex::default(),
        dry_gate: Arc::default(),
        run_fail: Mutex::default(),
        live: Arc::default(),
        max_live: AtomicUsize::new(0),
        suspends: Arc::default(),
        probe: Mutex::default(),
        count: Mutex::default(),
        count_gate: Arc::default(),
        counted: AtomicUsize::new(0),
    })
}

impl Harness {
    fn new() -> Harness {
        let tmp = tempfile::tempdir().unwrap();
        Harness { _tmp: Some(tmp), ..Harness::at(Path::new(""), false) }.rebased()
    }

    /// 在指定目录里建队列；`persist` 时队列写到该目录的 queue.json
    fn at(dir: &Path, persist: bool) -> Harness {
        let clock = Arc::new(FakeClock(AtomicU64::new(1_700_000_000_000)));
        let tools = fake_tools(clock.clone());
        let sink = Arc::new(Sink::default());
        let store = persist.then(|| dir.join("queue.json"));
        let queue =
            Queue::start(QueueDeps { tools: tools.clone(), clock, sink: sink.clone(), store, lang: Lang::ZhCn });
        let settings =
            Settings { output_dir: Some(dir.join("out").to_string_lossy().to_string()), ..Settings::default() };
        Harness { queue, tools, sink, dir: dir.to_path_buf(), settings, _tmp: None }
    }

    /// `new` 先建好临时目录再把输出目录指过去
    fn rebased(mut self) -> Harness {
        let dir = self._tmp.as_ref().unwrap().path().to_path_buf();
        self.settings.output_dir = Some(dir.join("out").to_string_lossy().to_string());
        self.dir = dir;
        self
    }

    fn ready(&self) {
        self.queue.set_environment(caps(), self.settings.clone());
    }

    fn add(&self, media: &MediaInfo, plan: TranscodePlan) -> String {
        self.queue.add(vec![QueueItem { media: media.clone(), plan }]).remove(0)
    }

    fn job(&self, id: &str) -> Job {
        self.queue.snapshot().jobs.into_iter().find(|j| j.id == id).unwrap()
    }

    fn wait_for(&self, what: &str, f: impl Fn(&QueueSnapshot) -> bool) {
        let deadline = Instant::now() + WAIT;
        while !f(&self.queue.snapshot()) {
            assert!(
                Instant::now() < deadline,
                "等不到：{what}\n{:#?}",
                self.queue.snapshot().jobs.iter().map(|j| (&j.id, j.status, &j.events)).collect::<Vec<_>>()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// 等到 ffmpeg 真正跑起来（收到第一块进度），而不只是被调度器标为运行中
    fn wait_progress(&self, id: &str) {
        let deadline = Instant::now() + WAIT;
        while !self.sink.progress.lock().unwrap().iter().any(|e| e.id == id) {
            assert!(Instant::now() < deadline, "{id} 一直没有进度");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_status(&self, id: &str, status: JobStatus) {
        self.wait_for(&format!("{id} → {status:?}"), |s| s.jobs.iter().any(|j| j.id == id && j.status == status));
    }

    fn files(&self) -> Vec<String> {
        let out = self.dir.join("out");
        let mut names: Vec<String> = std::fs::read_dir(out)
            .map(|rd| rd.map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }
}

fn plan(media: &MediaInfo, enc: EncoderId) -> TranscodePlan {
    switch_encoder(recommend_plan(media, Scenario::Archive, &caps()), enc, media, &caps())
}

fn messages(job: &Job, level: EventLevel) -> Vec<String> {
    job.events.iter().filter(|e| e.level == level).map(|e| e.message.clone()).collect()
}

/// 事件里附带的 ffmpeg 原文（界面上可展开）
fn details(job: &Job, level: EventLevel) -> Vec<String> {
    job.events.iter().filter(|e| e.level == level).filter_map(|e| e.detail.clone()).collect()
}

// ───────────────── 测试 ─────────────────

#[test]
fn nothing_runs_until_the_environment_is_ready() {
    let h = Harness::new();
    let drone = sample("m-drone");
    let id = h.add(&drone, plan(&drone, EncoderId::Libx265));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(h.job(&id).status, JobStatus::Queued);
    h.ready();
    h.wait_status(&id, JobStatus::Done);
}

#[test]
fn jobs_finish_with_final_names_and_no_temp_files() {
    let h = Harness::new();
    h.ready();
    let drone = sample("m-drone");
    let ids: Vec<String> = (0..3).map(|_| h.add(&drone, plan(&drone, EncoderId::Libx265))).collect();
    assert!(h.queue.wait_idle(WAIT));
    for id in &ids {
        let j = h.job(id);
        assert_eq!(j.status, JobStatus::Done, "{:?}", j.events);
        assert_eq!(j.progress.percent, 100.0);
        assert!(!j.args.is_empty() && j.args.last().unwrap().ends_with(".vidforge-part"));
        assert!(Path::new(&j.output_path).exists());
    }
    // 同名的三个任务：第二、三个按"自动改名"写成 (1)、(2)
    let files = h.files();
    assert_eq!(
        files,
        [
            "DJI_20260812_0142_2160p_hevc (1).mkv",
            "DJI_20260812_0142_2160p_hevc (2).mkv",
            "DJI_20260812_0142_2160p_hevc.mkv"
        ]
    );
    assert_eq!(h.tools.max_live.load(Ordering::SeqCst), 1, "一张 CPU 票：软编逐个运行");
    assert!(!h.sink.progress.lock().unwrap().is_empty());
}

#[test]
fn cpu_and_gpu_tickets_run_side_by_side() {
    let h = Harness::new();
    h.tools.gate.set(true);
    h.ready();
    let drone = sample("m-drone");
    let cpu: Vec<String> = (0..2).map(|_| h.add(&drone, plan(&drone, EncoderId::Libx265))).collect();
    let gpu: Vec<String> = (0..2).map(|_| h.add(&drone, plan(&drone, EncoderId::HevcQsv))).collect();
    h.wait_for("一个软编加一个硬编同时运行", |s| {
        s.jobs.iter().filter(|j| j.status == JobStatus::Running).count() == 2
    });
    std::thread::sleep(Duration::from_millis(100));
    let running: Vec<String> =
        h.queue.snapshot().jobs.into_iter().filter(|j| j.status == JobStatus::Running).map(|j| j.id).collect();
    assert_eq!(running, [cpu[0].clone(), gpu[0].clone()]);
    h.tools.gate.set(false);
    assert!(h.queue.wait_idle(WAIT));
    assert_eq!(h.tools.max_live.load(Ordering::SeqCst), 2);
    // 硬件编码器开跑前做过预检
    assert!(messages(&h.job(&gpu[0]), EventLevel::Info).iter().any(|m| m.contains("预检通过")));
}

#[test]
fn cancel_kills_the_process_and_leaves_nothing_behind() {
    let h = Harness::new();
    h.tools.gate.set(true);
    h.ready();
    let drone = sample("m-drone");
    let id = h.add(&drone, plan(&drone, EncoderId::Libx265));
    let queued = h.add(&drone, plan(&drone, EncoderId::Libx265));
    h.wait_status(&id, JobStatus::Running);
    h.queue.apply(QueueOp::Cancel { id: queued.clone() }).unwrap();
    assert_eq!(h.job(&queued).status, JobStatus::Cancelled, "排队中的任务直接取消");
    h.queue.apply(QueueOp::Cancel { id: id.clone() }).unwrap();
    h.wait_status(&id, JobStatus::Cancelled);
    assert!(h.files().is_empty(), "目标目录不应有残留：{:?}", h.files());
    assert!(h.queue.apply(QueueOp::Cancel { id }).is_err(), "已结束的任务不能再取消");
}

#[test]
fn pause_resume_and_global_pause() {
    let h = Harness::new();
    h.tools.gate.set(true);
    h.ready();
    let drone = sample("m-drone");
    let a = h.add(&drone, plan(&drone, EncoderId::Libx265));
    let b = h.add(&drone, plan(&drone, EncoderId::Libx265));
    h.wait_progress(&a);

    h.queue.apply(QueueOp::Pause { id: a.clone() }).unwrap();
    assert_eq!(h.job(&a).status, JobStatus::Paused);
    assert_eq!(h.tools.suspends.load(Ordering::SeqCst), 1, "暂停是挂起进程");
    assert!(h.queue.apply(QueueOp::Pause { id: a.clone() }).is_err());
    h.queue.apply(QueueOp::Resume { id: a.clone() }).unwrap();
    assert_eq!(h.job(&a).status, JobStatus::Running);

    // 全部暂停：进行中的挂起，排队的不开始
    h.queue.apply(QueueOp::SetPaused { paused: true }).unwrap();
    assert!(h.queue.snapshot().paused);
    assert_eq!(h.job(&a).status, JobStatus::Paused);
    h.tools.gate.set(false);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!((h.job(&a).status, h.job(&b).status), (JobStatus::Paused, JobStatus::Queued));
    h.queue.apply(QueueOp::SetPaused { paused: false }).unwrap();
    assert!(h.queue.wait_idle(WAIT));
    assert_eq!((h.job(&a).status, h.job(&b).status), (JobStatus::Done, JobStatus::Done));
}

#[test]
fn device_missing_falls_back_along_the_chain_and_disables_the_vendor() {
    let h = Harness::new();
    h.tools.dry_fail.lock().unwrap().insert("hevc_nvenc".into(), "[hevc_nvenc @ 0x1] Cannot load nvcuda.dll".into());
    h.ready();
    let drone = sample("m-drone");
    let first = h.add(&drone, plan(&drone, EncoderId::HevcNvenc));
    h.wait_status(&first, JobStatus::Done);
    let j = h.job(&first);
    assert_eq!(j.encoder_used, EncoderId::HevcQsv);
    let warn = messages(&j, EventLevel::Warn);
    assert!(warn.iter().any(|m| m.contains("回退到 hevc_qsv") && m.contains("本次运行不再使用")), "{warn:?}");
    assert!(!warn.iter().any(|m| m.contains("nvcuda")), "说明里不直接抛原始报错：{warn:?}");
    let fallback = j.events.iter().find(|e| e.message.contains("回退到 hevc_qsv")).unwrap();
    assert_eq!(
        fallback.detail.as_deref(),
        Some("[hevc_nvenc @ 0x1] Cannot load nvcuda.dll"),
        "原文放在可展开的 detail"
    );
    // 输出读不出来（测试里不分析输出）：完成事件带上 ffprobe 的原文
    assert_eq!(j.events.last().unwrap().detail.as_deref(), Some("测试里不分析输出"));
    assert!(!h.files().iter().any(|f| f.contains("vidforge-part")));

    // 本次运行已停用 NVIDIA：之后的任务不再试它
    let before = h.tools.record.lock().unwrap().dry_runs.len();
    let second = h.add(&drone, plan(&drone, EncoderId::HevcNvenc));
    h.wait_status(&second, JobStatus::Done);
    let dry = h.tools.record.lock().unwrap().dry_runs[before..].to_vec();
    assert!(dry.iter().all(|a| encoder_of(a) != "hevc_nvenc"), "{dry:?}");
    assert_ne!(h.job(&second).encoder_used, EncoderId::HevcNvenc);
}

#[test]
fn late_failures_restart_on_software_and_say_so() {
    let h = Harness::new();
    h.tools.run_fail.lock().unwrap().insert("hevc_qsv".into(), ("[hevc_qsv] something odd".into(), 15_000));
    h.ready();
    let drone = sample("m-drone");
    let id = h.add(&drone, plan(&drone, EncoderId::HevcQsv));
    h.wait_status(&id, JobStatus::Done);
    let j = h.job(&id);
    assert_eq!(j.encoder_used, EncoderId::Libx265);
    let warn = messages(&j, EventLevel::Warn).join(" / ");
    assert!(warn.contains("秒后失败") && warn.contains("已删除部分输出") && warn.contains("libx265"), "{warn}");
    assert_eq!(j.attempts, 2);
}

#[test]
fn software_failures_are_final_and_retry_starts_over() {
    let h = Harness::new();
    h.tools.run_fail.lock().unwrap().insert("libx265".into(), ("Conversion failed!".into(), 0));
    h.ready();
    let drone = sample("m-drone");
    let id = h.add(&drone, plan(&drone, EncoderId::Libx265));
    h.wait_status(&id, JobStatus::Failed);
    let j = h.job(&id);
    let error = &messages(&j, EventLevel::Error)[0];
    assert!(error.starts_with("libx265 失败：ffmpeg 执行失败"), "{error}");
    assert_eq!(details(&j, EventLevel::Error), ["Conversion failed!"]);
    assert_eq!(j.log, ["Conversion failed!"]);
    assert!(h.files().is_empty());
    assert!(h.queue.apply(QueueOp::Remove { id: "nope".into() }).is_err());

    h.tools.run_fail.lock().unwrap().clear();
    h.queue.apply(QueueOp::Retry { id: id.clone() }).unwrap();
    h.wait_status(&id, JobStatus::Done);
    assert_eq!(h.job(&id).attempts, 1, "手动重试开始新的一轮，自动回退的次数重新计");
}

#[test]
fn two_pass_runs_both_passes_and_removes_the_stats_files() {
    let h = Harness::new();
    h.ready();
    let drone = sample("m-drone");
    let mut p = plan(&drone, EncoderId::Libx265);
    p.video.rate_control = RateControl::TwoPass { kbps: 8000 };
    let id = h.add(&drone, p);
    h.wait_status(&id, JobStatus::Done);
    let spawned = h.tools.record.lock().unwrap().spawned.clone();
    assert_eq!(spawned.len(), 2);
    assert!(spawned[0].windows(2).any(|w| w == ["-pass", "1"]) && spawned[1].windows(2).any(|w| w == ["-pass", "2"]));
    assert_eq!(h.files(), ["DJI_20260812_0142_2160p_hevc.mkv"], "统计文件应已删除");
    let passes: Vec<Option<u8>> = h.sink.progress.lock().unwrap().iter().map(|e| e.progress.pass).collect();
    assert!(passes.contains(&Some(1)) && passes.contains(&Some(2)));
    // 两遍合并的百分比单调不减
    let pct: Vec<f64> = h.sink.progress.lock().unwrap().iter().map(|e| e.progress.percent).collect();
    assert!(pct.windows(2).all(|w| w[1] >= w[0]), "{pct:?}");
}

#[test]
fn conflict_policy_skip_keeps_the_existing_file() {
    let mut h = Harness::new();
    h.settings.conflict = ConflictPolicy::Skip;
    h.ready();
    let out = h.dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("DJI_20260812_0142_2160p_hevc.mkv"), b"mine").unwrap();
    let drone = sample("m-drone");
    let id = h.add(&drone, plan(&drone, EncoderId::Libx265));
    h.wait_status(&id, JobStatus::Skipped);
    assert_eq!(std::fs::read(out.join("DJI_20260812_0142_2160p_hevc.mkv")).unwrap(), b"mine");
    assert!(h.tools.record.lock().unwrap().spawned.is_empty(), "跳过的任务不启动 ffmpeg");
}

#[test]
fn hardware_encoding_disabled_in_settings_uses_software() {
    let mut h = Harness::new();
    h.settings.hw_encode = false;
    h.ready();
    let drone = sample("m-drone");
    let id = h.add(&drone, plan(&drone, EncoderId::HevcQsv));
    h.wait_status(&id, JobStatus::Done);
    assert_eq!(h.job(&id).encoder_used, EncoderId::Libx265);
    assert!(h.tools.record.lock().unwrap().dry_runs.is_empty(), "软编不需要预检");
}

#[test]
fn job_messages_follow_the_interface_language() {
    let mut h = Harness::new();
    h.settings.language = Lang::En;
    h.tools.dry_fail.lock().unwrap().insert("hevc_nvenc".into(), "[hevc_nvenc @ 0x1] Cannot load nvcuda.dll".into());
    h.ready();
    let drone = sample("m-drone");
    *h.tools.probe.lock().unwrap() = Some(drone.clone());
    let id = h.add(&drone, plan(&drone, EncoderId::HevcNvenc));
    h.wait_status(&id, JobStatus::Done);
    let j = h.job(&id);
    let all: Vec<&str> = j.events.iter().map(|e| e.message.as_str()).collect();
    let han = |m: &&str| m.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    assert!(!all.iter().any(han), "全部是英文：{all:?}");
    assert!(all.iter().any(|m| m.starts_with("Done")), "{all:?}");
    let report = j.report.as_ref().expect("有校验报告");
    assert!(report.iter().any(|r| r.label == "Duration"), "校验报告也跟随语言：{report:?}");
}

#[test]
fn only_verified_sources_can_go_to_the_trash() {
    let mut h = Harness::new();
    let dir = h.dir.join("src");
    std::fs::create_dir_all(&dir).unwrap();
    let mut m = sample("m-drone");
    m.path = dir.join("clip.mp4").to_string_lossy().to_string();
    m.name = "clip.mp4".into();
    std::fs::write(&m.path, b"source").unwrap();
    // 输出分析结果与计划一致：校验全部通过
    let p = plan(&m, EncoderId::Libx265);
    let mut out = m.clone();
    out.video[0].codec = "hevc".into();
    *h.tools.probe.lock().unwrap() = Some(out);
    h.settings.output_dir = Some(dir.join("out").to_string_lossy().to_string());
    h.ready();
    let ok = h.add(&m, p.clone());
    h.wait_status(&ok, JobStatus::Done);
    let report = h.job(&ok).report.unwrap();
    assert!(report.iter().all(|r| r.ok), "{report:?}");
    assert_eq!(h.queue.trashable_sources(std::slice::from_ref(&ok)), [PathBuf::from(&m.path)]);

    // 同一个源还有排队中的任务：不能动
    h.queue.apply(QueueOp::SetPaused { paused: true }).unwrap();
    let pending = h.add(&m, p.clone());
    assert!(h.queue.trashable_sources(std::slice::from_ref(&ok)).is_empty(), "还有任务要用这个源文件");
    h.queue.apply(QueueOp::Cancel { id: pending }).unwrap();
    assert_eq!(h.queue.trashable_sources(std::slice::from_ref(&ok)).len(), 1);

    // 校验没通过、没完成、源文件已不在：都不给
    *h.tools.probe.lock().unwrap() = None;
    h.queue.apply(QueueOp::SetPaused { paused: false }).unwrap();
    let unverified = h.add(&m, p);
    h.wait_status(&unverified, JobStatus::Done);
    assert!(h.queue.trashable_sources(std::slice::from_ref(&unverified)).is_empty(), "校验没通过");
    assert!(h.queue.trashable_sources(&["nope".into()]).is_empty());
    // 校验之后输出被删掉了：源文件是唯一的一份，不能再动
    let out = h.job(&ok).output_path;
    let saved = std::fs::read(&out).unwrap();
    std::fs::remove_file(&out).unwrap();
    assert!(h.queue.trashable_sources(std::slice::from_ref(&ok)).is_empty(), "输出已经不在");
    std::fs::write(&out, saved).unwrap();
    assert_eq!(h.queue.trashable_sources(std::slice::from_ref(&ok)).len(), 1);
    std::fs::remove_file(&m.path).unwrap();
    assert!(h.queue.trashable_sources(&[ok]).is_empty(), "源文件已经不在");
}

/// 输出没有帧数记录（ffmpeg 写的 MKV）时，校验阶段用可控的进程数帧，结果进报告
#[test]
fn frames_are_counted_with_a_controllable_process() {
    let h = Harness::new();
    let drone = sample("m-drone");
    let mut out = drone.clone();
    out.video[0].frame_count = None;
    *h.tools.probe.lock().unwrap() = Some(out);
    *h.tools.count.lock().unwrap() = drone.video[0].frame_count;
    h.ready();
    let id = h.add(&drone, plan(&drone, EncoderId::Libx265));
    h.wait_status(&id, JobStatus::Done);
    assert_eq!(h.tools.counted.load(Ordering::SeqCst), 1);
    let report = h.job(&id).report.unwrap();
    assert!(report.iter().any(|r| r.label == "帧数" && r.ok), "{report:#?}");
}

/// 核对期间点取消：数帧进程被结束，任务记为取消而不是完成，已写完的输出留着
#[test]
fn cancelling_during_verification_stops_the_count_and_keeps_the_output() {
    let h = Harness::new();
    let drone = sample("m-drone");
    let mut out = drone.clone();
    out.video[0].frame_count = None;
    *h.tools.probe.lock().unwrap() = Some(out);
    *h.tools.count.lock().unwrap() = Some(1);
    h.tools.count_gate.set(true);
    h.ready();
    let id = h.add(&drone, plan(&drone, EncoderId::Libx265));
    let deadline = Instant::now() + WAIT;
    while h.tools.counted.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "没有开始数帧");
        std::thread::sleep(Duration::from_millis(5));
    }
    h.queue.apply(QueueOp::Cancel { id: id.clone() }).unwrap();
    h.wait_status(&id, JobStatus::Cancelled);
    let j = h.job(&id);
    assert!(j.events.last().unwrap().message.contains("没有核对"), "{:?}", j.events);
    assert!(j.report.is_none());
    assert!(Path::new(&j.output_path).is_file(), "写完的输出不删");
}

#[test]
fn order_moves_and_clear_finished() {
    let h = Harness::new();
    let drone = sample("m-drone");
    let ids: Vec<String> = (0..3).map(|_| h.add(&drone, plan(&drone, EncoderId::Libx265))).collect();
    h.queue.apply(QueueOp::Move { id: ids[2].clone(), delta: -1 }).unwrap();
    let order: Vec<String> = h.queue.snapshot().jobs.into_iter().map(|j| j.id).collect();
    assert_eq!(order, [ids[0].clone(), ids[2].clone(), ids[1].clone()]);
    h.queue.apply(QueueOp::Cancel { id: ids[0].clone() }).unwrap();
    h.queue.apply(QueueOp::ClearFinished).unwrap();
    assert_eq!(h.queue.snapshot().jobs.len(), 2);
    // 排队中的任务已经按环境算好了命令与输出路径，界面可以预览
    h.ready();
    assert!(h.job(&ids[1]).output_path.ends_with(".mkv"));
}

#[test]
fn a_crash_leaves_unfinished_jobs_queued_and_cleans_up() {
    let tmp = tempfile::tempdir().unwrap();
    let drone = sample("m-drone");
    let (id, part) = {
        let h = Harness::at(tmp.path(), true);
        h.tools.gate.set(true);
        h.ready();
        let id = h.add(&drone, plan(&drone, EncoderId::Libx265));
        h.wait_status(&id, JobStatus::Running);
        // 模拟崩溃时留下的临时文件；退出时不改任务状态
        let part = format!("{}.vidforge-part", h.job(&id).output_path);
        // 状态先变为进行中，输出目录随后才由工作线程创建
        std::fs::create_dir_all(Path::new(&part).parent().unwrap()).unwrap();
        std::fs::write(&part, b"half").unwrap();
        h.queue.shutdown();
        (id, part)
    };
    // 用同一个 queue.json 重启
    let h = Harness::at(tmp.path(), true);
    let job = h.job(&id);
    assert_eq!(job.status, JobStatus::Queued);
    assert!(job.events.iter().any(|e| e.level == EventLevel::Warn && e.message.contains("重新排队")));
    assert!(!Path::new(&part).exists(), "残留的临时文件应已删除");
    h.ready();
    h.wait_status(&id, JobStatus::Done);
    assert!(h.sink.snapshots.load(Ordering::SeqCst) > 0);
}

#[test]
fn the_source_file_is_never_overwritten() {
    // 输出目录就是源目录、命名模板是 {name}、扩展名相同、策略是覆盖：目标恰好等于源文件
    let mut h = Harness::new();
    let dir = h.dir.join("src");
    std::fs::create_dir_all(&dir).unwrap();
    let mut m = sample("m-drone");
    m.path = dir.join("DJI_20260812_0142.mkv").to_string_lossy().to_string();
    m.name = "DJI_20260812_0142.mkv".into();
    std::fs::write(&m.path, b"source").unwrap();
    h.settings.output_dir = Some(dir.to_string_lossy().to_string());
    h.settings.naming_template = "{name}".into();
    h.settings.conflict = ConflictPolicy::Overwrite;
    h.ready();
    let id = h.add(&m, plan(&m, EncoderId::Libx265));
    h.wait_status(&id, JobStatus::Done);
    assert_eq!(std::fs::read(&m.path).unwrap(), b"source", "源文件被改动了");
    let j = h.job(&id);
    assert!(j.output_path.ends_with("DJI_20260812_0142 (1).mkv"), "{}", j.output_path);
    assert!(messages(&j, EventLevel::Warn).iter().any(|w| w.contains("源文件不会被覆盖")));
}

#[test]
fn concurrent_jobs_never_share_a_target() {
    let mut h = Harness::new();
    h.settings.cpu_slots = 2;
    h.tools.gate.set(true);
    h.ready();
    let drone = sample("m-drone");
    let a = h.add(&drone, plan(&drone, EncoderId::Libx265));
    let b = h.add(&drone, plan(&drone, EncoderId::Libx265));
    h.wait_progress(&a);
    h.wait_progress(&b);
    let (pa, pb) = (h.job(&a).output_path, h.job(&b).output_path);
    assert_ne!(pa, pb, "两个同时运行的同名任务选了同一个目标");
    h.tools.gate.set(false);
    assert!(h.queue.wait_idle(WAIT));
    assert_eq!(h.files(), ["DJI_20260812_0142_2160p_hevc (1).mkv", "DJI_20260812_0142_2160p_hevc.mkv"]);
}

#[test]
fn a_hanging_pre_check_can_be_cancelled() {
    let h = Harness::new();
    h.tools.dry_gate.set(true);
    h.ready();
    let drone = sample("m-drone");
    let id = h.add(&drone, plan(&drone, EncoderId::HevcQsv));
    h.wait_for("预检开始", |_| !h.tools.record.lock().unwrap().dry_runs.is_empty());
    h.queue.apply(QueueOp::Cancel { id: id.clone() }).unwrap();
    h.wait_status(&id, JobStatus::Cancelled);
    assert!(h.tools.record.lock().unwrap().spawned.is_empty(), "取消后不应再开始编码");
}
