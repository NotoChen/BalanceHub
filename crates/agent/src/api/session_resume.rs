//! 原生会话继续命令。

use crate::{
    api::blocking,
    models::{AgentSessionResumeOperation, AgentSessionResumeRequest},
    services::cli_sessions::resume::{self, AgentSessionResumeService},
};
use std::{sync::Arc, time::Duration};

fn service() -> Arc<AgentSessionResumeService> {
    Arc::clone(&crate::state::services().session_resumes)
}

pub(crate) async fn resume_agent_session(
    actor: &str,
    request: AgentSessionResumeRequest,
) -> Result<AgentSessionResumeOperation, String> {
    let client = actor.to_owned();
    let actor = crate::state::asset_actor(actor).map_err(|_| "无法识别当前窗口")?;
    let service = service();
    let (operation, created) = service.start(&actor, &client, request).await?;
    if created {
        resume::emit(&operation);
        let run_service = Arc::clone(&service);
        let run_id = operation.id.clone();
        balancehub_core::runtime::spawn_blocking(move || run_service.run(&run_id));
        let timeout_id = operation.id.clone();
        balancehub_core::runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(resume::OPERATION_TIMEOUT_SECONDS)).await;
            service.timeout(&timeout_id);
        });
    }
    Ok(operation)
}

pub(crate) async fn get_agent_session_resume_operation(
    actor: &str,
    operation_id: String,
) -> Result<AgentSessionResumeOperation, String> {
    let actor = crate::state::asset_actor(actor).map_err(|_| "无法识别当前窗口")?;
    let service = service();
    blocking("读取继续任务", move || {
        service.operation(&actor, &operation_id)
    })
    .await
}

pub(crate) async fn list_agent_session_resume_operations(
    actor: &str,
) -> Result<Vec<AgentSessionResumeOperation>, String> {
    let actor = crate::state::asset_actor(actor).map_err(|_| "无法识别当前窗口")?;
    let service = service();
    blocking("读取继续任务列表", move || {
        Ok(service.operations(&actor))
    })
    .await
}

pub(crate) async fn cancel_agent_session_resume_operation(
    actor: &str,
    operation_id: String,
) -> Result<AgentSessionResumeOperation, String> {
    let actor = crate::state::asset_actor(actor).map_err(|_| "无法识别当前窗口")?;
    let service = service();
    blocking("取消继续任务", move || {
        service.cancel(&actor, &operation_id)
    })
    .await
}
