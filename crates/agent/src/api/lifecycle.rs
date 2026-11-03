//! 显式 CLI 生命周期命令；提交后的工作不等窗口。

use crate::{api::blocking, models::*, services::cli_paths::user_home};

pub(crate) async fn set_agent_automatic_update(
    request: SetAgentAutomaticUpdateRequest,
) -> Result<AgentLifecycleCatalog, AgentLifecycleError> {
    if crate::state::core_state().load_error().is_some() {
        let mut error = internal();
        error.message = "应用配置处于保护状态，自动更新设置暂时不可修改".into();
        return Err(error);
    }
    let service = crate::state::services().lifecycle();
    let settings = crate::state::app_data().settings;
    let home = user_home()
        .and_then(|home| home.canonicalize().ok())
        .ok_or_else(internal)?;
    tokio::time::timeout(
        std::time::Duration::from_secs(65),
        service.set_automatic(settings, home, request),
    )
    .await
    .map_err(|_| {
        let mut error = internal();
        error.message = "保存自动更新设置超时，请刷新安装状态确认结果".into();
        error
    })?
}

pub(crate) async fn get_agent_lifecycle_catalog(
    request: AgentLifecycleCatalogRequest,
) -> Result<AgentLifecycleCatalog, AgentLifecycleError> {
    let service = crate::state::services().lifecycle();
    let settings = crate::state::app_data().settings;
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

pub(crate) async fn plan_agent_lifecycle(
    actor: &str,
    request: AgentLifecyclePlanRequest,
) -> Result<AgentLifecyclePlan, AgentLifecycleError> {
    let actor = crate::state::asset_actor(actor).map_err(|_| internal())?;
    let service = crate::state::services().lifecycle();
    let settings = crate::state::app_data().settings;
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

pub(crate) async fn apply_agent_lifecycle(
    actor: &str,
    request: AgentLifecycleApplyRequest,
) -> Result<AgentLifecycleOperation, AgentLifecycleError> {
    let actor = crate::state::asset_actor(actor).map_err(|_| internal())?;
    let service = crate::state::services().lifecycle();
    let starter = std::sync::Arc::clone(&service);
    let operation = blocking("启动 CLI 升级", move || {
        starter
            .start(&actor, &request)
            .map_err(|error| error.message)
    })
    .await
    .map_err(failed)?;
    let id = operation.id.clone();
    balancehub_core::runtime::spawn_blocking(move || service.run_operation(&id));
    Ok(operation)
}

pub(crate) async fn get_agent_lifecycle_operation(
    actor: &str,
    operation_id: String,
) -> Result<AgentLifecycleOperation, AgentLifecycleError> {
    let actor = crate::state::asset_actor(actor).map_err(|_| internal())?;
    let service = crate::state::services().lifecycle();
    let settings = crate::state::app_data().settings;
    blocking("读取 CLI 升级任务", move || {
        let mut operation = service
            .operation(&actor, &operation_id)
            .map_err(|error| error.message)?;
        crate::services::agent_cli::lifecycle::annotate_launch(&mut operation, &settings);
        service
            .publish_launch(&actor, operation)
            .map_err(|error| error.message)
    })
    .await
    .map_err(failed)
}

pub(crate) async fn list_agent_lifecycle_operations(
    actor: &str,
) -> Result<Vec<AgentLifecycleOperation>, AgentLifecycleError> {
    let actor = crate::state::asset_actor(actor).map_err(|_| internal())?;
    let service = crate::state::services().lifecycle();
    let settings = crate::state::app_data().settings;
    let home = user_home()
        .and_then(|home| home.canonicalize().ok())
        .ok_or_else(internal)?;
    blocking("读取 CLI 升级任务", move || {
        service
            .reconcile_history(settings.clone(), home)
            .map_err(|error| error.message)?;
        let mut operations = service.operations(&actor).map_err(|error| error.message)?;
        crate::services::agent_cli::lifecycle::annotate_launches(&mut operations, &settings);
        operations
            .into_iter()
            .map(|operation| service.publish_launch(&actor, operation))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.message)
    })
    .await
    .map_err(failed)
}

pub(crate) async fn cancel_agent_lifecycle_operation(
    actor: &str,
    operation_id: String,
) -> Result<AgentLifecycleOperation, AgentLifecycleError> {
    let actor = crate::state::asset_actor(actor).map_err(|_| internal())?;
    let service = crate::state::services().lifecycle();
    blocking("取消 CLI 升级", move || {
        service
            .cancel(&actor, &operation_id)
            .map_err(|error| error.message)
    })
    .await
    .map_err(failed)
}

fn internal() -> AgentLifecycleError {
    AgentLifecycleError::new(AgentLifecycleErrorKind::InternalFailure)
}

fn failed(message: String) -> AgentLifecycleError {
    let mut error = internal();
    error.message = message;
    error
}
