//! 服务端宿主：没有窗口、托盘和系统通知，只提供数据目录、日志事件
//! 和一条 WebSocket 事件流。

use balancehub_core::{
    app_events::{BackgroundTaskEvent, BACKGROUND_TASK_EVENT},
    host::Host,
    models::AppData,
    state::CoreState,
};
use serde::Serialize;
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};
use tokio::sync::broadcast;

const EVENT_CAPACITY: usize = 256;
/// 服务端没有界面，后台任务失败只能从日志和状态接口看到；保留最近一段
/// 任务事件，既能解释「为什么没刷新」，又不会随运行时间无限增长。
const TASK_JOURNAL_EVENTS: usize = 50;

#[derive(Debug, Clone, Serialize)]
pub struct ServerEvent {
    pub event: String,
    pub payload: serde_json::Value,
}

#[derive(Default)]
struct TaskJournal {
    events: VecDeque<BackgroundTaskEvent>,
}

impl TaskJournal {
    fn push(&mut self, task: BackgroundTaskEvent) {
        self.events.push_back(task);
        while self.events.len() > TASK_JOURNAL_EVENTS {
            self.events.pop_front();
        }
    }

    /// 每个任务只返回最近一次状态，最新的任务排在前面。
    fn latest(&self) -> Vec<BackgroundTaskEvent> {
        let mut seen = HashSet::new();
        self.events
            .iter()
            .rev()
            .filter(|task| seen.insert(task.task_id.as_str()))
            .cloned()
            .collect()
    }
}

pub struct ServerHost {
    data_dir: PathBuf,
    config_dir: PathBuf,
    cache_dir: PathBuf,
    events: broadcast::Sender<ServerEvent>,
    core: Arc<OnceLock<Arc<CoreState>>>,
    journal: Mutex<TaskJournal>,
}

impl ServerHost {
    pub fn new(data_dir: PathBuf, config_dir: PathBuf, cache_dir: PathBuf) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            data_dir,
            config_dir,
            cache_dir,
            events,
            core: Arc::new(OnceLock::new()),
            journal: Mutex::new(TaskJournal::default()),
        }
    }

    pub fn attach_core(&self, core: Arc<CoreState>) {
        let _ = self.core.set(core);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.events.subscribe()
    }

    pub fn data_revision(&self) -> u64 {
        self.core.get().map_or(0, |core| core.current_revision())
    }

    pub fn task_journal(&self) -> Vec<BackgroundTaskEvent> {
        self.journal
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .latest()
    }

    fn record_task(&self, payload: &serde_json::Value) {
        let Ok(task) = serde_json::from_value::<BackgroundTaskEvent>(payload.clone()) else {
            log::warn!("后台任务事件格式无效，已跳过记录");
            return;
        };
        if task.status != "running" {
            match &task.error {
                Some(error) => {
                    log::warn!("后台任务「{}」结束：{}（{error}）", task.title, task.detail)
                }
                None => log::info!("后台任务「{}」结束：{}", task.title, task.detail),
            }
        }
        self.journal
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(task);
    }
}

impl Host for ServerHost {
    fn kind(&self) -> &'static str {
        "server"
    }

    fn core_state(&self) -> Arc<CoreState> {
        self.core.get().expect("核心状态尚未接入服务端宿主").clone()
    }

    fn data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.data_dir.clone())
    }

    fn config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.config_dir.clone())
    }

    fn cache_dir(&self) -> Result<PathBuf, String> {
        Ok(self.cache_dir.clone())
    }

    fn emit(&self, event: &str, payload: serde_json::Value) {
        if event == BACKGROUND_TASK_EVENT {
            self.record_task(&payload);
        }
        // 没有浏览器连着时发送会失败；事件本来就是可丢弃的实时通知。
        let _ = self.events.send(ServerEvent {
            event: event.to_owned(),
            payload,
        });
    }

    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        log::info!("通知 {title}: {body}");
        Ok(())
    }

    /// 服务端没有可打开的本地浏览器；需要授权的流程由调用方降级为手工链接。
    fn open_url(&self, _url: &str) -> Result<(), String> {
        Err("服务端不能打开本地浏览器".to_owned())
    }

    /// 有桌面会话的服务器同样可以启动终端；无显示环境时不提供入口。
    fn has_desktop_session(&self) -> bool {
        if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            return true;
        }
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
    }

    fn app_data_changed(&self, previous: &AppData, next: &AppData) {
        if balancehub_core::services::cloud_sync::app_changed(previous, next) {
            balancehub_core::services::cloud_sync::signal().changed();
        }
    }

    /// 服务端同样有 Agent 共享资产库，参与 WebDAV 同步。
    fn cloud_library(
        &self,
    ) -> Option<Arc<dyn balancehub_core::services::cloud_sync::library::CloudLibrary>> {
        balancehub_agent::cloud_library()
    }
}
