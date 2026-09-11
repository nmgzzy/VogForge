//! 暴露给前端的命令。耗时操作放到阻塞线程池，进度通过事件推送。

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager, State};
use vidforge_core::config::{self, Settings};
use vidforge_core::ffmpeg::capability::{ProbeContext, probe};
use vidforge_core::ffmpeg::exec::SystemRunner;
use vidforge_core::ffmpeg::locate::SystemEnv;
use vidforge_core::model::{Capabilities, Platform};

use crate::state::AppState;

pub const EVENT_PROBE_PROGRESS: &str = "probe://progress";

type CmdResult<T> = Result<T, String>;

/// 探测环境能力。`force` 为 false 时优先用缓存。
#[tauri::command]
pub async fn get_capabilities(app: AppHandle, force: bool) -> CmdResult<Capabilities> {
    tauri::async_runtime::spawn_blocking(move || {
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
        Ok(caps)
    })
    .await
    .map_err(|e| e.to_string())?
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
    Ok(settings)
}
