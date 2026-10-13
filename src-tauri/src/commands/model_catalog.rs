use crate::{models::ModelMetadataSnapshot, state::AppState};
use tauri::{AppHandle, Emitter, Manager};

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
    let _ = get_model_metadata(app.clone(), ids, false).await?;
    app.state::<AppState>().model_catalog(&app)?.estimate(usage)
}

#[tauri::command]
pub(crate) async fn get_model_metadata(
    app: AppHandle,
    ids: Vec<String>,
    refresh: bool,
) -> Result<ModelMetadataSnapshot, String> {
    if ids.len() > 2000 || ids.iter().any(|id| id.len() > 512) {
        return Err("模型资料查询数量或名称超出范围".to_owned());
    }
    let service_app = app.clone();
    let service = super::run_blocking("读取模型资料缓存", move || {
        service_app.state::<AppState>().model_catalog(&service_app)
    })
    .await?;
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    let snapshot = service.snapshot(&ids)?;
    if refresh || snapshot.updated_at.is_none() {
        let _ = service.refresh(&settings, refresh).await;
        let current = service.snapshot(&ids)?;
        if current.updated_at != snapshot.updated_at {
            let _ = app.emit("model-metadata-updated", ());
        }
        return Ok(current);
    }
    if snapshot.stale {
        let updated_at = snapshot.updated_at.clone();
        tauri::async_runtime::spawn(async move {
            let _ = service.refresh(&settings, false).await;
            if service
                .snapshot(&[])
                .is_ok_and(|current| current.updated_at != updated_at)
            {
                let _ = app.emit("model-metadata-updated", ());
            }
        });
    }
    Ok(snapshot)
}
