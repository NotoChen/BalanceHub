//! Explicit CLI lifecycle requests; submitted work never waits on the window.

use crate::{models::*, services::cli_paths::user_home, state::AppState};
use tauri::{AppHandle, Manager, Window};

#[tauri::command]
pub(crate) async fn get_agent_lifecycle_catalog(
    app: AppHandle,
    request: AgentLifecycleCatalogRequest,
) -> Result<AgentLifecycleCatalog, AgentLifecycleError> {
    let state = app.state::<AppState>();
    let service = state.agent_lifecycle();
    let settings = state
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    let home = user_home()
        .and_then(|home| home.canonicalize().ok())
        .ok_or_else(internal)?;
    tokio::time::timeout(
        std::time::Duration::from_secs(60),
        service.catalog(settings, home, request),
    )
    .await
    .map_err(|_| AgentLifecycleError::new(AgentLifecycleErrorKind::VersionCheckFailed))?
}

#[tauri::command]
pub(crate) async fn plan_agent_lifecycle(
    app: AppHandle,
    window: Window,
    request: AgentLifecyclePlanRequest,
) -> Result<AgentLifecyclePlan, AgentLifecycleError> {
    let state = app.state::<AppState>();
    let actor = state.asset_actor(window.label()).map_err(|_| internal())?;
    let service = state.agent_lifecycle();
    let settings = state
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    let home = user_home()
        .and_then(|home| home.canonicalize().ok())
        .ok_or_else(internal)?;
    tokio::time::timeout(
        std::time::Duration::from_secs(60),
        service.plan(&actor, settings, home, request),
    )
    .await
    .map_err(|_| AgentLifecycleError::new(AgentLifecycleErrorKind::VersionCheckFailed))?
}

#[tauri::command]
pub(crate) async fn apply_agent_lifecycle(
    app: AppHandle,
    window: Window,
    request: AgentLifecycleApplyRequest,
) -> Result<AgentLifecycleOperation, AgentLifecycleError> {
    let state = app.state::<AppState>();
    let actor = state.asset_actor(window.label()).map_err(|_| internal())?;
    let service = state.agent_lifecycle();
    let starter = std::sync::Arc::clone(&service);
    let operation = tauri::async_runtime::spawn_blocking(move || starter.start(&actor, &request))
        .await
        .map_err(|_| internal())??;
    let id = operation.id.clone();
    tauri::async_runtime::spawn_blocking(move || service.run_operation(&id));
    Ok(operation)
}

#[tauri::command]
pub(crate) async fn get_agent_lifecycle_operation(
    app: AppHandle,
    window: Window,
    operation_id: String,
) -> Result<AgentLifecycleOperation, AgentLifecycleError> {
    let state = app.state::<AppState>();
    let actor = state.asset_actor(window.label()).map_err(|_| internal())?;
    let service = state.agent_lifecycle();
    let settings = state
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut operation = service.operation(&actor, &operation_id)?;
        crate::services::agent_cli::lifecycle::annotate_launch(&mut operation, &settings);
        service.publish_launch(&actor, operation)
    })
    .await
    .map_err(|_| internal())?
}

#[tauri::command]
pub(crate) async fn list_agent_lifecycle_operations(
    app: AppHandle,
    window: Window,
) -> Result<Vec<AgentLifecycleOperation>, AgentLifecycleError> {
    let state = app.state::<AppState>();
    let actor = state.asset_actor(window.label()).map_err(|_| internal())?;
    let service = state.agent_lifecycle();
    let settings = state
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    let home = user_home()
        .and_then(|home| home.canonicalize().ok())
        .ok_or_else(internal)?;
    tauri::async_runtime::spawn_blocking(move || {
        service.reconcile_history(settings.clone(), home)?;
        let mut operations = service.operations(&actor)?;
        crate::services::agent_cli::lifecycle::annotate_launches(&mut operations, &settings);
        operations
            .into_iter()
            .map(|operation| service.publish_launch(&actor, operation))
            .collect()
    })
    .await
    .map_err(|_| internal())?
}

#[tauri::command]
pub(crate) async fn cancel_agent_lifecycle_operation(
    app: AppHandle,
    window: Window,
    operation_id: String,
) -> Result<AgentLifecycleOperation, AgentLifecycleError> {
    let state = app.state::<AppState>();
    let actor = state.asset_actor(window.label()).map_err(|_| internal())?;
    let service = state.agent_lifecycle();
    tauri::async_runtime::spawn_blocking(move || service.cancel(&actor, &operation_id))
        .await
        .map_err(|_| internal())?
}

fn internal() -> AgentLifecycleError {
    AgentLifecycleError::new(AgentLifecycleErrorKind::InternalFailure)
}
