//! 执行一个任务：解析输出位置 → 生成命令 → 预检（硬件编码器）→ 一遍或两遍编码 → 改名 → 校验。
//! 失败时按 [`fallback::recover`] 决定降级重试、换编码器还是放弃；重试的任务回到排队状态，
//! 由调度器按票据重新开始（换成软编后占的就是 CPU 票）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::ffmpeg::classify::{classify, key_line};
use crate::ffmpeg::errors::explain;
use crate::ffmpeg::probe::{count_frames_args, parse_frame_count};
use crate::ffmpeg::progress::{ProgressParser, SpeedTracker, overall_eta, overall_percent};
use crate::i18n::{Lang, pick};
use crate::model::{
    Capabilities, EventLevel, FailureKind, JobProgress, JobProgressEvent, JobStatus, MediaInfo, ReportItem,
    TranscodePlan,
};
use crate::pipeline::args::{build_arg_segments, build_arg_segments_measured, build_first_pass, dry_run_args, flatten};
use crate::pipeline::loudness::{measure_all, parse_measure};
use crate::pipeline::text::{format_bytes, format_percent};
use crate::pipeline::update_plan;
use crate::tr;
use crate::verify;

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
        let lang = p.env.settings.language;
        let msg =
            tr!(lang, "已尝试 {} 次仍未成功，停止重试", "Still failing after {} attempts; giving up", MAX_ATTEMPTS);
        inner.finish(&id, JobStatus::Failed, EventLevel::Error, msg);
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
fn stop(inner: &Inner, id: &str, temp: &Path, lang: Lang) {
    files::cleanup(temp);
    if inner.lock().shutdown {
        return;
    }
    let msg = pick(
        lang,
        "已取消，临时文件已删除，目标目录无残留",
        "Cancelled; the temporary file was deleted and nothing was left in the output folder",
    );
    inner.finish(id, JobStatus::Cancelled, EventLevel::Info, msg);
}

fn execute(inner: &Inner, id: &str, p: Prepared) {
    let Prepared { media, plan, env, caps } = p;
    let ffmpeg = PathBuf::from(&caps.ffmpeg_path);
    let conflict = env.settings.conflict;
    let lang = env.settings.language;
    let cannot_start = |e: std::io::Error| {
        tr!(lang, "无法启动 ffmpeg（{}）：{}", "Could not start ffmpeg ({}): {}", ffmpeg.display(), e)
    };

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
                let msg = tr!(
                    lang,
                    "目标文件已存在，按设置跳过：{}",
                    "The target already exists; skipped as configured: {}",
                    desired.display()
                );
                return inner.finish(id, JobStatus::Skipped, EventLevel::Info, msg);
            }
        }
    };
    if let Some(dir) = target.parent().filter(|d| !d.as_os_str().is_empty()) {
        if let Err(e) = fs::create_dir_all(dir) {
            let msg =
                tr!(lang, "无法创建输出目录 {}：{}", "Could not create the output folder {}: {}", dir.display(), e);
            inner.finish(id, JobStatus::Failed, EventLevel::Error, msg);
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
            let msg = tr!(
                lang,
                "输出路径与源文件相同，改名为 {}，源文件不会被覆盖",
                "The output path equals the source file; writing {} instead so the source is never overwritten",
                file_name(&target)
            );
            inner.event(&mut s, id, EventLevel::Warn, msg);
        } else if target != desired {
            let msg = tr!(
                lang,
                "目标文件已存在，改名为 {}",
                "The target already exists; writing {} instead",
                file_name(&target)
            );
            inner.event(&mut s, id, EventLevel::Info, msg);
        }
    }
    inner.publish();

    // ── 预检：硬件编码器用这组参数编 3 帧，比跑了很久才失败好 ──
    if plan.video.encoder.is_hardware() {
        if let Some(dry) = dry_run_args(&media, &plan) {
            // 预检也是可以暂停、取消的进程，卡住时由看门狗结束
            let out = run_quiet(inner, id, &ffmpeg, &dry, Some(DRY_RUN_TIMEOUT));
            if cancelled(inner, id) {
                return stop(inner, id, &temp, lang);
            }
            match out {
                Ok(o) if o.success => {
                    let mut s = inner.lock();
                    let msg = tr!(
                        lang,
                        "预检通过：{} 以当前参数试编码 3 帧成功",
                        "Pre-check passed: {} encoded 3 test frames with these settings",
                        plan.video.encoder.name()
                    );
                    inner.event(&mut s, id, EventLevel::Info, msg);
                }
                Ok(o) => {
                    let text = o.stderr.join("\n");
                    let key = match key_line(&text) {
                        k if k.is_empty() => {
                            pick(lang, "预检没有通过（超时或没有输出）", "pre-check failed (timed out or no output)")
                                .into()
                        }
                        k => k,
                    };
                    return failed(inner, id, classify(&text), &text, &key, &plan, &media, &caps, 0.0, &temp, lang);
                }
                Err(e) => return inner.finish(id, JobStatus::Failed, EventLevel::Error, cannot_start(e)),
            }
        }
    }

    // ── 响度测量：两遍 loudnorm 的第一遍，每条要标准化的音轨一次 ──
    let mut args = args;
    let measures = measure_all(&media, &plan);
    if !measures.is_empty() {
        {
            let mut s = inner.lock();
            let msg = tr!(lang, "测量 {} 条音轨的响度", "Measuring loudness of {} audio track(s)", measures.len());
            inner.event(&mut s, id, EventLevel::Info, msg);
        }
        inner.publish();
        let mut found = Vec::new();
        for (track, cmd) in measures {
            let exit = match run_quiet(inner, id, &ffmpeg, &cmd, None) {
                Ok(exit) => exit,
                Err(e) => return inner.finish(id, JobStatus::Failed, EventLevel::Error, cannot_start(e)),
            };
            if cancelled(inner, id) {
                return stop(inner, id, &temp, lang);
            }
            match exit.success.then(|| parse_measure(&exit.stderr.join("\n"))).flatten() {
                Some(m) => found.push((track, m)),
                None => {
                    let mut s = inner.lock();
                    let msg = tr!(
                        lang,
                        "第 {} 条音轨的响度测量没有结果，这条音轨按单遍标准化",
                        "Loudness measurement of track {} returned nothing; that track uses single-pass normalization",
                        track + 1
                    );
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
        let msg = if first.is_some() {
            pick(lang, "开始两遍编码", "Two-pass encoding started")
        } else {
            pick(lang, "开始转码", "Transcoding started")
        };
        inner.event(&mut s, id, EventLevel::Info, msg);
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
                return inner.finish(id, JobStatus::Failed, EventLevel::Error, cannot_start(e));
            }
        };
        {
            let mut s = inner.lock();
            if let Some(j) = s.job_mut(id) {
                j.log = exit.stderr.clone();
            }
        }
        if cancelled(inner, id) {
            return stop(inner, id, &temp, lang);
        }
        if !exit.success {
            let text = exit.stderr.join("\n");
            let elapsed = {
                let s = inner.lock();
                inner.now().saturating_sub(started).saturating_sub(s.paused_total(id, inner.now())) as f64 / 1000.0
            };
            let kind = if text.trim().is_empty() { FailureKind::Unknown } else { classify(&text) };
            let key = match key_line(&text) {
                k if k.is_empty() => {
                    let code = exit.code.map_or("?".into(), |c| c.to_string());
                    tr!(lang, "退出码 {}", "exit code {}", code)
                }
                k => k,
            };
            return failed(inner, id, kind, &text, &key, &plan, &media, &caps, elapsed, &temp, lang);
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
            let msg = pick(
                lang,
                "转码期间目标位置出现了同名文件，按设置跳过，临时文件已删除",
                "A file with the same name appeared during encoding; skipped as configured and the temporary file was deleted",
            );
            return inner.finish(id, JobStatus::Skipped, EventLevel::Info, msg);
        }
        Err(e) => {
            files::cleanup(&temp);
            let msg = tr!(lang, "无法改名为最终文件：{}", "Could not rename to the final file: {}", e);
            return inner.finish(id, JobStatus::Failed, EventLevel::Error, msg);
        }
    };
    let size = fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);

    // ── 校验 ──
    let ffprobe = PathBuf::from(&caps.ffprobe_path);
    let mut probe_raw = None;
    let report = match inner.deps.tools.probe(&ffprobe, &final_path) {
        Ok(mut out) => {
            // ffmpeg 写的 MKV 没有 nb_frames：数一遍包。要读完整个文件，所以用可暂停、可取消的进程
            if let Some(v) = out.video.first_mut().filter(|v| v.frame_count.is_none()) {
                let mut cmd = vec!["ffprobe".to_string()];
                cmd.extend(count_frames_args(&final_path, v.index));
                v.frame_count = run_capture(inner, id, &ffprobe, &cmd, None)
                    .ok()
                    .filter(|(exit, _)| exit.success)
                    .and_then(|(_, stdout)| parse_frame_count(&stdout));
            }
            verify::report(&media, &plan, &out, lang)
        }
        Err(e) => {
            let (reason, raw) = e.describe(lang);
            probe_raw = raw;
            vec![ReportItem {
                label: pick(lang, "读取输出", "Read output").into(),
                expected: pick(lang, "能被 ffprobe 分析", "readable by ffprobe").into(),
                actual: reason,
                ok: false,
            }]
        }
    };
    // 核对期间点了取消：输出已经写完、改好名，留着它，但任务不算完成，也不触发完成后动作
    if cancelled(inner, id) {
        if inner.lock().shutdown {
            return;
        }
        let msg = tr!(
            lang,
            "已取消：输出 {} 已经写完，但没有核对",
            "Cancelled: the output {} was written but not verified",
            file_name(&final_path)
        );
        return inner.finish(id, JobStatus::Cancelled, EventLevel::Warn, msg);
    }
    let bad: Vec<&str> = report.iter().filter(|r| !r.ok).map(|r| r.label.as_str()).collect();
    let ratio = format_percent(size as f64 / media.size_bytes.max(1) as f64);
    let sizes = tr!(lang, "{} → {}（{}）", "{} → {} ({})", format_bytes(media.size_bytes), format_bytes(size), ratio);
    let (level, msg) = if bad.is_empty() {
        (EventLevel::Info, tr!(lang, "完成，校验通过：{}", "Done, all checks passed: {}", sizes))
    } else {
        let list = bad.join(pick(lang, "、", ", "));
        let msg = tr!(
            lang,
            "已完成（{}），但 {} 项与预期不符：{}",
            "Done ({}), but {} item(s) differ from what was expected: {}",
            sizes,
            bad.len(),
            list
        );
        (EventLevel::Warn, msg)
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
    inner.finish_detail(id, JobStatus::Done, level, msg, probe_raw);
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
    run_capture(inner, id, ffmpeg, cmd, timeout).map(|(exit, _)| exit)
}

/// 同 [`run_quiet`]，另外留下标准输出的前几行（数帧只输出一行；进度输出不保留，免得长任务占内存）
fn run_capture(
    inner: &Inner,
    id: &str,
    program: &Path,
    cmd: &[String],
    timeout: Option<Duration>,
) -> std::io::Result<(super::process::ProcessExit, String)> {
    const KEEP: usize = 8;
    let mut proc = inner.deps.tools.spawn(program, &cmd[1..])?;
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
    let mut stdout = Vec::new();
    while let Some(line) = proc.next_line() {
        if stdout.len() < KEEP {
            stdout.push(line);
        }
    }
    let exit = proc.wait();
    done.store(true, Ordering::SeqCst);
    inner.lock().controls.remove(id);
    exit.map(|e| (e, stdout.join("\n")))
}

/// 失败：删掉部分输出，按分类决定重试（回到排队）还是放弃。`stderr` 用来向用户解释原因，
/// `key` 是最能说明问题的那一行原文，放进事件的 detail 供展开查看
#[allow(clippy::too_many_arguments)]
fn failed(
    inner: &Inner,
    id: &str,
    kind: FailureKind,
    stderr: &str,
    key: &str,
    plan: &TranscodePlan,
    media: &MediaInfo,
    caps: &Capabilities,
    elapsed: f64,
    temp: &Path,
    lang: Lang,
) {
    files::cleanup(temp);
    // 软件编码器与原样封装失败不再回退：把原因说清楚
    if plan.video.action == crate::model::StreamAction::Copy || !plan.video.encoder.is_hardware() {
        let explained = explain(stderr, lang);
        let msg = tr!(lang, "{} 失败：{}", "{} failed: {}", plan.video.encoder.name(), explained.sentence(lang));
        return inner.finish_detail(id, JobStatus::Failed, EventLevel::Error, msg, Some(key.to_string()));
    }
    let mut tried = inner.lock().tried.remove(id).unwrap_or_default();
    let recovery = fallback::recover(kind, plan, media, caps, &mut tried, elapsed, lang);
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
                inner.event_detail(&mut s, id, EventLevel::Warn, message, Some(key.to_string()));
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
        Recovery::GiveUp { message } => {
            inner.finish_detail(id, JobStatus::Failed, EventLevel::Error, message, Some(key.to_string()))
        }
    }
}
