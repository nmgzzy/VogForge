//! 转码队列（设计文档 4.7）：任务、进度、事件与界面快照。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::encoder::EncoderId;
use super::media::MediaInfo;
use super::plan::TranscodePlan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum JobStatus {
    Queued,
    Running,
    Paused,
    Done,
    /// 目标文件已存在且冲突策略是跳过
    Skipped,
    Failed,
    Cancelled,
}

impl JobStatus {
    /// 已经结束（不会再自己变化）
    pub fn finished(self) -> bool {
        matches!(self, JobStatus::Done | JobStatus::Skipped | JobStatus::Failed | JobStatus::Cancelled)
    }

    /// 正占着并发票（暂停只是挂起进程，内存与 GPU 会话仍被占用）
    pub fn active(self) -> bool {
        matches!(self, JobStatus::Running | JobStatus::Paused)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct JobProgress {
    /// 0–100；两遍编码按两遍合并
    pub percent: f64,
    /// 当前这一遍已处理到的时间点
    pub out_time_sec: f64,
    /// 实时倍速（平滑后）
    pub speed: f64,
    pub fps: f64,
    pub size_bytes: u64,
    /// 剩余时间；开头几秒速度不可信，不给
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_sec: Option<f64>,
    pub dup_frames: u64,
    pub drop_frames: u64,
    /// 两遍编码时正在跑第几遍
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pass: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EventLevel {
    Info,
    Warn,
    Error,
}

/// 任务时间线上的一条记录：预检、回退、完成、失败原因……
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, optional_fields)]
pub struct JobEvent {
    /// Unix 毫秒
    pub at: u64,
    pub level: EventLevel,
    pub message: String,
    /// ffmpeg 原文，界面上可展开查看（需求 F-9.4：不直接抛原始报错）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// 输出校验的一项（设计文档 4.8）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReportItem {
    pub label: String,
    pub expected: String,
    pub actual: String,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct Job {
    pub id: String,
    pub media: MediaInfo,
    /// 实际执行的计划（回退后会换编码器）
    pub plan: TranscodePlan,
    /// 实际执行的命令（两遍编码时是第二遍），写入临时文件
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_pass: Option<Vec<String>>,
    /// 最终文件路径
    pub output_path: String,
    /// 命名模板里 `{date}` 用的日期，来自加入队列时的界面
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub status: JobStatus,
    pub progress: JobProgress,
    pub encoder_used: EncoderId,
    pub events: Vec<JobEvent>,
    /// ffmpeg 输出的最后若干行
    pub log: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<Vec<ReportItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_size: Option<u64>,
    /// 已经执行过的次数（含回退重跑）
    pub attempts: u32,
}

/// 加入队列的一项：界面上的素材与计划。命令由后端按计划重新生成，不信任前端的参数
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, optional_fields)]
pub struct QueueItem {
    pub media: MediaInfo,
    pub plan: TranscodePlan,
    /// 命名模板里 `{date}` 用的本地日期（YYYY-MM-DD），界面加入队列时给出，与预览一致。没有时用当天（UTC）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

/// 队列的完整状态，结构变化（增删、换序、状态改变）时整体推送
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueSnapshot {
    pub jobs: Vec<Job>,
    /// 全部暂停：进行中的任务挂起，排队的不开始
    pub paused: bool,
}

/// 运行中的进度推送，只带变化的部分
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct JobProgressEvent {
    pub id: String,
    pub progress: JobProgress,
}

/// 界面对队列的操作
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum QueueOp {
    Pause { id: String },
    Resume { id: String },
    Cancel { id: String },
    Retry { id: String },
    Remove { id: String },
    Move { id: String, delta: i32 },
    SetPaused { paused: bool },
    ClearFinished,
}
