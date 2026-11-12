//! 跨 Agent 交接命令。只读取已有会话引用并生成独立交接文件，
//! 不启动进程、不修改原生会话或 Agent 配置。

use crate::{
    api::blocking,
    models::{AgentHandoff, AgentHandoffRequest},
    services::cli_sessions::workbench::{self, AgentSessionActor},
};
use std::sync::atomic::Ordering;

pub(crate) async fn prepare_agent_handoff(
    actor: String,
    request: AgentHandoffRequest,
) -> Result<AgentHandoff, String> {
    let budget = workbench::begin_request(
        AgentSessionActor { client: &actor },
        &request.consumer_id,
        request.request_id,
        workbench::HANDOFF_TIMEOUT,
    )?;
    let cancellation = budget.cancelled.clone();
    let prepared = tokio::time::timeout(
        workbench::HANDOFF_TIMEOUT,
        blocking("生成会话交接", {
            let actor = actor.clone();
            move || {
                workbench::prepare_handoff(AgentSessionActor { client: &actor }, &request, &budget)
            }
        }),
    )
    .await;
    cancellation.store(true, Ordering::Release);
    match prepared {
        Ok(result) => result,
        Err(_) => Err("生成会话交接超时，后台读取已取消".into()),
    }
}
