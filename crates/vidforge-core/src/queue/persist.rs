//! 队列持久化（设计文档 4.7，需求 F-6.9）：状态写 `~/.vidforge/queue.json`，重启后恢复。
//!
//! 恢复是重新开始而不是续传（ffmpeg 不支持断点续传）：上次没跑完的任务回到排队状态，
//! 残留的临时文件与统计文件先删掉。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::write_json_atomic;
use crate::i18n::{Lang, pick};
use crate::model::{EventLevel, Job, JobEvent, JobProgress, JobStatus, QueueSnapshot};

use super::files;

/// 文件结构版本；不兼容的旧文件改名为 `.bak` 后从空队列开始
pub const QUEUE_SCHEMA: u32 = 1;

pub fn queue_path(app_dir: &Path) -> PathBuf {
    app_dir.join("queue.json")
}

#[derive(Serialize, Deserialize)]
struct Stored {
    schema: u32,
    paused: bool,
    jobs: Vec<Job>,
}

pub fn save(path: &Path, snapshot: &QueueSnapshot) -> io::Result<()> {
    write_json_atomic(path, &Stored { schema: QUEUE_SCHEMA, paused: snapshot.paused, jobs: snapshot.jobs.clone() })
}

/// 读取保存的队列；文件不存在时为空队列，读不懂时备份后为空队列
pub fn load(path: &Path) -> QueueSnapshot {
    let empty = QueueSnapshot { jobs: Vec::new(), paused: false };
    let Ok(text) = fs::read_to_string(path) else { return empty };
    match serde_json::from_str::<Stored>(&text) {
        Ok(s) if s.schema == QUEUE_SCHEMA => QueueSnapshot { jobs: s.jobs, paused: s.paused },
        _ => {
            let _ = fs::rename(path, path.with_extension("json.bak"));
            empty
        }
    }
}

/// 上次退出时没跑完的任务回到排队状态，删掉它们留下的临时文件；返回恢复的任务数
pub fn recover(jobs: &mut [Job], now_ms: u64, lang: Lang) -> usize {
    let mut n = 0;
    for job in jobs.iter_mut().filter(|j| j.status.active()) {
        if !job.output_path.is_empty() {
            files::cleanup(&files::temp_path(Path::new(&job.output_path)));
        }
        job.status = JobStatus::Queued;
        job.progress = JobProgress::default();
        job.events.push(JobEvent {
            at: now_ms,
            level: EventLevel::Warn,
            message: pick(
                lang,
                "上次退出时这个任务没有完成，已删除临时文件并重新排队，将从头开始",
                "This job was unfinished when the app last exited; its temporary file was deleted and it will start over",
            )
            .into(),
            detail: None,
        });
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Capabilities, EnvStatus, MediaInfo, Scenario};
    use crate::pipeline::recommend_plan;

    fn job(id: &str, status: JobStatus, output: &Path) -> Job {
        let media: MediaInfo = serde_json::from_str(
            r#"{"id":"m","path":"/a/b.mp4","name":"b.mp4","container":"mov","durationSec":10,"sizeBytes":1000,
            "bitrate":8000000,"video":[{"index":0,"codec":"h264","width":1920,"height":1080,"fpsAvg":30,"fpsNominal":30,
            "isVfr":false,"bitDepth":8,"pixFmt":"yuv420p","color":{"primaries":"bt709","transfer":"bt709",
            "space":"bt709","range":"tv","hdrKind":"none"},"hdr10plus":false,"rotation":0}],"audio":[],"subtitle":[],
            "chapters":0,"attachments":0,"sourceHint":"unknown"}"#,
        )
        .unwrap();
        let plan = recommend_plan(&media, Scenario::Archive, &Capabilities::placeholder(EnvStatus::Probing, ""));
        Job {
            id: id.into(),
            encoder_used: plan.video.encoder,
            media,
            plan,
            args: vec!["ffmpeg".into()],
            first_pass: None,
            output_path: output.to_string_lossy().to_string(),
            status,
            progress: JobProgress { percent: 40.0, ..Default::default() },
            events: Vec::new(),
            log: Vec::new(),
            report: None,
            started_at: Some(1),
            finished_at: None,
            output_size: None,
            attempts: 1,
        }
    }

    #[test]
    fn round_trip_keeps_every_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = queue_path(dir.path());
        assert_eq!(load(&path).jobs.len(), 0, "没有文件时是空队列");
        let snap = QueueSnapshot { jobs: vec![job("a", JobStatus::Done, &dir.path().join("a.mkv"))], paused: true };
        save(&path, &snap).unwrap();
        assert_eq!(load(&path), snap);
    }

    #[test]
    fn unreadable_files_are_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = queue_path(dir.path());
        fs::write(&path, "{ 坏掉的 json").unwrap();
        assert!(load(&path).jobs.is_empty());
        assert!(path.with_extension("json.bak").exists());
    }

    #[test]
    fn unfinished_jobs_restart_and_leftovers_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("a.mkv");
        let part = files::temp_path(&out);
        fs::write(&part, b"half").unwrap();
        fs::write(dir.path().join("a.mkv.vidforge-part.2pass-0.log"), b"x").unwrap();
        let mut jobs = vec![
            job("run", JobStatus::Running, &out),
            job("pause", JobStatus::Paused, &dir.path().join("b.mkv")),
            job("done", JobStatus::Done, &dir.path().join("c.mkv")),
        ];
        assert_eq!(recover(&mut jobs, 99, Lang::ZhCn), 2);
        assert_eq!(
            jobs.iter().map(|j| j.status).collect::<Vec<_>>(),
            [JobStatus::Queued, JobStatus::Queued, JobStatus::Done]
        );
        assert_eq!(jobs[0].progress.percent, 0.0);
        assert_eq!(jobs[0].events[0].level, EventLevel::Warn);
        assert!(!part.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0, "统计文件也要删掉");
    }
}
