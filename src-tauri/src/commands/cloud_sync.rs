use super::run_blocking;
use crate::{models::*, services::cloud_sync::Action, state::AppState};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub(crate) async fn cloud_sync_state(app: AppHandle) -> Result<CloudSyncSnapshot, String> {
    run_blocking("读取同步状态", move || {
        Ok(app.state::<AppState>().cloud_sync(&app)?.snapshot())
    })
    .await
}

#[tauri::command]
pub(crate) async fn cloud_sync_save(
    app: AppHandle,
    input: CloudSyncSettingsInput,
) -> Result<CloudSyncSnapshot, String> {
    run_blocking("保存同步设置", move || {
        app.state::<AppState>().cloud_sync(&app)?.save(&app, input)
    })
    .await
}

async fn start(app: AppHandle, action: Action) -> Result<CloudSyncSnapshot, String> {
    run_blocking("启动同步任务", move || {
        let service = app.state::<AppState>().cloud_sync(&app)?;
        service.start(app, action, false)
    })
    .await
}

#[tauri::command]
pub(crate) async fn cloud_sync_start(app: AppHandle) -> Result<CloudSyncSnapshot, String> {
    start(app, Action::Sync).await
}

#[tauri::command]
pub(crate) async fn cloud_sync_test(app: AppHandle) -> Result<CloudSyncSnapshot, String> {
    start(app, Action::Test).await
}

#[tauri::command]
pub(crate) async fn cloud_sync_confirm(
    app: AppHandle,
    review_id: String,
    resolutions: Vec<CloudSyncResolution>,
) -> Result<CloudSyncSnapshot, String> {
    start(
        app,
        Action::Confirm {
            review_id,
            resolutions,
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn cloud_sync_restore(app: AppHandle) -> Result<CloudSyncSnapshot, String> {
    start(app, Action::Restore).await
}

#[tauri::command]
pub(crate) async fn cloud_sync_cancel(
    app: AppHandle,
    task_id: String,
) -> Result<CloudSyncSnapshot, String> {
    run_blocking("取消同步", move || {
        app.state::<AppState>()
            .cloud_sync(&app)?
            .cancel(&app, &task_id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn cloud_sync_compare(
    app: AppHandle,
    review_id: String,
    key: String,
) -> Result<CloudSyncComparison, String> {
    run_blocking("读取同步差异", move || {
        app.state::<AppState>()
            .cloud_sync(&app)?
            .compare(&review_id, &key)
    })
    .await
}
