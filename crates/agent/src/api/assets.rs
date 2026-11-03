//! 资产盘点、安全来源访问与确认后的操作命令。

use crate::{
    models::*,
    services::agent_cli::environment::{
        self,
        access_registry::{AgentAssetAccessRequest, AgentAssetAccessTargetKind},
    },
};
use std::{path::Path, sync::Arc};

pub(crate) async fn get_agent_environment_inventory(
    client: &str,
    workspace: Option<String>,
) -> Result<AgentEnvironmentInventory, String> {
    let services = crate::state::services();
    let settings = crate::state::app_data().settings;
    let actor = crate::state::asset_actor(client).map_err(|_| "无法创建资产访问会话".to_owned())?;
    let registry = Arc::clone(&services.asset_access);
    let normalized =
        environment::normalize_optional_workspace(workspace.as_deref().map(Path::new))?;
    let ticket = registry
        .begin_publish(&actor, normalized.as_deref())
        .map_err(|error| error.to_string())?;
    crate::api::blocking("盘点 Agent 环境", move || {
        let mut build =
            environment::inventory_with_access(&settings, workspace.as_deref().map(Path::new))?;
        registry
            .publish_ticket(ticket, &mut build.inventory, build.access_evidence)
            .map_err(|error| error.to_string())?;
        Ok(build.inventory)
    })
    .await
}

pub(crate) async fn read_agent_environment_asset(
    client: &str,
    request: AgentAssetReadRequest,
) -> Result<AgentAssetReadResult, AgentAssetAccessError> {
    read_target(client, request, AgentAssetAccessTargetKind::Asset).await
}

pub(crate) async fn read_agent_environment_source(
    client: &str,
    request: AgentAssetReadRequest,
) -> Result<AgentAssetReadResult, AgentAssetAccessError> {
    read_target(client, request, AgentAssetAccessTargetKind::Source).await
}

async fn read_target(
    client: &str,
    request: AgentAssetReadRequest,
    kind: AgentAssetAccessTargetKind,
) -> Result<AgentAssetReadResult, AgentAssetAccessError> {
    let actor = crate::state::asset_actor(client)
        .map_err(|_| AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessUnavailable))?;
    let registry = Arc::clone(&crate::state::services().asset_access);
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

pub(crate) async fn open_agent_environment_asset(
    client: &str,
    request: AgentAssetOpenRequest,
) -> Result<(), AgentAssetAccessError> {
    open_target(client, request, AgentAssetAccessTargetKind::Asset).await
}

pub(crate) async fn open_agent_environment_source(
    client: &str,
    request: AgentAssetOpenRequest,
) -> Result<(), AgentAssetAccessError> {
    open_target(client, request, AgentAssetAccessTargetKind::Source).await
}

async fn open_target(
    client: &str,
    request: AgentAssetOpenRequest,
    kind: AgentAssetAccessTargetKind,
) -> Result<(), AgentAssetAccessError> {
    let actor = crate::state::asset_actor(client)
        .map_err(|_| AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessUnavailable))?;
    let registry = Arc::clone(&crate::state::services().asset_access);
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
            let open = |path: &Path| {
                let path = crate::platform::paths::compatible_path(path);
                match request.target {
                    AgentAssetOpenTarget::Asset => {
                        crate::host::open_path(&path).map_err(|_| "系统打开失败".to_owned())
                    }
                    AgentAssetOpenTarget::Reveal => {
                        crate::host::reveal_path(&path).map_err(|_| "系统定位失败".to_owned())
                    }
                }
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

pub(crate) async fn plan_agent_asset(
    client: &str,
    request: AgentAssetPlanRequest,
    request_id: String,
    agent_kind: AgentCliKind,
) -> Result<AgentAssetPlan, AgentAssetMutationError> {
    let services = crate::state::services();
    let actor = crate::state::asset_actor(client).map_err(|_| mutation_internal_error())?;
    let service = Arc::clone(&services.asset_mutations);
    let reads = services.catalog().map_err(|_| mutation_internal_error())?;
    let base = super::catalog::inspector(request.workspace.clone())
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

pub(crate) fn apply_agent_asset(
    client: &str,
    request: AgentAssetApplyRequest,
) -> Result<AgentAssetOperation, AgentAssetMutationError> {
    let services = crate::state::services();
    let actor = crate::state::asset_actor(client).map_err(|_| mutation_internal_error())?;
    let service = Arc::clone(&services.asset_mutations);
    let operation = service.start(&actor, &request)?;
    let operation_id = operation.id.clone();
    balancehub_core::runtime::spawn_blocking(move || service.run_operation(&operation_id));
    Ok(operation)
}

pub(crate) fn get_agent_asset_operation(
    client: &str,
    operation_id: String,
) -> Result<AgentAssetOperation, AgentAssetMutationError> {
    let actor = crate::state::asset_actor(client).map_err(|_| mutation_internal_error())?;
    crate::state::services()
        .asset_mutations
        .operation(&actor, &operation_id)
}

pub(crate) fn list_agent_asset_operations(
    client: &str,
) -> Result<Vec<AgentAssetOperation>, AgentAssetMutationError> {
    let actor = crate::state::asset_actor(client).map_err(|_| mutation_internal_error())?;
    Ok(crate::state::services().asset_mutations.operations(&actor))
}

pub(crate) fn cancel_agent_asset_operation(
    client: &str,
    operation_id: String,
) -> Result<AgentAssetOperation, AgentAssetMutationError> {
    let actor = crate::state::asset_actor(client).map_err(|_| mutation_internal_error())?;
    crate::state::services()
        .asset_mutations
        .cancel(&actor, &operation_id)
}

pub(crate) async fn verify_agent_asset_operation(
    client: &str,
    operation_id: String,
) -> Result<AgentAssetOperation, AgentAssetMutationError> {
    let actor = crate::state::asset_actor(client).map_err(|_| mutation_internal_error())?;
    let service = Arc::clone(&crate::state::services().asset_mutations);
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
    balancehub_core::runtime::spawn_blocking(task)
        .await
        .map_err(|_| join_error)?
}
