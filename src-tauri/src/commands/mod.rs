pub(crate) mod agent;
pub(crate) mod app;
pub(crate) mod browser_runtime;
pub(crate) mod cloud_sync;
pub(crate) mod login_accounts;
pub(crate) mod model_catalog;
pub(crate) mod oauth_accounts;
pub(crate) mod provider;
pub(crate) mod provider_app_import;
pub(crate) mod provider_browser_login;

pub(crate) async fn run_blocking<T, F>(label: &'static str, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|err| format!("{label}任务异常: {err}"))?
}

/// 执行与无界面后端共用的命令实现，并把结果解析回 IPC 类型。
///
/// 桌面在这里只保留自己的编排副作用（进度通道、窗口），业务实现全部来自
/// `balancehub_core::api`，避免两套代码各自漂移。
pub(crate) async fn dispatch<T>(command: &str, args: serde_json::Value) -> Result<T, String>
where
    T: serde::de::DeserializeOwned,
{
    dispatch_with_progress(command, args, None).await
}

/// 直接透传核心返回的 JSON。视图类型含 `&'static str` 等仅序列化字段，
/// 反序列化只会增加无意义的包装，前端类型才是这些结构的接收契约。
pub(crate) async fn dispatch_value(
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    dispatch_value_with_progress(command, args, None).await
}

pub(crate) async fn dispatch_value_with_progress(
    command: &str,
    args: serde_json::Value,
    progress: Option<std::sync::Arc<dyn crate::contracts::ProgressSink>>,
) -> Result<serde_json::Value, String> {
    balancehub_core::api::dispatch_with_progress(command, args, progress).await
}

pub(crate) async fn dispatch_with_progress<T>(
    command: &str,
    args: serde_json::Value,
    progress: Option<std::sync::Arc<dyn crate::contracts::ProgressSink>>,
) -> Result<T, String>
where
    T: serde::de::DeserializeOwned,
{
    let value = balancehub_core::api::dispatch_with_progress(command, args, progress).await?;
    serde_json::from_value(value).map_err(|error| format!("命令 {command} 返回结构无效：{error}"))
}
