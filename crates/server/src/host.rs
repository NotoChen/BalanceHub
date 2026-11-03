//! 服务端宿主：没有窗口、托盘和系统通知，只提供数据目录、日志事件
//! 和一条 WebSocket 事件流。

use balancehub_core::{
    host::Host,
    models::{AppData, AppSettings},
    state::CoreState,
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use tokio::sync::broadcast;

const EVENT_CAPACITY: usize = 256;

#[derive(Debug, Clone, Serialize)]
pub struct ServerEvent {
    pub event: String,
    pub payload: serde_json::Value,
}

pub struct ServerHost {
    data_dir: PathBuf,
    config_dir: PathBuf,
    cache_dir: PathBuf,
    events: broadcast::Sender<ServerEvent>,
    core: Arc<OnceLock<Arc<CoreState>>>,
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
        }
    }

    pub fn attach_core(&self, core: Arc<CoreState>) {
        let _ = self.core.set(core);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.events.subscribe()
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

    fn settings_changed(&self, previous: &AppSettings, current: &AppSettings) {
        balancehub_agent::settings_changed(previous, current);
    }

    /// 服务端同样有 Agent 共享资产库，参与 WebDAV 同步。
    fn cloud_library(
        &self,
    ) -> Option<Arc<dyn balancehub_core::services::cloud_sync::library::CloudLibrary>> {
        balancehub_agent::cloud_library()
    }
}
