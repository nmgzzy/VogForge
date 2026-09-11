//! 应用级共享状态。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};
use vidforge_core::config::{self, Settings};
use vidforge_core::model::{Capabilities, JobProgressEvent, QueueSnapshot};
use vidforge_core::queue::process::SystemTools;
use vidforge_core::queue::{EventSink, Queue, QueueDeps, SystemClock};

pub const EVENT_QUEUE_SNAPSHOT: &str = "queue://snapshot";
pub const EVENT_QUEUE_PROGRESS: &str = "queue://progress";

pub struct AppState {
    pub app_dir: PathBuf,
    pub settings: Mutex<Settings>,
    /// 最近一次探测结果
    pub caps: Mutex<Option<Capabilities>>,
    /// 串行化探测：并发调用时后来者等待，然后直接命中缓存
    pub probe_lock: Mutex<()>,
    pub queue: Queue,
}

/// 队列状态变化转成前端事件
struct TauriSink(AppHandle);

impl EventSink for TauriSink {
    fn snapshot(&self, snapshot: &QueueSnapshot) {
        let _ = self.0.emit(EVENT_QUEUE_SNAPSHOT, snapshot);
    }

    fn progress(&self, event: &JobProgressEvent) {
        let _ = self.0.emit(EVENT_QUEUE_PROGRESS, event);
    }
}

impl AppState {
    pub fn load(app: AppHandle) -> AppState {
        let app_dir = config::app_dir();
        let _ = std::fs::create_dir_all(&app_dir);
        let settings = config::load_settings(&app_dir);
        // 上次没跑完的任务会重新排队；探测完成（set_environment）后才开始执行
        let queue = Queue::start(QueueDeps {
            tools: Arc::new(SystemTools),
            clock: Arc::new(SystemClock),
            sink: Arc::new(TauriSink(app)),
            store: Some(Queue::store_path(&app_dir)),
        });
        AppState { app_dir, settings: Mutex::new(settings), caps: Mutex::new(None), probe_lock: Mutex::new(()), queue }
    }

    /// 探测结果或设置变化后同步给队列
    pub fn sync_queue(&self) {
        let caps = self.caps.lock().ok().and_then(|c| c.clone());
        let settings = self.settings.lock().map(|s| s.clone()).unwrap_or_default();
        if let Some(caps) = caps {
            self.queue.set_environment(caps, settings);
        }
    }
}
