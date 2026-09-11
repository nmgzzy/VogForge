//! 执行一个任务：解析输出位置 → 生成命令 → 预检（硬件编码器）→ 一遍或两遍编码 → 改名 → 校验。
//! 失败时按 [`fallback::recover`] 决定降级重试、换编码器还是放弃；重试的任务回到排队状态，
//! 由调度器按票据重新开始（换成软编后占的就是 CPU 票）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::ffmpeg::classify::{classify, key_line};
use crate::ffmpeg::progress::{ProgressParser, SpeedTracker, overall_eta, overall_percent};
use crate::model::{
    Capabilities, EventLevel, FailureKind, JobProgress, JobProgressEvent, JobStatus, MediaInfo, ReportItem,
    TranscodePlan,
};
use crate::pipeline::args::{build_arg_segments, build_arg_segments_measured, build_first_pass, dry_run_args, flatten};
use crate::pipeline::loudness::{measure_all, parse_measure};
use crate::pipeline::text::{format_bytes, format_percent};
use crate::pipeline::update_plan;
use crate::verify::basic_report;

use super::fallback::{self, Recovery};
use super::files::{self, Target};
use super::{Environment, Inner, MAX_ATTEMPTS};

/// 预检最多等这么久
const DRY_RUN_TIMEOUT: Duration = Duration::from_secs(60);
/// 进度推送的最小间隔
const PROGRESS_INTERVAL_MS: u64 = 250;

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

struct Prepared {
    media: MediaInfo,
    plan: TranscodePlan,
    env: Environment,
    caps: Capabilities,
}

pub(super) fn run(inner: Arc<Inner>, id: String) {
    let Some(p) = prepare(&inner, &id) else { return };
    let attempts = inner.lock().jobs.iter().find(|j| j.id == id).map_or(0, |j| j.attempts);
    if attempts > MAX_ATTEMPTS {
        inner.finish(&id, JobStatus::Failed, EventLevel::Error, format!("已尝试 {MAX_ATTEMPTS} 次仍未成功，停止重试"));
        return;
    }
    execute(&inner, &id, p);
}

fn prepare(inner: &Inner, id: &str) -> Option<Prepared> {
    let mut s = inner.lock();
    let env = s.env.clone()?;
    let caps = env.effective(&s.disabled);
    let job = s.job_mut(id)?;
    let plan = update_plan(job.plan.clone(), &job.media, &caps);
    job.plan = plan.clone();
    Some(Prepared { media: job.media.clone(), plan, env, caps })
}

fn cancelled(inner: &Inner, id: &str) -> bool {
    let s = inner.lock();
    s.cancel.contains(id) || s.shutdown
}

/// 取消或退出时的收尾；退出时不改状态，下次启动按"没跑完"恢复
fn stop(inner: &Inner, id: &str, temp: &Path) {
    files::cleanup(temp);
    if inner.lock().shutdown {
        return;
    }
    inner.finish(id, JobStatus::Cancelled, EventLevel::Info, "已取消，临时文件已删除，目标目录无残留");
}

fn execute(inner: &Inner, id: &str, p: Prepared) {
    let Prepared { media, plan, env, caps } = p;
    let ffmpeg = PathBuf::from(&caps.ffmpeg_path);
    let conflict = env.settings.conflict;

    // ── 输出位置 ──
    // 源文件本身、别的任务正在写的目标都不能写：在队列状态里原子地选定并登记，避免两个同名任务写同一个临时文件
    let desired = crate::output::output_path(&media, &plan, &env.settings, &crate::output::today());
    let source = PathBuf::from(&media.path);
    let target = {
        let mut s = inner.lock();
        let busy = |p: &Path| {
            files::same_path(p, &source) || s.reserved.iter().any(|(k, r)| k != id && files::same_path(r, p))
        };
        match files::resolve_target(&desired, conflict, &busy) {
            Target::Write(p) => {
                s.reserved.insert(id.to_string(), p.clone());
                p
            }
            Target::Skip => {
                drop(s);
                let msg = format!("目标文件已存在，按设置跳过：{}", desired.display());
                return inner.finish(id, JobStatus::Skipped, EventLevel::Info, msg);
            }
        }
    };
    if let Some(dir) = target.parent().filter(|d| !d.as_os_str().is_empty()) {
        if let Err(e) = fs::create_dir_all(dir) {
            inner.finish(id, JobStatus::Failed, EventLevel::Error, format!("无法创建输出目录 {}：{e}", dir.display()));
            return;
        }
    }
    let temp = files::temp_path(&target);

    // ── 命令（后端按计划重新生成，不用界面传来的参数）──
    let args = flatten(&build_arg_segments(&media, &plan, &caps, &temp));
    let first = build_first_pass(&media, &plan, &caps, &temp).map(|s| flatten(&s));
    {
        let mut s = inner.lock();
        if let Some(j) = s.job_mut(id) {
            j.args = args.clone();
            j.first_pass = first.clone();
            j.output_path = target.to_string_lossy().to_string();
            j.encoder_used = plan.video.encoder;
        }
        if files::same_path(&desired, &source) {
            let msg = format!("输出路径与源文件相同，改名为 {}，源文件不会被覆盖", file_name(&target));
            inner.event(&mut s, id, EventLevel::Warn, msg);
        } else if target != desired {
            inner.event(&mut s, id, EventLevel::Info, format!("目标文件已存在，改名为 {}", file_name(&target)));
        }
    }
    inner.publish();

    // ── 预检：硬件编码器用这组参数编 3 帧，比跑了很久才失败好 ──
    if plan.video.encoder.is_hardware() {
        if let Some(dry) = dry_run_args(&media, &plan) {
            // 预检也是可以暂停、取消的进程，卡住时由看门狗结束
            let out = run_quiet(inner, id, &ffmpeg, &dry, Some(DRY_RUN_TIMEOUT));
            if cancelled(inner, id) {
                return stop(inner, id, &temp);
            }
            match out {
                Ok(o) if o.success => {
                    let mut s = inner.lock();
                    let msg = format!("预检通过：{} 以当前参数试编码 3 帧成功", plan.video.encoder.name());
                    inner.event(&mut s, id, EventLevel::Info, msg);
                }
                Ok(o) => {
                    let text = o.stderr.join("\n");
                    let key = match key_line(&text) {
                        k if k.is_empty() => "预检没有通过（超时或没有输出）".to_string(),
                        k => k,
                    };
                    return failed(inner, id, classify(&text), &key, &plan, &media, &caps, 0.0, &temp);
                }
                Err(e) => {
                    let msg = format!("无法启动 ffmpeg（{}）：{e}", ffmpeg.display());
                    return inner.finish(id, JobStatus::Failed, EventLevel::Error, msg);
                }
            }
        }
    }

    // ── 响度测量：两遍 loudnorm 的第一遍，每条要标准化的音轨一次 ──
    let mut args = args;
    let measures = measure_all(&media, &plan);
    if !measures.is_empty() {
        {
            let mut s = inner.lock();
            inner.event(&mut s, id, EventLevel::Info, format!("测量 {} 条音轨的响度", measures.len()));
        }
        inner.publish();
        let mut found = Vec::new();
        for (track, cmd) in measures {
            let exit = match run_quiet(inner, id, &ffmpeg, &cmd, None) {
                Ok(exit) => exit,
                Err(e) => {
                    let msg = format!("无法启动 ffmpeg（{}）：{e}", ffmpeg.display());
                    return inner.finish(id, JobStatus::Failed, EventLevel::Error, msg);
                }
            };
            if cancelled(inner, id) {
                return stop(inner, id, &temp);
            }
            match exit.success.then(|| parse_measure(&exit.stderr.join("\n"))).flatten() {
                Some(m) => found.push((track, m)),
                None => {
                    let mut s = inner.lock();
                    let msg = format!("第 {} 条音轨的响度测量没有结果，这条音轨按单遍标准化", track + 1);
                    inner.event(&mut s, id, EventLevel::Warn, msg);
                }
            }
        }
        args = flatten(&build_arg_segments_measured(&media, &plan, &caps, &temp, &found));
        let mut s = inner.lock();
        if let Some(j) = s.job_mut(id) {
            j.args = args.clone();
        }
    }

    // ── 编码 ──
    {
        let mut s = inner.lock();
        inner.event(&mut s, id, EventLevel::Info, if first.is_some() { "开始两遍编码" } else { "开始转码" });
    }
    inner.publish();
    let started = inner.now();
    let passes: Vec<(Option<u8>, Vec<String>)> = match first {
        Some(f) => vec![(Some(1), f), (Some(2), args)],
        None => vec![(None, args)],
    };
    for (pass, cmd) in passes {
        let exit = match run_pass(inner, id, &ffmpeg, &cmd, pass, media.duration_sec) {
            Ok(exit) => exit,
            Err(e) => {
                files::cleanup(&temp);
                let msg = format!("无法启动 ffmpeg（{}）：{e}", ffmpeg.display());
                return inner.finish(id, JobStatus::Failed, EventLevel::Error, msg);
            }
        };
        {
            let mut s = inner.lock();
            if let Some(j) = s.job_mut(id) {
                j.log = exit.stderr.clone();
            }
        }
        if cancelled(inner, id) {
            return stop(inner, id, &temp);
        }
        if !exit.success {
            let text = exit.stderr.join("\n");
            let elapsed = {
                let s = inner.lock();
                inner.now().saturating_sub(started).saturating_sub(s.paused_total(id, inner.now())) as f64 / 1000.0
            };
            let kind = if text.trim().is_empty() { FailureKind::Unknown } else { classify(&text) };
            let key = match key_line(&text) {
                k if k.is_empty() => format!("退出码 {}", exit.code.map_or("未知".into(), |c| c.to_string())),
                k => k,
            };
            return failed(inner, id, kind, &key, &plan, &media, &caps, elapsed, &temp);
        }
    }

    // ── 改名为最终文件 ──
    for f in files::passlog_files(&temp) {
        let _ = fs::remove_file(f);
    }
    let busy = |p: &Path| {
        let s = inner.lock();
        files::same_path(p, &source) || s.reserved.iter().any(|(k, r)| k != id && files::same_path(r, p))
    };
    let final_path = match files::finalize(&temp, &target, conflict, &busy) {
        Ok(Some(p)) => p,
        Ok(None) => {
            let msg = "转码期间目标位置出现了同名文件，按设置跳过，临时文件已删除";
            return inner.finish(id, JobStatus::Skipped, EventLevel::Info, msg);
        }
        Err(e) => {
            files::cleanup(&temp);
            return inner.finish(id, JobStatus::Failed, EventLevel::Error, format!("无法改名为最终文件：{e}"));
        }
    };
    let size = fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);

    // ── 校验 ──
    let report = match inner.deps.tools.probe(Path::new(&caps.ffprobe_path), &final_path) {
        Ok(out) => basic_report(&media, &plan, &out),
        Err(e) => {
            vec![ReportItem {
                label: "读取输出".into(), expected: "能被 ffprobe 分析".into(), actual: e, ok: false
            }]
        }
    };
    let bad: Vec<&str> = report.iter().filter(|r| !r.ok).map(|r| r.label.as_str()).collect();
    let sizes = format!(
        "{} → {}（{}）",
        format_bytes(media.size_bytes),
        format_bytes(size),
        format_percent(size as f64 / media.size_bytes.max(1) as f64)
    );
    let (level, msg) = if bad.is_empty() {
        (EventLevel::Info, format!("完成，校验通过：{sizes}"))
    } else {
        (EventLevel::Warn, format!("已完成（{sizes}），但 {} 项与预期不符：{}", bad.len(), bad.join("、")))
    };
    {
        let mut s = inner.lock();
        if let Some(j) = s.job_mut(id) {
            j.output_path = final_path.to_string_lossy().to_string();
            j.output_size = Some(size);
            j.report = Some(report);
            j.progress.percent = 100.0;
            j.progress.size_bytes = size;
        }
    }
    inner.finish(id, JobStatus::Done, level, msg);
}

/// 跑一遍 ffmpeg，边读边更新进度
fn run_pass(
    inner: &Inner,
    id: &str,
    ffmpeg: &Path,
    cmd: &[String],
    pass: Option<u8>,
    duration: f64,
) -> std::io::Result<super::process::ProcessExit> {
    let mut proc = inner.deps.tools.spawn(ffmpeg, &cmd[1..])?;
    let pass_start = inner.now();
    let paused_before = {
        let mut s = inner.lock();
        let control = proc.control();
        // 进程启动前就已取消或暂停（例如两遍之间）：立即执行
        if s.cancel.contains(id) || s.shutdown {
            control.kill();
        } else if s.jobs.iter().any(|j| j.id == id && j.status == JobStatus::Paused) {
            control.suspend();
        }
        s.controls.insert(id.to_string(), control);
        s.paused_total(id, pass_start)
    };

    let mut parser = ProgressParser::default();
    let mut tracker = SpeedTracker::default();
    let mut last_emit = 0u64;
    let mut progress = JobProgress { pass, ..Default::default() };
    while let Some(line) = proc.next_line() {
        let Some(block) = parser.feed(&line) else { continue };
        let now = inner.now();
        let active = {
            let s = inner.lock();
            now.saturating_sub(pass_start).saturating_sub(s.paused_total(id, now).saturating_sub(paused_before))
        } as f64
            / 1000.0;
        let out = block.out_time_sec().unwrap_or(progress.out_time_sec);
        let (speed, eta) = tracker.update(out, block.speed, active, duration);
        progress = JobProgress {
            percent: overall_percent(pass, if duration > 0.0 { out / duration } else { 0.0 }),
            out_time_sec: out,
            speed,
            fps: block.fps.unwrap_or(progress.fps),
            size_bytes: block.total_size.unwrap_or(progress.size_bytes),
            eta_sec: overall_eta(pass, eta, speed, duration),
            dup_frames: block.dup_frames.unwrap_or(progress.dup_frames),
            drop_frames: block.drop_frames.unwrap_or(progress.drop_frames),
            pass,
        };
        {
            let mut s = inner.lock();
            if let Some(j) = s.job_mut(id) {
                j.progress = progress.clone();
            }
        }
        if block.end || now.saturating_sub(last_emit) >= PROGRESS_INTERVAL_MS {
            last_emit = now;
            inner.deps.sink.progress(&JobProgressEvent { id: id.to_string(), progress: progress.clone() });
        }
    }
    let exit = proc.wait();
    inner.lock().controls.remove(id);
    exit
}

/// 跑一条没有进度输出的命令（预检、响度测量），可以被取消与暂停；给了 `timeout` 时超时由看门狗结束
fn run_quiet(
    inner: &Inner,
    id: &str,
    ffmpeg: &Path,
    cmd: &[String],
    timeout: Option<Duration>,
) -> std::io::Result<super::process::ProcessExit> {
    let mut proc = inner.deps.tools.spawn(ffmpeg, &cmd[1..])?;
    let done = Arc::new(AtomicBool::new(false));
    if let Some(limit) = timeout {
        let (control, done) = (proc.control(), done.clone());
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + limit;
            while std::time::Instant::now() < deadline {
                if done.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            control.kill();
        });
    }
    {
        let mut s = inner.lock();
        let control = proc.control();
        if s.cancel.contains(id) || s.shutdown {
            control.kill();
        } else if s.jobs.iter().any(|j| j.id == id && j.status == JobStatus::Paused) {
            control.suspend();
        }
        s.controls.insert(id.to_string(), control);
    }
    while proc.next_line().is_some() {}
    let exit = proc.wait();
    done.store(true, Ordering::SeqCst);
    inner.lock().controls.remove(id);
    exit
}

/// 失败：删掉部分输出，按分类决定重试（回到排队）还是放弃
#[allow(clippy::too_many_arguments)]
fn failed(
    inner: &Inner,
    id: &str,
    kind: FailureKind,
    key: &str,
    plan: &TranscodePlan,
    media: &MediaInfo,
    caps: &Capabilities,
    elapsed: f64,
    temp: &Path,
) {
    files::cleanup(temp);
    let mut tried = inner.lock().tried.remove(id).unwrap_or_default();
    let recovery = fallback::recover(kind, key, plan, media, caps, &mut tried, elapsed);
    match recovery {
        Recovery::Retry { plan, message, disable_vendor, delay_ms, serialize_gpu } => {
            {
                let mut s = inner.lock();
                let now = inner.now();
                if let Some(v) = disable_vendor.filter(|v| !s.disabled.contains(v)) {
                    s.disabled.push(v);
                }
                s.serial_gpu |= serialize_gpu;
                s.tried.insert(id.to_string(), tried);
                if delay_ms > 0 {
                    s.not_before.insert(id.to_string(), now + delay_ms);
                }
                inner.event(&mut s, id, EventLevel::Warn, message);
                if let Some(j) = s.job_mut(id) {
                    j.encoder_used = plan.video.encoder;
                    j.plan = plan;
                    j.status = JobStatus::Queued;
                    j.progress = JobProgress::default();
                }
                s.controls.remove(id);
                s.reserved.remove(id);
                s.pause_since.remove(id);
                s.paused_ms.remove(id);
            }
            inner.publish();
            inner.wake.notify_all();
        }
        Recovery::GiveUp { message } => inner.finish(id, JobStatus::Failed, EventLevel::Error, message),
    }
}
