use crate::state::AppState;
use serde_json::json;
use tauri::{AppHandle, Manager};

/// 会话用量的费用估算依赖桌面端的模型目录缓存；命令实现与无界面后端共用。
#[tauri::command]
pub(crate) async fn estimate_session_usage(
    app: AppHandle,
    usage: crate::models::SessionUsageSummary,
) -> Result<crate::models::SessionUsageSummary, String> {
    if usage.models.len() > 256 {
        return Err("会话模型数量超出范围".to_owned());
    }
    let ids = usage
        .models
        .iter()
        .filter_map(|model| model.model.clone())
        .collect();
    let _ = get_model_metadata(app.clone(), ids, false, None).await?;
    app.state::<AppState>().model_catalog()?.estimate(usage)
}

#[tauri::command]
pub(crate) async fn get_model_metadata(
    _app: AppHandle,
    ids: Vec<String>,
    refresh: bool,
    provider_id: Option<String>,
) -> Result<serde_json::Value, String> {
    super::dispatch_value(
        "get_model_metadata",
        json!({ "ids": ids, "refresh": refresh, "providerId": provider_id }),
    )
    .await
}
