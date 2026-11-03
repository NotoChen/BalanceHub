use serde_json::json;

/// 模型资料读取与无界面后端共用核心实现。
#[tauri::command]
pub(crate) async fn get_model_metadata(
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
