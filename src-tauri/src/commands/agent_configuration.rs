//! Window-bound configuration authority; submission returns before file work.
use crate::{models::*, services::agent_cli::configuration::ConfigurationService, state::AppState};
use std::{sync::Arc, time::Duration};
use tauri::{AppHandle, Emitter, Manager, Window};
use tauri_plugin_opener::OpenerExt;

pub(super) fn actor_and_service(
    app: &AppHandle,
    window: &Window,
) -> Result<(String, Arc<ConfigurationService>), AgentConfigurationError> {
    let state = app.state::<AppState>();
    let actor = state.asset_actor(window.label()).map_err(|_| internal())?;
    Ok((actor, state.agent_configuration(app)?))
}

pub(super) async fn run_configuration<T, F>(task: F) -> Result<T, AgentConfigurationError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AgentConfigurationError> + Send + 'static,
{
    // Service traversal/codec loops enforce the 10 second work budget. This is
    // an outer IPC bound, not a claim that dropping a future cancels a write.
    tokio::time::timeout(
        Duration::from_secs(15),
        tauri::async_runtime::spawn_blocking(task),
    )
    .await
    .map_err(|_| AgentConfigurationError::new(AgentConfigurationErrorKind::Timeout))?
    .map_err(|_| internal())?
}

#[tauri::command]
pub(crate) async fn list_agent_configuration_sources(
    app: AppHandle,
    window: Window,
    request: AgentConfigurationListRequest,
) -> Result<AgentConfigurationSnapshot, AgentConfigurationError> {
    let (actor, service) = actor_and_service(&app, &window)?;
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    run_configuration(move || service.list_sources(&actor, request, Some(&settings))).await
}

#[tauri::command]
pub(crate) async fn read_agent_configuration_source(
    app: AppHandle,
    window: Window,
    request: AgentConfigurationSourceRequest,
) -> Result<AgentConfigurationReadResult, AgentConfigurationError> {
    let (actor, service) = actor_and_service(&app, &window)?;
    run_configuration(move || service.read_source(&actor, request)).await
}

#[tauri::command]
pub(crate) async fn begin_agent_configuration_edit(
    app: AppHandle,
    window: Window,
    request: AgentConfigurationSourceRequest,
) -> Result<AgentConfigurationEdit, AgentConfigurationError> {
    let (actor, service) = actor_and_service(&app, &window)?;
    run_configuration(move || service.begin_edit(&actor, request)).await
}

#[tauri::command]
pub(crate) async fn begin_agent_resource_edit(
    app: AppHandle,
    window: Window,
    request: AgentResourceEditRequest,
    request_id: String,
) -> Result<AgentConfigurationEdit, AgentConfigurationError> {
    let (actor, service) = actor_and_service(&app, &window)?;
    let catalog = app
        .state::<AppState>()
        .agent_catalog(&app)
        .map_err(resource_read_error)?;
    let publication = catalog
        .published_catalog(&actor, request.workspace.as_deref())
        .map_err(resource_read_error)?;
    let kind = publication
        .snapshot
        .inventory
        .assets
        .iter()
        .find(|asset| asset.stable_id == request.asset_id)
        .map(|asset| asset.agent_kind)
        .ok_or_else(|| resource_read_error("此资源已变化，请刷新列表后重新打开".to_owned()))?;
    let inspector = super::agent_catalog::inspector(&app, request.workspace)
        .map_err(|_| AgentConfigurationError::new(AgentConfigurationErrorKind::InvalidRequest))?;
    let worker_actor = actor.clone();
    catalog
        .run_read(&actor, "edit", &request_id, move |control| {
            let snapshot = publication.snapshot_for(kind);
            let mut result = service.begin_resource_edit(
                &worker_actor,
                &snapshot,
                &request.asset_id,
                request.document_id.as_deref(),
                &control,
            );
            if result.as_ref().is_err_and(|error| {
                matches!(
                    error.kind,
                    AgentConfigurationErrorKind::SourceChanged
                        | AgentConfigurationErrorKind::RootChanged
                )
            }) {
                control.check()?;
                let current =
                    super::agent_catalog::refresh_read_inventory(inspector.as_ref(), kind)?;
                control.check()?;
                let snapshot = publication.remember_agent(kind, current);
                result = service.begin_resource_edit(
                    &worker_actor,
                    &snapshot,
                    &request.asset_id,
                    request.document_id.as_deref(),
                    &control,
                );
            }
            Ok(result)
        })
        .await
        .map_err(resource_read_error)?
}

fn resource_read_error(message: String) -> AgentConfigurationError {
    AgentConfigurationError {
        kind: AgentConfigurationErrorKind::SourceUnavailable,
        message,
        diagnostics: Vec::new(),
    }
}

#[tauri::command]
pub(crate) async fn open_agent_resource_link(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = reqwest::Url::parse(&url).map_err(|_| "文档链接无效")?;
    if !matches!(parsed.scheme(), "https" | "http")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("仅支持 HTTP 或 HTTPS 文档链接".to_owned());
    }
    super::run_blocking("打开资源文档", move || {
        app.opener()
            .open_url(parsed.as_str(), None::<&str>)
            .map_err(|_| "无法打开文档链接".to_owned())
    })
    .await
}

#[tauri::command]
pub(crate) async fn plan_agent_configuration_save(
    app: AppHandle,
    window: Window,
    request: AgentConfigurationSaveRequest,
) -> Result<AgentConfigurationPlan, AgentConfigurationError> {
    let (actor, service) = actor_and_service(&app, &window)?;
    let ticket = service.begin_plan_save(&actor, request)?;
    run_configuration(move || service.plan_save_with_ticket(ticket)).await
}

#[tauri::command]
pub(crate) fn apply_agent_configuration_plan(
    app: AppHandle,
    window: Window,
    request: AgentConfigurationApplyRequest,
) -> Result<AgentConfigurationOperation, AgentConfigurationError> {
    let (actor, service) = actor_and_service(&app, &window)?;
    let (operation, created) = service.start(&actor, request)?;
    if created {
        let id = operation.id.clone();
        tauri::async_runtime::spawn_blocking(move || {
            service.run_operation(&id, |operation| {
                let _ = app.emit("agent-configuration-operation", operation);
            });
        });
    }
    Ok(operation)
}

#[tauri::command]
pub(crate) fn discard_agent_configuration_edit(
    app: AppHandle,
    window: Window,
    edit_id: String,
) -> Result<(), AgentConfigurationError> {
    let (actor, service) = actor_and_service(&app, &window)?;
    service.discard_edit(&actor, &edit_id)
}

#[tauri::command]
pub(crate) async fn get_agent_configuration_operation(
    app: AppHandle,
    window: Window,
    operation_id: String,
) -> Result<AgentConfigurationOperation, AgentConfigurationError> {
    let (_, service) = actor_and_service(&app, &window)?;
    run_configuration(move || service.get_operation(&operation_id)).await
}

#[tauri::command]
pub(crate) async fn list_agent_configuration_operations(
    app: AppHandle,
    window: Window,
) -> Result<Vec<AgentConfigurationOperation>, AgentConfigurationError> {
    let (_, service) = actor_and_service(&app, &window)?;
    run_configuration(move || service.list_operations()).await
}

#[tauri::command]
pub(crate) fn cancel_agent_configuration_operation(
    app: AppHandle,
    window: Window,
    operation_id: String,
) -> Result<AgentConfigurationOperation, AgentConfigurationError> {
    let (actor, service) = actor_and_service(&app, &window)?;
    service.cancel_operation(&actor, &operation_id)
}

#[tauri::command]
pub(crate) async fn open_agent_configuration_source(
    app: AppHandle,
    window: Window,
    request: AgentConfigurationOpenRequest,
) -> Result<(), AgentConfigurationError> {
    let (actor, service) = actor_and_service(&app, &window)?;
    run_configuration(move || {
        let target = request.target;
        service.open_source(&actor, request, |path| match target {
            AgentAssetOpenTarget::Asset => app
                .opener()
                .open_path(path.to_string_lossy(), None::<&str>)
                .map_err(|_| "无法打开来源".to_owned()),
            AgentAssetOpenTarget::Reveal => app
                .opener()
                .reveal_item_in_dir(path)
                .map_err(|_| "无法定位来源".to_owned()),
        })
    })
    .await
}

fn internal() -> AgentConfigurationError {
    AgentConfigurationError::new(AgentConfigurationErrorKind::InternalFailure)
}
