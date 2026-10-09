use super::{selection::Validation, BrowserInfo};
use crate::adapters::browser::BrowserSession;
use serde_json::json;
use std::{fs, path::Path, sync::Arc};
use tauri::{AppHandle, Manager};
use tokio::sync::watch;

/// Exercise the same Playwright executable path with an empty, disposable profile.
/// No login data or external page is used; cancellation owns the whole process tree.
pub(super) async fn run(
    app: &AppHandle,
    directory: &Path,
    browser: &BrowserInfo,
    cancelled: &mut watch::Receiver<bool>,
) -> Result<String, String> {
    if *cancelled.borrow() {
        return Err("浏览器检查已取消".into());
    }
    let before = Validation::capture(directory, browser, String::new())
        .ok_or("浏览器组件不完整，请修复组件")?;
    let root = app
        .path()
        .app_cache_dir()
        .map_err(|_| "无法创建浏览器检查目录")?
        .join("browser-probes");
    fs::create_dir_all(&root).map_err(|_| "无法创建浏览器检查目录")?;
    let profile = tempfile::Builder::new()
        .prefix("probe-")
        .tempdir_in(root)
        .map_err(|_| "无法创建浏览器检查目录")?;
    let mut session = BrowserSession::spawn(directory, Arc::new(|_| {}))?;
    let result = tokio::select! {
        result = session.request("probe", json!({ "profileDir": profile.path(), "executablePath": browser.path })) => result.map_err(|error| error.to_string()),
        _ = cancelled.changed() => Err("浏览器检查已取消".to_string()),
    };
    session.close().await;
    let result = result?;
    if !before.matches(directory, browser) {
        return Err("浏览器或辅助组件在检查期间发生变化，请重新验证".into());
    }
    let version = result
        .get("version")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty() && value.len() <= 100)
        .ok_or("浏览器未返回有效版本，无法确认可用性")?;
    Ok(version.to_string())
}
