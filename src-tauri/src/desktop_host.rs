//! 桌面宿主：把 Tauri 的数据目录、事件和系统集成能力接到核心宿主接口。
//!
//! 核心 crate 无法为 Tauri 的 `AppHandle` 实现自己的 trait（孤儿规则），
//! 所以这里用本地包装类型。启动时注册到 `balancehub_core::host` 后，核心
//! 服务通过 `host::current()` 取用，不再逐层传递句柄。

use balancehub_core::{
    host::Host,
    models::{AppData, AppSettings},
    state::CoreState,
};
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use tauri::Manager;

pub struct DesktopHost {
    app: tauri::AppHandle,
    core: Arc<OnceLock<Arc<CoreState>>>,
}

impl DesktopHost {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            app,
            core: Arc::new(OnceLock::new()),
        }
    }

    /// 配置加载完成、`AppState` 建立后回填核心状态。
    pub fn attach_core(&self, core: Arc<CoreState>) {
        let _ = self.core.set(core);
    }
}

impl Host for DesktopHost {
    fn kind(&self) -> &'static str {
        "desktop"
    }

    fn core_state(&self) -> Arc<CoreState> {
        self.core.get().expect("核心状态尚未接入桌面宿主").clone()
    }

    fn data_dir(&self) -> Result<PathBuf, String> {
        crate::desktop::data_dir(&self.app)
    }

    fn config_dir(&self) -> Result<PathBuf, String> {
        crate::desktop::config_dir(&self.app)
    }

    fn cache_dir(&self) -> Result<PathBuf, String> {
        self.app
            .path()
            .app_cache_dir()
            .map_err(|error| error.to_string())
    }

    fn emit(&self, event: &str, payload: serde_json::Value) {
        let _ = tauri::Emitter::emit(&self.app, event, payload);
    }

    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        use tauri_plugin_notification::NotificationExt;
        self.app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        use tauri_plugin_opener::OpenerExt;
        self.app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|error| error.to_string())
    }

    fn settings_changed(&self, previous: &AppSettings, current: &AppSettings) {
        crate::services::cli_sessions::reconfigure_index(&self.app, previous, current);
    }

    fn refresh_tray(&self) {
        crate::tray::refresh_from_state(&self.app);
    }

    fn app_data_changed(&self, previous: &AppData, next: &AppData) {
        let state = self.app.state::<crate::state::AppState>();
        if crate::services::cloud_sync::app_changed(previous, next) {
            state.cloud_sync_signal.changed();
        }
    }
}
