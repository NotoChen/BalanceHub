//! WebDAV 配置同步命令。实现来自 `balancehub_core::api`，与服务端共用。

use serde_json::json;

use super::dispatch_value;
use crate::models::CloudSyncSettingsInput;

#[tauri::command]
pub(crate) async fn cloud_sync_state() -> Result<serde_json::Value, String> {
    dispatch_value("cloud_sync_state", json!({})).await
}

#[tauri::command]
pub(crate) async fn cloud_sync_save(
    input: CloudSyncSettingsInput,
) -> Result<serde_json::Value, String> {
    dispatch_value("cloud_sync_save", json!({ "input": input })).await
}

#[tauri::command]
pub(crate) async fn cloud_sync_start() -> Result<serde_json::Value, String> {
    dispatch_value("cloud_sync_start", json!({})).await
}

#[tauri::command]
pub(crate) async fn cloud_sync_test() -> Result<serde_json::Value, String> {
    dispatch_value("cloud_sync_test", json!({})).await
}

#[tauri::command]
pub(crate) async fn cloud_sync_confirm(
    review_id: String,
    resolutions: Vec<crate::models::CloudSyncResolution>,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "cloud_sync_confirm",
        json!({ "reviewId": review_id, "resolutions": resolutions }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn cloud_sync_restore() -> Result<serde_json::Value, String> {
    dispatch_value("cloud_sync_restore", json!({})).await
}

#[tauri::command]
pub(crate) async fn cloud_sync_cancel(task_id: String) -> Result<serde_json::Value, String> {
    dispatch_value("cloud_sync_cancel", json!({ "taskId": task_id })).await
}

#[tauri::command]
pub(crate) async fn cloud_sync_compare(
    review_id: String,
    key: String,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "cloud_sync_compare",
        json!({ "reviewId": review_id, "key": key }),
    )
    .await
}
