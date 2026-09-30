//! Window-bound asset inventory, safe source access and confirmed operations.

use crate::{
    models::*,
    services::agent_cli::environment::{
        self,
        access_registry::{AgentAssetAccessRequest, AgentAssetAccessTargetKind},
    },
    state::AppState,
};
use std::{path::Path, sync::Arc};
use tauri::{AppHandle, Manager, Window};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub(crate) async fn get_agent_environment_inventory(
    app: AppHandle,
    window: Window,
    workspace: Option<String>,
) -> Result<AgentEnvironmentInventory, String> {
    let state = app.state::<AppState>();
    let settings = state
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| "无法创建资产访问会话".to_owned())?;
    let registry = Arc::clone(&state.agent_asset_access);
    let normalized =
        environment::normalize_optional_workspace(workspace.as_deref().map(Path::new))?;
    let ticket = registry
        .begin_publish(&actor, normalized.as_deref())
        .map_err(|error| error.to_string())?;
    super::run_blocking("盘点 Agent 环境", move || {
        let mut build =
            environment::inventory_with_access(&settings, workspace.as_deref().map(Path::new))?;
        registry
            .publish_ticket(ticket, &mut build.inventory, build.access_evidence)
            .map_err(|error| error.to_string())?;
        Ok(build.inventory)
    })
    .await
}

#[tauri::command]
pub(crate) async fn read_agent_environment_asset(
    app: AppHandle,
    window: Window,
    request: AgentAssetReadRequest,
) -> Result<AgentAssetReadResult, AgentAssetAccessError> {
    read_target(app, window, request, AgentAssetAccessTargetKind::Asset).await
}

#[tauri::command]
pub(crate) async fn read_agent_environment_source(
    app: AppHandle,
    window: Window,
    request: AgentAssetReadRequest,
) -> Result<AgentAssetReadResult, AgentAssetAccessError> {
    read_target(app, window, request, AgentAssetAccessTargetKind::Source).await
}

async fn read_target(
    app: AppHandle,
    window: Window,
    request: AgentAssetReadRequest,
    kind: AgentAssetAccessTargetKind,
) -> Result<AgentAssetReadResult, AgentAssetAccessError> {
    let state = app.state::<AppState>();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessUnavailable))?;
    let registry = Arc::clone(&state.agent_asset_access);
    run_control(
        move || {
            let access = AgentAssetAccessRequest {
                actor: &actor,
                environment_id: &request.environment_id,
                workspace: request.workspace.as_deref().map(Path::new),
                target_id: &request.target_id,
                access_id: &request.access_id,
                target_kind: kind,
            };
            match kind {
                AgentAssetAccessTargetKind::Asset => environment::read_asset(&registry, access),
                AgentAssetAccessTargetKind::Source => environment::read_source(&registry, access),
                AgentAssetAccessTargetKind::ConfigurationSource => Err(AgentAssetAccessError::new(
                    AgentAssetAccessErrorKind::TargetMismatch,
                )),
            }
        },
        AgentAssetAccessError::new(AgentAssetAccessErrorKind::ReadFailed),
    )
    .await
}

#[tauri::command]
pub(crate) async fn open_agent_environment_asset(
    app: AppHandle,
    window: Window,
    request: AgentAssetOpenRequest,
) -> Result<(), AgentAssetAccessError> {
    open_target(app, window, request, AgentAssetAccessTargetKind::Asset).await
}

#[tauri::command]
pub(crate) async fn open_agent_environment_source(
    app: AppHandle,
    window: Window,
    request: AgentAssetOpenRequest,
) -> Result<(), AgentAssetAccessError> {
    open_target(app, window, request, AgentAssetAccessTargetKind::Source).await
}

async fn open_target(
    app: AppHandle,
    window: Window,
    request: AgentAssetOpenRequest,
    kind: AgentAssetAccessTargetKind,
) -> Result<(), AgentAssetAccessError> {
    let state = app.state::<AppState>();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessUnavailable))?;
    let registry = Arc::clone(&state.agent_asset_access);
    run_control(
        move || {
            let access = AgentAssetAccessRequest {
                actor: &actor,
                environment_id: &request.environment_id,
                workspace: request.workspace.as_deref().map(Path::new),
                target_id: &request.target_id,
                access_id: &request.access_id,
                target_kind: kind,
            };
            // The access layer retains all object guards until this callback returns.
            let open = |path: &Path| match request.target {
                AgentAssetOpenTarget::Asset => app
                    .opener()
                    .open_path(path.to_string_lossy(), None::<&str>)
                    .map_err(|_| "系统打开失败".to_owned()),
                AgentAssetOpenTarget::Reveal => app
                    .opener()
                    .reveal_item_in_dir(path)
                    .map_err(|_| "系统定位失败".to_owned()),
            };
            match kind {
                AgentAssetAccessTargetKind::Asset => environment::open_asset(
                    &registry,
                    access,
                    request.target,
                    &request.accepted_risks,
                    open,
                ),
                AgentAssetAccessTargetKind::Source => environment::open_source(
                    &registry,
                    access,
                    request.target,
                    &request.accepted_risks,
                    open,
                ),
                AgentAssetAccessTargetKind::ConfigurationSource => Err(AgentAssetAccessError::new(
                    AgentAssetAccessErrorKind::TargetMismatch,
                )),
            }
        },
        AgentAssetAccessError::new(AgentAssetAccessErrorKind::ExternalOpenFailed),
    )
    .await
}

#[tauri::command]
pub(crate) async fn plan_agent_asset(
    app: AppHandle,
    window: Window,
    request: AgentAssetPlanRequest,
    request_id: String,
    agent_kind: AgentCliKind,
) -> Result<AgentAssetPlan, AgentAssetMutationError> {
    let state = app.state::<AppState>();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| mutation_internal_error())?;
    let service = Arc::clone(&state.agent_asset_mutations);
    let reads = state
        .agent_catalog(&app)
        .map_err(|_| mutation_internal_error())?;
    let base = super::agent_catalog::inspector(&app, request.workspace.clone())
        .map_err(|_| mutation_internal_error())?;
    let mut kinds = std::collections::BTreeSet::from([agent_kind]);
    if let Ok(publication) = reads.published_catalog(&actor, request.workspace.as_deref()) {
        if let Some(item) = publication.catalog.assets.iter().find(|item| {
            item.bindings
                .iter()
                .any(|binding| binding.id == request.asset_id)
        }) {
            let scoped = AgentCatalogPlanRequest {
                source: AgentCatalogPlanSource::Catalog {
                    asset_id: item.id.clone(),
                    expected_version: item.version,
                },
                action: if request.action == AgentAssetActionKind::Enable {
                    AgentCatalogAction::Enable
                } else {
                    AgentCatalogAction::Disable
                },
                target_ids: vec![request.asset_id.clone()],
                expected_revision: publication.catalog.revision.clone(),
                workspace: request.workspace.clone(),
            };
            kinds.extend(
                crate::services::agent_cli::catalog::plan_scope::agents(&publication, &scoped)
                    .map_err(|_| mutation_internal_error())?,
            );
        }
    }
    let inspector =
        crate::services::agent_cli::environment::mutation::scoped_inspector(base, kinds);
    let owner = actor.clone();
    reads
        .run_read(&actor, "native-plan", &request_id, move |control| {
            control.check()?;
            let result = service.plan_read(&owner, request, inspector, control.cancellation_flag());
            control.check()?;
            Ok(result)
        })
        .await
        .map_err(|message| AgentAssetMutationError {
            kind: AgentAssetMutationErrorKind::PreparationFailed,
            message,
            reason: None,
        })?
}

#[tauri::command]
pub(crate) fn apply_agent_asset(
    app: AppHandle,
    window: Window,
    request: AgentAssetApplyRequest,
) -> Result<AgentAssetOperation, AgentAssetMutationError> {
    let state = app.state::<AppState>();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| mutation_internal_error())?;
    let service = Arc::clone(&state.agent_asset_mutations);
    let operation = service.start(&actor, &request)?;
    let operation_id = operation.id.clone();
    tauri::async_runtime::spawn_blocking(move || service.run_operation(&operation_id));
    Ok(operation)
}

#[tauri::command]
pub(crate) fn get_agent_asset_operation(
    app: AppHandle,
    window: Window,
    operation_id: String,
) -> Result<AgentAssetOperation, AgentAssetMutationError> {
    let state = app.state::<AppState>();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| mutation_internal_error())?;
    state.agent_asset_mutations.operation(&actor, &operation_id)
}

#[tauri::command]
pub(crate) fn list_agent_asset_operations(
    app: AppHandle,
    window: Window,
) -> Result<Vec<AgentAssetOperation>, AgentAssetMutationError> {
    let state = app.state::<AppState>();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| mutation_internal_error())?;
    Ok(state.agent_asset_mutations.operations(&actor))
}

#[tauri::command]
pub(crate) fn cancel_agent_asset_operation(
    app: AppHandle,
    window: Window,
    operation_id: String,
) -> Result<AgentAssetOperation, AgentAssetMutationError> {
    let state = app.state::<AppState>();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| mutation_internal_error())?;
    state.agent_asset_mutations.cancel(&actor, &operation_id)
}

#[tauri::command]
pub(crate) async fn verify_agent_asset_operation(
    app: AppHandle,
    window: Window,
    operation_id: String,
) -> Result<AgentAssetOperation, AgentAssetMutationError> {
    let state = app.state::<AppState>();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| mutation_internal_error())?;
    let service = Arc::clone(&state.agent_asset_mutations);
    run_control(
        move || service.verify_operation(&actor, &operation_id),
        mutation_internal_error(),
    )
    .await
}

fn mutation_internal_error() -> AgentAssetMutationError {
    AgentAssetMutationError::new(AgentAssetMutationErrorKind::InternalFailure)
}

async fn run_control<T, E, F>(task: F, join_error: E) -> Result<T, E>
where
    T: Send + 'static,
    E: Send + 'static,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_| join_error)?
}
