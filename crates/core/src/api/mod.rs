//! 传输无关的命令分发。
//!
//! 桌面端把每个命令注册成 `#[tauri::command]`；服务端把同一个命令名
//! 经 HTTP RPC 送进来。两边执行的是同一份业务实现，参数与返回值都按
//! 前端当前的 IPC 结构序列化，因此前端不需要知道请求走的是哪条通道。

mod browser;
mod checkin;
mod cloud_sync;
mod credentials;
mod oauth;
mod provider;
mod settings;

use crate::contracts::ProgressSink;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::Arc;

pub use browser::COMMANDS as BROWSER_COMMANDS;
pub use checkin::COMMANDS as CHECK_IN_COMMANDS;
pub use cloud_sync::COMMANDS as CLOUD_SYNC_COMMANDS;
pub use credentials::COMMANDS as CREDENTIAL_COMMANDS;
pub use oauth::COMMANDS as OAUTH_COMMANDS;
pub use provider::COMMANDS as PROVIDER_COMMANDS;
pub use settings::COMMANDS as SETTINGS_COMMANDS;

/// 服务端当前实现的命令名。前端据此隐藏桌面专属区块。
pub fn commands() -> Vec<&'static str> {
    settings::COMMANDS
        .iter()
        .chain(provider::COMMANDS)
        .chain(checkin::COMMANDS)
        .chain(oauth::COMMANDS)
        .chain(credentials::COMMANDS)
        .chain(cloud_sync::COMMANDS)
        .chain(browser::COMMANDS)
        .copied()
        .collect()
}

pub async fn dispatch(command: &str, args: Value) -> Result<Value, String> {
    dispatch_with_progress(command, args, None).await
}

/// `progress` 只有桌面宿主会提供（Tauri Channel）；服务端留空，进度按事件广播。
pub async fn dispatch_with_progress(
    command: &str,
    args: Value,
    progress: Option<Arc<dyn ProgressSink>>,
) -> Result<Value, String> {
    if settings::COMMANDS.contains(&command) {
        return settings::handle(command, &args).await;
    }
    if provider::COMMANDS.contains(&command) {
        return provider::handle(command, &args, progress).await;
    }
    if checkin::COMMANDS.contains(&command) {
        return checkin::handle(command, &args).await;
    }
    if oauth::COMMANDS.contains(&command) {
        return oauth::handle(command, &args).await;
    }
    if credentials::COMMANDS.contains(&command) {
        return credentials::handle(command, &args).await;
    }
    if cloud_sync::COMMANDS.contains(&command) {
        return cloud_sync::handle(command, &args).await;
    }
    if browser::COMMANDS.contains(&command) {
        return browser::handle(command, &args).await;
    }
    Err(format!("当前宿主不支持命令：{command}"))
}

/// 在阻塞线程池执行同步事务，避免持锁写盘占用异步工作线程。
async fn blocking<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    crate::runtime::spawn_blocking(task)
        .await
        .map_err(|error| format!("后台任务异常：{error}"))?
}

// ── 参数读取 ──────────────────────────────────────────────────────────

pub(crate) fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    let value = args.get(name).cloned().unwrap_or(Value::Null);
    serde_json::from_value(value).map_err(|error| format!("参数 {name} 无效：{error}"))
}

pub(crate) fn optional<T: DeserializeOwned>(args: &Value, name: &str) -> Result<Option<T>, String> {
    match args.get(name) {
        Some(Value::Null) | None => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| format!("参数 {name} 无效：{error}")),
    }
}

pub(crate) fn to_value<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| format!("结果序列化失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dispatch_supports_loading_app_data() {
        crate::host::testing::install();
        let data = dispatch("load_app_data", serde_json::json!({}))
            .await
            .unwrap();
        assert!(data.get("providers").is_some());
    }

    #[test]
    fn dispatch_future_is_send() {
        crate::host::testing::install();
        let future = dispatch("load_app_data", serde_json::json!({}));
        assert_send(future);
    }

    fn assert_send<T: Send>(_: T) {}

    #[tokio::test]
    async fn dispatch_supports_listing_oauth_logins() {
        crate::host::testing::install();
        let tasks = dispatch("list_oauth_logins", serde_json::json!({}))
            .await
            .unwrap();
        assert!(tasks.is_array());
    }

    #[tokio::test]
    async fn dispatch_rejects_unknown_commands() {
        let error = dispatch("agent_catalog", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(error.contains("不支持"));
    }
}
