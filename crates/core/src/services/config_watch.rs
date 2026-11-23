//! 外部 `config.json` 热重载监听。
//!
//! 用户可能在系统编辑器里修改配置。这里不依赖文件系统事件（跨平台事件源
//! 和编辑器临时文件差异较大），而是以很低的开销轮询 mtime + 大小；只有
//! 指纹变化时才重新读取、校验并在事务锁内合并运行状态。解析失败时保留
//! 当前内存配置和磁盘文件，等待用户修复。

use crate::{
    app_events::{CONFIG_RELOADED_EVENT, CONFIG_RELOAD_FAILED_EVENT, PROVIDERS_CHANGED_EVENT},
    host,
    services::provider_service::ProviderService,
    state::CoreState,
    storage,
};
use serde_json::json;
use std::{
    sync::{Arc, Once},
    thread,
    time::Duration,
};

/// 轮询间隔足够让编辑器保存后几乎立即生效，同时只是每分钟约 75 次 stat。
const POLL_INTERVAL: Duration = Duration::from_millis(800);

pub fn start(core: Arc<CoreState>) {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        let result = thread::Builder::new()
            .name("balancehub-config-watch".to_string())
            .spawn(move || watch(core));
        if let Err(error) = result {
            log::warn!("启动 config.json 热重载监听失败：{error}");
        }
    });
}

fn watch(core: Arc<CoreState>) {
    let mut last_error: Option<String> = None;
    let mut failed_signature: Option<String> = None;
    loop {
        thread::sleep(POLL_INTERVAL);
        if core.load_error().is_some() {
            if !core.can_retry_load() {
                // 云同步事务未完成等保护态只能重启恢复，热重载不能绕过。
                continue;
            }
            let signature = match storage::startup_source_signature() {
                Ok(signature) => signature,
                Err(error) => {
                    report_error(&mut last_error, error);
                    continue;
                }
            };
            if failed_signature.as_deref() == Some(signature.as_str()) {
                // 文件没有变化时不反复解析同一份损坏配置；修好或替换后再重试。
                continue;
            }
            match ProviderService::new().load_app_data() {
                Ok(data) => {
                    last_error = None;
                    failed_signature = None;
                    announce(Some(data.providers.len()), true);
                    host::emit(PROVIDERS_CHANGED_EVENT, json!(null));
                    host::refresh_tray();
                }
                Err(error) => {
                    failed_signature = Some(signature);
                    report_error(&mut last_error, error);
                }
            }
            continue;
        }

        match storage::reload_config_from_disk(&core) {
            Ok(Some((provider_count, settings_changed))) => {
                last_error = None;
                announce(Some(provider_count), settings_changed);
                host::emit(PROVIDERS_CHANGED_EVENT, json!(null));
                host::refresh_tray();
                // 外部配置可能删除了站点或替换了凭据；旧配置上挂起的等待
                // 任务不能继续占据窗口或稍后写回已经不存在的中转站。
                crate::services::check_in_tasks::cancel_outdated();
                crate::services::provider_browser_login::cancel_outdated();
            }
            Ok(None) => last_error = None,
            Err(error) => report_error(&mut last_error, error),
        }
    }
}

fn announce(provider_count: Option<usize>, settings_changed: bool) {
    let path = config_path();
    host::emit(
        CONFIG_RELOADED_EVENT,
        json!({
            "path": path,
            "providerCount": provider_count,
            "settingsChanged": settings_changed,
        }),
    );
}

fn report_error(last_error: &mut Option<String>, error: String) {
    if last_error.as_deref() == Some(error.as_str()) {
        return;
    }
    log::warn!("config.json 热重载失败：{error}");
    *last_error = Some(error.clone());
    host::emit(
        CONFIG_RELOAD_FAILED_EVENT,
        json!({ "path": config_path(), "error": error }),
    );
}

fn config_path() -> String {
    host::config_dir()
        .map(|directory| directory.join("config.json").display().to_string())
        .unwrap_or_else(|_| "config.json".to_string())
}
