//! 配置来源读取与编辑命令；提交后立即返回，文件工作留在后台。

use crate::{models::*, services::agent_cli::configuration::ConfigurationService};
use std::{sync::Arc, time::Duration};

pub(super) fn actor_and_service(
    client: &str,
) -> Result<(String, Arc<ConfigurationService>), AgentConfigurationError> {
    let services = crate::state::services();
    let actor = crate::state::asset_actor(client).map_err(|_| internal())?;
    Ok((actor, services.configuration()?))
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
        balancehub_core::runtime::spawn_blocking(task),
    )
    .await
    .map_err(|_| AgentConfigurationError::new(AgentConfigurationErrorKind::Timeout))?
    .map_err(|_| internal())?
}

pub(crate) async fn list_agent_configuration_sources(
    actor: &str,
    request: AgentConfigurationListRequest,
) -> Result<AgentConfigurationSnapshot, AgentConfigurationError> {
    let (actor, service) = actor_and_service(actor)?;
    let settings = crate::state::app_data().settings;
    run_configuration(move || service.list_sources(&actor, request, Some(&settings))).await
}

pub(crate) async fn read_agent_configuration_source(
    actor: &str,
    request: AgentConfigurationSourceRequest,
) -> Result<AgentConfigurationReadResult, AgentConfigurationError> {
    let (actor, service) = actor_and_service(actor)?;
    run_configuration(move || service.read_source(&actor, request)).await
}

pub(crate) async fn begin_agent_configuration_edit(
    actor: &str,
    request: AgentConfigurationSourceRequest,
) -> Result<AgentConfigurationEdit, AgentConfigurationError> {
    let (actor, service) = actor_and_service(actor)?;
    run_configuration(move || service.begin_edit(&actor, request)).await
}

pub(crate) async fn read_agent_configuration_form(
    actor: &str,
    request: AgentConfigurationFormRequest,
) -> Result<AgentConfigurationForm, AgentConfigurationError> {
    let (actor, service) = actor_and_service(actor)?;
    let mut settings = crate::state::app_data().settings;
    run_configuration(move || {
        // Reuse the installed CLI summary. Opening a form must not perform
        // installation discovery or shell startup again.
        if let Some(cached) = crate::services::cli_runtime::CliRuntimeService::new().cached_tools()
        {
            for tool in cached.tools.into_iter().filter(|tool| tool.available) {
                if settings.agent_cli_path(tool.descriptor.kind).is_empty() {
                    settings.set_agent_cli_path(tool.descriptor.kind, tool.path);
                }
            }
        }
        service.configuration_form(&actor, request, &settings)
    })
    .await
}

pub(crate) async fn begin_agent_resource_edit(
    actor: &str,
    request: AgentResourceEditRequest,
    request_id: String,
) -> Result<AgentConfigurationEdit, AgentConfigurationError> {
    let (actor, service) = actor_and_service(actor)?;
    let catalog = crate::state::services()
        .catalog()
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
    let inspector = super::catalog::inspector(request.workspace)
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
                let current = super::catalog::refresh_read_inventory(inspector.as_ref(), kind)?;
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

pub(crate) async fn plan_agent_configuration_save(
    actor: &str,
    request: AgentConfigurationSaveRequest,
) -> Result<AgentConfigurationPlan, AgentConfigurationError> {
    let (actor, service) = actor_and_service(actor)?;
    let ticket = service.begin_plan_save(&actor, request)?;
    run_configuration(move || service.plan_save_with_ticket(ticket)).await
}

pub(crate) fn apply_agent_configuration_plan(
    actor: &str,
    request: AgentConfigurationApplyRequest,
) -> Result<AgentConfigurationOperation, AgentConfigurationError> {
    let (actor, service) = actor_and_service(actor)?;
    let (operation, created) = service.start(&actor, request)?;
    if created {
        let id = operation.id.clone();
        balancehub_core::runtime::spawn_blocking(move || {
            service.run_operation(&id, |operation| {
                crate::emit("agent-configuration-operation", operation);
            });
        });
    }
    Ok(operation)
}

pub(crate) fn discard_agent_configuration_edit(
    actor: &str,
    edit_id: String,
) -> Result<(), AgentConfigurationError> {
    let (actor, service) = actor_and_service(actor)?;
    service.discard_edit(&actor, &edit_id)
}

pub(crate) async fn get_agent_configuration_operation(
    actor: &str,
    operation_id: String,
) -> Result<AgentConfigurationOperation, AgentConfigurationError> {
    let (_, service) = actor_and_service(actor)?;
    run_configuration(move || service.get_operation(&operation_id)).await
}

pub(crate) async fn list_agent_configuration_operations(
    actor: &str,
) -> Result<Vec<AgentConfigurationOperation>, AgentConfigurationError> {
    let (_, service) = actor_and_service(actor)?;
    run_configuration(move || service.list_operations()).await
}

pub(crate) fn cancel_agent_configuration_operation(
    actor: &str,
    operation_id: String,
) -> Result<AgentConfigurationOperation, AgentConfigurationError> {
    let (actor, service) = actor_and_service(actor)?;
    service.cancel_operation(&actor, &operation_id)
}

pub(crate) async fn open_agent_configuration_source(
    actor: &str,
    request: AgentConfigurationOpenRequest,
) -> Result<(), AgentConfigurationError> {
    let (actor, service) = actor_and_service(actor)?;
    run_configuration(move || {
        let target = request.target;
        service.open_source(&actor, request, |path| {
            let path = crate::platform::paths::compatible_path(path);
            match target {
                AgentAssetOpenTarget::Asset => {
                    crate::host::open_path(&path).map_err(|_| "无法打开来源".to_owned())
                }
                AgentAssetOpenTarget::Reveal => {
                    crate::host::reveal_path(&path).map_err(|_| "无法定位来源".to_owned())
                }
            }
        })
    })
    .await
}

fn internal() -> AgentConfigurationError {
    AgentConfigurationError::new(AgentConfigurationErrorKind::InternalFailure)
}
