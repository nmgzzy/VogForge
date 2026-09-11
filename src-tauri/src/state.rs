//! 应用级共享状态。

use std::path::PathBuf;
use std::sync::Mutex;

use vidforge_core::config::{self, Settings};
use vidforge_core::model::Capabilities;

pub struct AppState {
    pub app_dir: PathBuf,
    pub settings: Mutex<Settings>,
    /// 最近一次探测结果
    pub caps: Mutex<Option<Capabilities>>,
    /// 串行化探测：并发调用时后来者等待，然后直接命中缓存
    pub probe_lock: Mutex<()>,
}

impl AppState {
    pub fn load() -> AppState {
        let app_dir = config::app_dir();
        let _ = std::fs::create_dir_all(&app_dir);
        let settings = config::load_settings(&app_dir);
        AppState { app_dir, settings: Mutex::new(settings), caps: Mutex::new(None), probe_lock: Mutex::new(()) }
    }
}
