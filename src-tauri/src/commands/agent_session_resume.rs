use crate::{
    models::{AgentSessionResumeOperation, AgentSessionResumeRequest},
    services::cli_sessions::resume::{self, AgentSessionResumeService},
    state::AppState,
};
use std::{sync::Arc, time::Duration};
use tauri::{AppHandle, Manager, Window};

fn service(app: &AppHandle) -> Arc<AgentSessionResumeService> {
    Arc::clone(&app.state::<AppState>().agent_session_resumes)
}

#[tauri::command]
pub(crate) async fn resume_agent_session(
    app: AppHandle,
    window: Window,
    request: AgentSessionResumeRequest,
) -> Result<AgentSessionResumeOperation, String> {
    let actor = app
        .state::<AppState>()
        .asset_actor(window.label())
        .map_err(|_| "无法识别当前窗口")?;
    let window_label = window.label().to_owned();
    let service = service(&app);
    let (operation, created) = service.start(&app, &actor, &window_label, request).await?;
    if created {
        resume::emit(&app, &operation);
        let run_app = app.clone();
        let run_service = Arc::clone(&service);
        let run_id = operation.id.clone();
        tauri::async_runtime::spawn_blocking(move || run_service.run(&run_app, &run_id));
        let timeout_id = operation.id.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(resume::OPERATION_TIMEOUT_SECONDS)).await;
            service.timeout(&app, &timeout_id);
        });
    }
    Ok(operation)
}

#[tauri::command]
pub(crate) async fn get_agent_session_resume_operation(
    app: AppHandle,
    window: Window,
    operation_id: String,
) -> Result<AgentSessionResumeOperation, String> {
    let actor = app
        .state::<AppState>()
        .asset_actor(window.label())
        .map_err(|_| "无法识别当前窗口")?;
    let service = service(&app);
    tauri::async_runtime::spawn_blocking(move || service.operation(&app, &actor, &operation_id))
        .await
        .map_err(|_| "读取继续任务失败")?
}

#[tauri::command]
pub(crate) async fn list_agent_session_resume_operations(
    app: AppHandle,
    window: Window,
) -> Result<Vec<AgentSessionResumeOperation>, String> {
    let actor = app
        .state::<AppState>()
        .asset_actor(window.label())
        .map_err(|_| "无法识别当前窗口")?;
    let service = service(&app);
    tauri::async_runtime::spawn_blocking(move || service.operations(&app, &actor))
        .await
        .map_err(|_| "读取继续任务列表失败".to_owned())
}

#[tauri::command]
pub(crate) async fn cancel_agent_session_resume_operation(
    app: AppHandle,
    window: Window,
    operation_id: String,
) -> Result<AgentSessionResumeOperation, String> {
    let actor = app
        .state::<AppState>()
        .asset_actor(window.label())
        .map_err(|_| "无法识别当前窗口")?;
    let service = service(&app);
    tauri::async_runtime::spawn_blocking(move || service.cancel(&app, &actor, &operation_id))
        .await
        .map_err(|_| "取消继续任务失败")?
}
