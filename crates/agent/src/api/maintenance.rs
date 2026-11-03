use crate::models::*;

pub(crate) async fn get_agent_maintenance_change(
    client: &str,
    plan_token: String,
    index: usize,
) -> Result<AgentAssetPlanChange, String> {
    let (service, actor) = session(client)?;
    crate::api::blocking("读取变更文件", move || {
        service.change(&actor, &plan_token, index)
    })
    .await
}
pub(crate) fn discard_agent_maintenance_plan(
    client: &str,
    plan_token: String,
) -> Result<(), String> {
    let (service, actor) = session(client)?;
    service.discard(&actor, &plan_token)
}

pub(crate) async fn list_agent_history(
    offset: usize,
    limit: usize,
) -> Result<Vec<AgentHistoryEntry>, String> {
    crate::api::blocking("读取配置历史", move || {
        crate::services::agent_cli::config_support::native_history::store()?.list(offset, limit)
    })
    .await
}

pub(crate) async fn plan_agent_history_restore(
    client: &str,
    history_id: String,
    selected_paths: Option<Vec<String>>,
) -> Result<AgentMaintenancePlan, String> {
    let (service, actor) = session(client)?;
    crate::api::blocking("预览历史恢复", move || {
        crate::services::agent_cli::maintenance::history::plan(
            &service,
            &actor,
            &history_id,
            selected_paths,
        )
    })
    .await
}

pub(crate) async fn list_agent_shared_versions(
    asset_id: String,
) -> Result<Vec<AgentSharedVersion>, String> {
    let catalog = crate::state::services().catalog()?;
    crate::api::blocking("读取共享版本", move || {
        catalog.definition_versions(&asset_id)
    })
    .await
}
pub(crate) async fn plan_agent_shared_version(
    client: &str,
    asset_id: String,
    version: u64,
) -> Result<AgentMaintenancePlan, String> {
    let (service, actor) = session(client)?;
    let catalog = crate::state::services().catalog()?;
    crate::api::blocking("比较共享版本", move || {
        catalog.plan_definition_version(&actor, &service, &asset_id, version)
    })
    .await
}

pub(crate) async fn get_agent_skill_origin(
    client: &str,
    asset_id: String,
    workspace: Option<String>,
) -> Result<Option<AgentSkillOrigin>, String> {
    let services = crate::state::services();
    let catalog = services.catalog()?;
    let actor = crate::state::asset_actor(client).map_err(|_| "无法确认操作窗口")?;
    let publication = catalog.published_catalog(&actor, workspace.as_deref()).ok();
    crate::api::blocking("读取 Skill 来源", move || {
        catalog.skill_origin(&asset_id, publication.as_deref())
    })
    .await
}
pub(crate) async fn save_agent_skill_origin(
    asset_id: String,
    version: u64,
    origin: Option<AgentSkillOrigin>,
) -> Result<Option<AgentSkillOrigin>, String> {
    let catalog = crate::state::services().catalog()?;
    crate::api::blocking("保存 Skill 来源", move || {
        catalog.save_skill_origin(&asset_id, version, origin)
    })
    .await
}
pub(crate) async fn check_agent_skill_update(
    client: &str,
    asset_id: String,
    request_id: String,
) -> Result<AgentSkillUpdateCheck, String> {
    let (_, actor) = session(client)?;
    let services = crate::state::services();
    let catalog = services.catalog()?;
    let settings = crate::state::app_data().settings;
    let worker = std::sync::Arc::clone(&catalog);
    catalog
        .run_network_read(
            &actor,
            "skill-update-check",
            &request_id,
            move |control| async move {
                worker
                    .check_skill_update(&asset_id, &settings, &control)
                    .await
            },
        )
        .await
}
pub(crate) async fn plan_agent_skill_update(
    client: &str,
    asset_id: String,
    request_id: String,
) -> Result<AgentMaintenancePlan, String> {
    let (service, actor) = session(client)?;
    let services = crate::state::services();
    let catalog = services.catalog()?;
    let settings = crate::state::app_data().settings;
    let worker = std::sync::Arc::clone(&catalog);
    let owner = actor.clone();
    catalog
        .run_network_read(
            &actor,
            "skill-update-plan",
            &request_id,
            move |control| async move {
                worker
                    .plan_skill_update(&owner, &service, &asset_id, &settings, &control)
                    .await
            },
        )
        .await
}

pub(crate) async fn get_agent_batch_options(
    client: &str,
    asset_ids: Vec<String>,
    action: AgentCatalogAction,
    workspace: Option<String>,
    request_id: String,
) -> Result<Vec<AgentBatchOption>, String> {
    let (_, actor) = session(client)?;
    let catalog = crate::state::services().catalog()?;
    let publication = catalog.published_catalog(&actor, workspace.as_deref())?;
    let worker = std::sync::Arc::clone(&catalog);
    catalog
        .run_read(&actor, "batch-options", &request_id, move |control| {
            worker.batch_options(asset_ids, action, &publication, &control)
        })
        .await
}
pub(crate) async fn plan_agent_batch(
    client: &str,
    requests: Vec<AgentCatalogPlanRequest>,
    workspace: Option<String>,
    request_id: String,
) -> Result<AgentMaintenancePlan, String> {
    let (service, actor) = session(client)?;
    let catalog = crate::state::services().catalog()?;
    let publication = catalog.published_catalog(&actor, workspace.as_deref())?;
    let mut kinds = std::collections::BTreeSet::new();
    for request in &requests {
        kinds.extend(crate::services::agent_cli::catalog::plan_scope::agents(
            &publication,
            request,
        )?);
    }
    let inspector = crate::services::agent_cli::environment::mutation::scoped_inspector(
        super::catalog::inspector(workspace)?,
        kinds,
    );
    let worker = std::sync::Arc::clone(&catalog);
    let owner = actor.clone();
    catalog
        .run_read(&actor, "batch-plan", &request_id, move |control| {
            worker.plan_batch(
                &owner,
                &service,
                requests,
                inspector,
                &publication,
                &control,
            )
        })
        .await
}

pub(crate) fn apply_agent_maintenance(
    client: &str,
    request: AgentMaintenanceApplyRequest,
) -> Result<AgentMaintenanceOperation, String> {
    let (service, actor) = session(client)?;
    let operation = service.start(&actor, request)?;
    let id = operation.id.clone();
    balancehub_core::runtime::spawn_blocking(move || service.run(&id));
    Ok(operation)
}

pub(crate) async fn get_agent_mcp_tool_rules(
    client: &str,
    request: AgentMcpToolsRequest,
    request_id: String,
) -> Result<AgentMcpToolRules, String> {
    let (_, actor) = session(client)?;
    let catalog = crate::state::services().catalog()?;
    let publication = catalog.published_catalog(&actor, request.workspace.as_deref())?;
    let kind =
        crate::services::agent_cli::catalog::tool_policy::selected_agent(&publication, &request)?;
    let inspector = crate::services::agent_cli::environment::mutation::scoped_inspector(
        super::catalog::inspector(request.workspace.clone())?,
        std::collections::BTreeSet::from([kind]),
    );
    let worker = std::sync::Arc::clone(&catalog);
    catalog
        .run_read(&actor, "mcp-tools", &request_id, move |control| {
            worker.mcp_tool_rules(&request, &publication, inspector.as_ref(), &control)
        })
        .await
}
pub(crate) async fn plan_agent_mcp_tools(
    client: &str,
    request: AgentMcpToolsRequest,
    disabled: Vec<String>,
    request_id: String,
) -> Result<AgentMaintenancePlan, String> {
    let (service, actor) = session(client)?;
    let catalog = crate::state::services().catalog()?;
    let publication = catalog.published_catalog(&actor, request.workspace.as_deref())?;
    let kind =
        crate::services::agent_cli::catalog::tool_policy::selected_agent(&publication, &request)?;
    let inspector = crate::services::agent_cli::environment::mutation::scoped_inspector(
        super::catalog::inspector(request.workspace.clone())?,
        std::collections::BTreeSet::from([kind]),
    );
    let worker = std::sync::Arc::clone(&catalog);
    let owner = actor.clone();
    catalog
        .run_read(&actor, "mcp-tool-plan", &request_id, move |control| {
            worker.plan_mcp_tools(
                &owner,
                &service,
                crate::services::agent_cli::catalog::tool_policy::McpToolSelection {
                    request,
                    disabled,
                },
                &publication,
                inspector,
                &control,
            )
        })
        .await
}

pub(crate) async fn plan_agent_session_relocation(
    client: &str,
    request: AgentSessionRelocationRequest,
) -> Result<AgentMaintenancePlan, String> {
    use crate::services::cli_sessions::workbench::{self, AgentSessionActor};
    let (service, actor) = session(client)?;
    let label = client.to_owned();
    let budget = workbench::begin_request(
        AgentSessionActor { client: &label },
        &request.consumer_id,
        request.request_id,
        workbench::QUERY_TIMEOUT,
    )?;
    let cancellation = std::sync::Arc::clone(&budget.cancelled);
    let result = tokio::time::timeout(
        workbench::QUERY_TIMEOUT,
        crate::api::blocking("预览项目会话迁移", move || {
            crate::services::cli_sessions::relocation::plan(
                &label, &actor, &service, &request, &budget,
            )
        }),
    )
    .await;
    cancellation.store(true, std::sync::atomic::Ordering::Release);
    result.map_err(|_| "会话迁移预览超时，读取已取消".to_owned())?
}

pub(crate) fn get_agent_maintenance_operation(
    client: &str,
    operation_id: String,
) -> Result<AgentMaintenanceOperation, String> {
    let (service, actor) = session(client)?;
    service.get(&actor, &operation_id)
}
pub(crate) fn list_agent_maintenance_operations(
    client: &str,
) -> Result<Vec<AgentMaintenanceOperation>, String> {
    let (service, actor) = session(client)?;
    Ok(service.list(&actor))
}
pub(crate) fn cancel_agent_maintenance_operation(
    client: &str,
    operation_id: String,
) -> Result<AgentMaintenanceOperation, String> {
    let (service, actor) = session(client)?;
    service.cancel(&actor, &operation_id)
}
pub(super) fn session(
    client: &str,
) -> Result<
    (
        std::sync::Arc<crate::services::agent_cli::maintenance::MaintenanceService>,
        String,
    ),
    String,
> {
    let services = crate::state::services();
    let actor = crate::state::asset_actor(client).map_err(|_| "无法确认操作窗口")?;
    Ok((std::sync::Arc::clone(&services.maintenance), actor))
}
