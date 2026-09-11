//! 转码队列（设计文档 4.2、4.7）：并发票据调度、任务执行、运行时回退、持久化。
//!
//! 结构：一个调度线程按票据挑选排队任务，每个开跑的任务一个工作线程（[`worker`]）。界面通过
//! [`Queue::apply`] 操作队列，状态变化经 [`EventSink`] 推送：结构或状态变化推完整快照，运行中的
//! 进度单独推。外部进程与时钟经 trait 注入，测试用脚本化的假实现。

pub mod fallback;
pub mod files;
pub mod persist;
pub mod process;
mod worker;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::config::Settings;
use crate::i18n::{Lang, pick};
use crate::model::{
    Capabilities, EnvStatus, EventLevel, Job, JobEvent, JobProgress, JobProgressEvent, JobStatus, QueueItem, QueueOp,
    QueueSnapshot, StreamAction, TranscodePlan, Vendor,
};
use crate::pipeline::args::{build_arg_segments, build_first_pass, flatten};
use crate::pipeline::update_plan;
use crate::tr;

use self::fallback::Tried;
use self::process::{ProcessControl, Tools};

pub trait EventSink: Send + Sync {
    fn snapshot(&self, snapshot: &QueueSnapshot);
    fn progress(&self, event: &JobProgressEvent);
}

pub trait Clock: Send + Sync {
    /// Unix 毫秒
    fn now_ms(&self) -> u64;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        crate::util::now_millis()
    }
}

pub struct QueueDeps {
    pub tools: Arc<dyn Tools>,
    pub clock: Arc<dyn Clock>,
    pub sink: Arc<dyn EventSink>,
    /// `queue.json` 的位置；None 时不持久化
    pub store: Option<PathBuf>,
    /// 环境就绪之前用的界面语言（之后跟随设置）
    pub lang: Lang,
}

/// 调度与执行依据的环境：探测结果与用户设置
#[derive(Debug, Clone)]
pub struct Environment {
    pub caps: Capabilities,
    pub settings: Settings,
}

impl Environment {
    /// 决策引擎实际使用的能力：按设置关掉硬件编解码，去掉本次运行停用的厂商
    fn effective(&self, disabled: &[Vendor]) -> Capabilities {
        self.caps.restricted(self.settings.hw_encode, self.settings.hw_decode, disabled, self.settings.language)
    }
}

/// 一个任务最多执行这么多次（含回退重跑），防止回退链异常时无限循环
pub const MAX_ATTEMPTS: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Ticket {
    /// 软件编码：x265 自己会吃满多核
    Cpu,
    Gpu,
    /// 原样封装：只受磁盘读写限制
    Io,
}

fn ticket(plan: &TranscodePlan) -> Ticket {
    if plan.video.action == StreamAction::Copy {
        Ticket::Io
    } else if plan.video.encoder.is_hardware() {
        Ticket::Gpu
    } else {
        Ticket::Cpu
    }
}

#[derive(Default)]
struct State {
    jobs: Vec<Job>,
    paused: bool,
    env: Option<Environment>,
    /// 运行中任务当前那个 ffmpeg 进程
    controls: HashMap<String, Arc<dyn ProcessControl>>,
    cancel: HashSet<String>,
    /// 被"全部暂停"挂起的任务，全部继续时只恢复这些
    held: HashSet<String>,
    /// 暂停开始时刻与累计暂停时长：算速度与剩余时间时扣掉
    pause_since: HashMap<String, u64>,
    paused_ms: HashMap<String, u64>,
    tried: HashMap<String, Tried>,
    /// 资源不足退避：这个时刻之前不重新开始
    not_before: HashMap<String, u64>,
    /// 各运行中任务选定的最终路径：别的任务不能再选同一个
    reserved: HashMap<String, PathBuf>,
    /// 本次运行中设备缺失的厂商
    disabled: Vec<Vendor>,
    /// 资源不足后 GPU 任务逐个运行
    serial_gpu: bool,
    shutdown: bool,
    seq: u64,
}

impl State {
    fn job_mut(&mut self, id: &str) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    fn snapshot(&self) -> QueueSnapshot {
        QueueSnapshot { jobs: self.jobs.clone(), paused: self.paused }
    }

    fn paused_total(&self, id: &str, now: u64) -> u64 {
        self.paused_ms.get(id).copied().unwrap_or(0)
            + self.pause_since.get(id).map_or(0, |since| now.saturating_sub(*since))
    }

    fn end_pause(&mut self, id: &str, now: u64) {
        if let Some(since) = self.pause_since.remove(id) {
            *self.paused_ms.entry(id.to_string()).or_default() += now.saturating_sub(since);
        }
    }
}

struct Inner {
    deps: QueueDeps,
    state: Mutex<State>,
    wake: Condvar,
    /// 串行化快照的推送与保存，保证落盘顺序与状态变化顺序一致
    publish_lock: Mutex<()>,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now(&self) -> u64 {
        self.deps.clock.now_ms()
    }

    /// 推送完整快照并保存
    fn publish(&self) {
        let _order = self.publish_lock.lock().unwrap_or_else(|e| e.into_inner());
        let snap = self.lock().snapshot();
        self.deps.sink.snapshot(&snap);
        if let Some(path) = &self.deps.store {
            let _ = persist::save(path, &snap);
        }
    }

    /// 当前界面语言：环境就绪后跟随设置
    fn lang(&self, s: &State) -> Lang {
        s.env.as_ref().map_or(self.deps.lang, |e| e.settings.language)
    }

    fn event(&self, s: &mut State, id: &str, level: EventLevel, message: impl Into<String>) {
        self.event_detail(s, id, level, message, None);
    }

    /// 带 ffmpeg 原文的事件：界面上正文是说明，原文可展开
    fn event_detail(
        &self,
        s: &mut State,
        id: &str,
        level: EventLevel,
        message: impl Into<String>,
        detail: Option<String>,
    ) {
        let at = self.now();
        if let Some(j) = s.job_mut(id) {
            j.events.push(JobEvent { at, level, message: message.into(), detail: detail.filter(|d| !d.is_empty()) });
        }
    }

    /// 任务结束：状态、时间、事件，清理调度用的临时状态
    fn finish(&self, id: &str, status: JobStatus, level: EventLevel, message: impl Into<String>) {
        self.finish_detail(id, status, level, message, None);
    }

    fn finish_detail(
        &self,
        id: &str,
        status: JobStatus,
        level: EventLevel,
        message: impl Into<String>,
        detail: Option<String>,
    ) {
        {
            let mut s = self.lock();
            let now = self.now();
            self.event_detail(&mut s, id, level, message, detail);
            if let Some(j) = s.job_mut(id) {
                j.status = status;
                j.finished_at = Some(now);
                j.progress.eta_sec = None;
            }
            s.cancel.remove(id);
            s.held.remove(id);
            s.reserved.remove(id);
            s.controls.remove(id);
            s.pause_since.remove(id);
            s.paused_ms.remove(id);
        }
        self.publish();
        self.wake.notify_all();
    }

    /// 按当前环境给排队中的任务算出预览命令与输出路径
    fn preview(&self, s: &mut State, id: &str) {
        let Some(env) = s.env.clone() else { return };
        let caps = env.effective(&s.disabled);
        let Some(job) = s.job_mut(id) else { return };
        let plan = update_plan(job.plan.clone(), &job.media, &caps);
        let out = crate::output::output_path(&job.media, &plan, &env.settings, &crate::output::today());
        let temp = files::temp_path(&out);
        job.args = flatten(&build_arg_segments(&job.media, &plan, &caps, &temp));
        job.first_pass = build_first_pass(&job.media, &plan, &caps, &temp).map(|s| flatten(&s));
        job.output_path = out.to_string_lossy().to_string();
        job.encoder_used = plan.video.encoder;
        job.plan = plan;
    }

    /// 挑出可以开始的任务并标为运行中
    fn pick(&self, s: &mut State) -> Vec<String> {
        let Some(env) = s.env.clone().filter(|e| e.caps.status == EnvStatus::Ready) else { return Vec::new() };
        if s.paused || s.shutdown {
            return Vec::new();
        }
        let caps = env.effective(&s.disabled);
        let limit = |t: Ticket, serial: bool| match t {
            Ticket::Cpu => env.settings.cpu_slots.max(1),
            Ticket::Gpu if serial => 1,
            Ticket::Gpu => env.settings.gpu_slots.max(1),
            Ticket::Io => 1,
        } as usize;
        let now = self.now();
        let mut used: HashMap<Ticket, usize> = HashMap::new();
        for j in s.jobs.iter().filter(|j| j.status.active()) {
            *used.entry(ticket(&j.plan)).or_default() += 1;
        }
        let mut started = Vec::new();
        let serial = s.serial_gpu;
        let waiting: Vec<String> = s
            .jobs
            .iter()
            .filter(|j| j.status == JobStatus::Queued && s.not_before.get(&j.id).is_none_or(|t| *t <= now))
            .map(|j| j.id.clone())
            .collect();
        for id in waiting {
            let job = s.job_mut(&id).expect("刚取出的任务");
            let plan = update_plan(job.plan.clone(), &job.media, &caps);
            let t = ticket(&plan);
            let n = used.entry(t).or_default();
            if *n >= limit(t, serial) {
                continue;
            }
            *n += 1;
            job.plan = plan;
            job.status = JobStatus::Running;
            job.started_at.get_or_insert(now);
            job.finished_at = None;
            job.progress = JobProgress::default();
            job.attempts += 1;
            started.push(id);
        }
        for id in &started {
            s.not_before.remove(id);
        }
        started
    }
}

pub struct Queue {
    inner: Arc<Inner>,
    scheduler: Mutex<Option<JoinHandle<()>>>,
}

impl Queue {
    /// 读入保存的队列（没跑完的任务重新排队），启动调度线程。环境就绪（[`Queue::set_environment`]）后才开始执行
    pub fn start(deps: QueueDeps) -> Queue {
        let mut snap =
            deps.store.as_deref().map(persist::load).unwrap_or(QueueSnapshot { jobs: Vec::new(), paused: false });
        let recovered = persist::recover(&mut snap.jobs, deps.clock.now_ms(), deps.lang);
        let state = State { jobs: snap.jobs, paused: snap.paused, ..Default::default() };
        let inner =
            Arc::new(Inner { deps, state: Mutex::new(state), wake: Condvar::new(), publish_lock: Mutex::new(()) });
        if recovered > 0 {
            inner.publish();
        }
        let worker_inner = inner.clone();
        let handle = thread::Builder::new()
            .name("vidforge-queue".into())
            .spawn(move || scheduler_loop(worker_inner))
            .expect("启动调度线程");
        Queue { inner, scheduler: Mutex::new(Some(handle)) }
    }

    /// 更新探测结果与设置（并发数、冲突策略、硬件开关……）
    pub fn set_environment(&self, caps: Capabilities, settings: Settings) {
        {
            let mut s = self.inner.lock();
            s.env = Some(Environment { caps, settings });
            let queued: Vec<String> =
                s.jobs.iter().filter(|j| j.status == JobStatus::Queued).map(|j| j.id.clone()).collect();
            for id in queued {
                self.inner.preview(&mut s, &id);
            }
        }
        self.inner.publish();
        self.inner.wake.notify_all();
    }

    pub fn snapshot(&self) -> QueueSnapshot {
        self.inner.lock().snapshot()
    }

    /// 可以移到回收站的源文件（需求 F-6.10）：只限已完成且校验全部通过、输出文件还在的任务，源文件还在、
    /// 不是输出本身，也没有其他没跑完的任务要用它。
    /// 界面传来的是任务 id，不接受任意路径，避免界面层误删别的文件
    pub fn trashable_sources(&self, ids: &[String]) -> Vec<PathBuf> {
        let s = self.inner.lock();
        let mut out: Vec<PathBuf> = Vec::new();
        for j in s.jobs.iter().filter(|j| ids.contains(&j.id)) {
            let verified = j.report.as_ref().is_some_and(|r| !r.is_empty() && r.iter().all(|x| x.ok));
            let src = PathBuf::from(&j.media.path);
            // 校验之后输出可能被删掉或移走：执行时再看一眼，输出不在就不动源文件
            let output_there = fs::metadata(&j.output_path).is_ok_and(|m| m.is_file() && m.len() > 0);
            let is_output = files::same_path(&src, Path::new(&j.output_path));
            let listed = out.iter().any(|p| files::same_path(p, &src));
            // 同一个源文件还有没跑完的任务（例如另一种用途）时不能动它
            let needed = s
                .jobs
                .iter()
                .any(|o| o.id != j.id && !o.status.finished() && files::same_path(Path::new(&o.media.path), &src));
            if j.status == JobStatus::Done
                && verified
                && output_there
                && src.is_file()
                && !is_output
                && !listed
                && !needed
            {
                out.push(src);
            }
        }
        out
    }

    /// 加入队列，返回新任务的 id
    pub fn add(&self, items: Vec<QueueItem>) -> Vec<String> {
        let ids = {
            let mut s = self.inner.lock();
            let now = self.inner.now();
            let lang = self.inner.lang(&s);
            let mut ids = Vec::new();
            for item in items {
                s.seq += 1;
                let id = format!("job-{now}-{}", s.seq);
                s.jobs.push(Job {
                    id: id.clone(),
                    encoder_used: item.plan.video.encoder,
                    media: item.media,
                    plan: item.plan,
                    args: Vec::new(),
                    first_pass: None,
                    output_path: String::new(),
                    status: JobStatus::Queued,
                    progress: JobProgress::default(),
                    events: vec![JobEvent {
                        at: now,
                        level: EventLevel::Info,
                        message: pick(lang, "已加入队列", "Added to the queue").into(),
                        detail: None,
                    }],
                    log: Vec::new(),
                    report: None,
                    started_at: None,
                    finished_at: None,
                    output_size: None,
                    attempts: 0,
                });
                self.inner.preview(&mut s, &id);
                ids.push(id);
            }
            ids
        };
        self.inner.publish();
        self.inner.wake.notify_all();
        ids
    }

    /// 执行界面操作；不合法的操作（例如移除运行中的任务）返回原因
    pub fn apply(&self, op: QueueOp) -> Result<(), String> {
        let inner = &self.inner;
        {
            let mut s = inner.lock();
            let now = inner.now();
            let lang = inner.lang(&s);
            let l = |zh: &str, en: &str| pick(lang, zh, en).to_string();
            let status = |s: &State, id: &str| {
                s.jobs
                    .iter()
                    .find(|j| j.id == id)
                    .map(|j| j.status)
                    .ok_or_else(|| tr!(lang, "没有这个任务：{}", "No such job: {}", id))
            };
            match op {
                QueueOp::Pause { id } => {
                    if status(&s, &id)? != JobStatus::Running {
                        return Err(l("只有进行中的任务可以暂停", "Only running jobs can be paused"));
                    }
                    if let Some(c) = s.controls.get(&id) {
                        if !c.suspend() {
                            return Err(l("无法暂停这个进程", "Could not pause this process"));
                        }
                    }
                    s.job_mut(&id).unwrap().status = JobStatus::Paused;
                    s.pause_since.insert(id.clone(), now);
                    inner.event(&mut s, &id, EventLevel::Info, l("已暂停", "Paused"));
                }
                QueueOp::Resume { id } => {
                    if status(&s, &id)? != JobStatus::Paused {
                        return Err(l("只有已暂停的任务可以继续", "Only paused jobs can be resumed"));
                    }
                    resume_job(&mut s, &id, now);
                    inner.event(&mut s, &id, EventLevel::Info, l("已继续", "Resumed"));
                }
                QueueOp::Cancel { id } => match status(&s, &id)? {
                    JobStatus::Queued => {
                        let j = s.job_mut(&id).unwrap();
                        j.status = JobStatus::Cancelled;
                        j.finished_at = Some(now);
                        inner.event(&mut s, &id, EventLevel::Info, l("已取消", "Cancelled"));
                    }
                    JobStatus::Running | JobStatus::Paused => {
                        s.cancel.insert(id.clone());
                        if let Some(c) = s.controls.get(&id) {
                            c.resume();
                            c.kill();
                        }
                    }
                    _ => return Err(l("任务已经结束", "The job has already finished")),
                },
                QueueOp::Retry { id } => {
                    let st = status(&s, &id)?;
                    if !st.finished() || st == JobStatus::Done {
                        return Err(l(
                            "只有失败、取消或跳过的任务可以重试",
                            "Only failed, cancelled or skipped jobs can be retried",
                        ));
                    }
                    s.tried.remove(&id);
                    let j = s.job_mut(&id).unwrap();
                    // 手动重试开始新的一轮：自动回退的次数重新计
                    j.attempts = 0;
                    j.status = JobStatus::Queued;
                    j.progress = JobProgress::default();
                    (j.report, j.finished_at, j.output_size) = (None, None, None);
                    inner.event(&mut s, &id, EventLevel::Info, l("重新加入队列", "Queued again"));
                    inner.preview(&mut s, &id);
                }
                QueueOp::Remove { id } => {
                    if status(&s, &id)?.active() {
                        return Err(l("进行中的任务要先取消", "Cancel the running job first"));
                    }
                    s.jobs.retain(|j| j.id != id);
                }
                QueueOp::Move { id, delta } => {
                    let i = s
                        .jobs
                        .iter()
                        .position(|j| j.id == id)
                        .ok_or_else(|| tr!(lang, "没有这个任务：{}", "No such job: {}", id))?;
                    let k = i as i64 + i64::from(delta.signum());
                    if (0..s.jobs.len() as i64).contains(&k) {
                        s.jobs.swap(i, k as usize);
                    }
                }
                QueueOp::SetPaused { paused } => {
                    s.paused = paused;
                    let targets: Vec<String> = if paused {
                        s.jobs.iter().filter(|j| j.status == JobStatus::Running).map(|j| j.id.clone()).collect()
                    } else {
                        s.held.iter().cloned().collect()
                    };
                    for id in targets {
                        if paused {
                            if s.controls.get(&id).is_none_or(|c| c.suspend()) {
                                s.job_mut(&id).unwrap().status = JobStatus::Paused;
                                s.pause_since.insert(id.clone(), now);
                                s.held.insert(id);
                            }
                        } else if status(&s, &id) == Ok(JobStatus::Paused) {
                            resume_job(&mut s, &id, now);
                        }
                    }
                    if !paused {
                        s.held.clear();
                    }
                }
                QueueOp::ClearFinished => {
                    s.jobs.retain(|j| !matches!(j.status, JobStatus::Done | JobStatus::Skipped | JobStatus::Cancelled));
                }
            }
        }
        inner.publish();
        inner.wake.notify_all();
        Ok(())
    }

    /// 应用退出：结束正在跑的 ffmpeg，不改任务状态（下次启动时按"没跑完"恢复）
    pub fn shutdown(&self) {
        {
            let mut s = self.inner.lock();
            s.shutdown = true;
            for c in s.controls.values() {
                c.resume();
                c.kill();
            }
        }
        self.inner.wake.notify_all();
        if let Some(h) = self.scheduler.lock().unwrap().take() {
            let _ = h.join();
        }
    }

    /// 等到没有排队或进行中的任务（测试与命令行用）；超时返回 false
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            {
                let s = self.inner.lock();
                if s.jobs.iter().all(|j| j.status.finished()) {
                    return true;
                }
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// 队列文件的默认位置
    pub fn store_path(app_dir: &Path) -> PathBuf {
        persist::queue_path(app_dir)
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn resume_job(s: &mut State, id: &str, now: u64) {
    if let Some(c) = s.controls.get(id) {
        c.resume();
    }
    if let Some(j) = s.job_mut(id) {
        j.status = JobStatus::Running;
    }
    s.end_pause(id, now);
    s.held.remove(id);
}

fn scheduler_loop(inner: Arc<Inner>) {
    loop {
        let started = {
            let mut s = inner.lock();
            if s.shutdown {
                return;
            }
            inner.pick(&mut s)
        };
        if !started.is_empty() {
            inner.publish();
            for id in started {
                let job_inner = inner.clone();
                thread::Builder::new()
                    .name(format!("vidforge-{id}"))
                    .spawn(move || worker::run(job_inner, id))
                    .expect("启动任务线程");
            }
        }
        let s = inner.lock();
        if s.shutdown {
            return;
        }
        let _ = inner.wake.wait_timeout(s, Duration::from_millis(250)).unwrap_or_else(|e| e.into_inner());
    }
}
