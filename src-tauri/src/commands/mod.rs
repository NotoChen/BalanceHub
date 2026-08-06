pub(crate) mod app;
pub(crate) mod cli;
pub(crate) mod provider;

pub(crate) async fn run_blocking<T, F>(label: &'static str, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|err| format!("{label}任务异常: {err}"))?
}
