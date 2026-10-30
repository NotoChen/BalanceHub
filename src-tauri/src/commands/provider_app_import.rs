use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::{
    models::{ProviderAppImportResult, ProviderAppImportTarget},
    platform::provider_app,
    services::provider_app_import,
    state::AppState,
};

use super::run_blocking;

#[tauri::command]
pub(crate) async fn open_provider_app_import(
    app: AppHandle,
    id: String,
    target: ProviderAppImportTarget,
) -> Result<ProviderAppImportResult, String> {
    // Read only the chosen provider from memory. Opening an integration never
    // triggers credential completion, an asset scan or a configuration write.
    let import_url = run_blocking("准备应用导入", move || {
        let state = app.state::<AppState>();
        let data = state
            .data
            .read()
            .map_err(|_| "读取中转站配置失败".to_string())?;
        let provider = data
            .providers
            .iter()
            .find(|provider| provider.identity.id == id)
            .ok_or_else(|| "中转站不存在，请刷新后重试".to_string())?;
        provider_app_import::build_link(provider, target)
    })
    .await?;

    let launch_url = import_url.clone();
    let launch_error = match tokio::time::timeout(
        Duration::from_secs(24),
        run_blocking("打开导入应用", move || {
            provider_app::open(target, &launch_url)
        }),
    )
    .await
    {
        Ok(result) => result.err(),
        Err(_) => Some(format!(
            "系统打开请求超时，请检查 {} 是否已打开",
            target.app_label()
        )),
    };
    Ok(ProviderAppImportResult {
        app_label: target.app_label(),
        target_label: target.label(),
        import_url: launch_error.as_ref().map(|_| import_url),
        launch_error,
    })
}
