//! Tauri 外壳：只做命令注册、事件转发与窗口能力，不写任何决策逻辑（设计文档 1.1）。

mod commands;
mod state;

use tauri::{Manager, RunEvent};

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            app.manage(AppState::load(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_capabilities,
            commands::get_settings,
            commands::save_settings,
            commands::import_media,
            commands::queue_snapshot,
            commands::queue_add,
            commands::queue_control,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // 退出时结束正在跑的 ffmpeg；任务状态不改，下次启动时按"没跑完"重新排队
            if let RunEvent::Exit = event {
                app.state::<AppState>().queue.shutdown();
            }
        });
}
