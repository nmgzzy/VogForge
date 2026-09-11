//! 暴露给前端的命令。耗时操作放到阻塞线程池，进度通过事件推送。

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Emitter, Manager, State};
use vidforge_core::config::{self, Settings};
use vidforge_core::ffmpeg::capability::{ProbeContext, probe};
use vidforge_core::ffmpeg::exec::SystemRunner;
use vidforge_core::ffmpeg::locate::SystemEnv;
use vidforge_core::import::import_paths;
use vidforge_core::model::{Capabilities, EnvStatus, ImportResult, Platform, QueueItem, QueueOp, QueueSnapshot};

use crate::state::AppState;

pub const EVENT_PROBE_PROGRESS: &str = "probe://progress";
pub const EVENT_IMPORT_PROGRESS: &str = "import://progress";
/// 同时分析的文件数。ffprobe 主要耗在读盘，并发太高反而拖慢机械硬盘与网络盘
const IMPORT_WORKERS: usize = 4;

type CmdResult<T> = Result<T, String>;

/// 在阻塞线程里调用：探测环境并更新状态。`force` 为 false 时优先用缓存
fn probe_blocking(app: &AppHandle, force: bool) -> CmdResult<Capabilities> {
    let state = app.state::<AppState>();
    let _guard = state.probe_lock.lock().map_err(|e| e.to_string())?;
    let user_path = state.settings.lock().map_err(|e| e.to_string())?.ffmpeg_path.clone().map(PathBuf::from);
    let ctx = ProbeContext {
        env: &SystemEnv,
        runner: &SystemRunner,
        platform: Platform::current(),
        app_dir: state.app_dir.clone(),
        user_path,
    };
    let emitter = app.clone();
    let caps = probe(&ctx, force, &move |p| {
        let _ = emitter.emit(EVENT_PROBE_PROGRESS, p);
    });
    *state.caps.lock().map_err(|e| e.to_string())? = Some(caps.clone());
    state.sync_queue();
    Ok(caps)
}

/// 当前能力快照；还没探测过就先探测一次（通常命中缓存）
fn current_caps(app: &AppHandle) -> CmdResult<Capabilities> {
    let cached = app.state::<AppState>().caps.lock().map_err(|e| e.to_string())?.clone();
    match cached {
        Some(c) => Ok(c),
        None => probe_blocking(app, false),
    }
}

/// 探测环境能力。`force` 为 false 时优先用缓存。
#[tauri::command]
pub async fn get_capabilities(app: AppHandle, force: bool) -> CmdResult<Capabilities> {
    tauri::async_runtime::spawn_blocking(move || probe_blocking(&app, force)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    Ok(state.settings.lock().map_err(|e| e.to_string())?.clone())
}

/// 保存设置并返回规整后的值。ffmpeg 路径变了由前端随后调用 `get_capabilities(true)`。
#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<Settings> {
    let settings = settings.sanitized();
    config::save_settings(&state.app_dir, &settings).map_err(|e| format!("保存设置失败：{e}"))?;
    *state.settings.lock().map_err(|e| e.to_string())? = settings.clone();
    state.sync_queue();
    Ok(settings)
}

/// 队列的完整状态；之后的变化通过 `queue://snapshot` 与 `queue://progress` 事件推送
#[tauri::command]
pub fn queue_snapshot(state: State<'_, AppState>) -> QueueSnapshot {
    state.queue.snapshot()
}

/// 加入队列。命令由后端按计划重新生成，不使用界面上的预览命令
#[tauri::command]
pub fn queue_add(state: State<'_, AppState>, items: Vec<QueueItem>) -> Vec<String> {
    state.queue.add(items)
}

#[tauri::command]
pub fn queue_control(state: State<'_, AppState>, op: QueueOp) -> CmdResult<()> {
    state.queue.apply(op)
}

/// 导入文件与文件夹（文件夹递归扫描），逐个用 ffprobe 分析。版本过低的 ffmpeg 也能分析，只是不能转码
#[tauri::command]
pub async fn import_media(app: AppHandle, paths: Vec<String>) -> CmdResult<ImportResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let caps = current_caps(&app)?;
        if !matches!(caps.status, EnvStatus::Ready | EnvStatus::TooOld) || caps.ffprobe_path.is_empty() {
            return Err("还没有可用的 ffprobe，请先在“环境与硬件”页解决 ffmpeg 的问题".to_string());
        }
        let inputs: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let emitter = app.clone();
        Ok(import_paths(&inputs, Path::new(&caps.ffprobe_path), &SystemRunner, IMPORT_WORKERS, &move |p| {
            let _ = emitter.emit(EVENT_IMPORT_PROGRESS, p);
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}
