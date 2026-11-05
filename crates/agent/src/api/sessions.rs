//! 原生会话检索命令。调用方身份由宿主传入，桌面用窗口标签，浏览器版用客户端标识。

use crate::{
    api::blocking,
    models::{
        AgentSessionCancelRequest, AgentSessionCountRequest, AgentSessionCounts,
        AgentSessionDetail, AgentSessionDetailRequest, AgentSessionMessageContent,
        AgentSessionMessageRequest, AgentSessionPage, AgentSessionQuery, AgentSessionScope,
        AgentSessionSearchRequest, AgentSessionSearchResult,
    },
    services::cli_sessions::workbench::{self, AgentSessionActor},
};
use std::sync::atomic::Ordering;

pub(crate) async fn count_agent_sessions(
    actor: String,
    request: AgentSessionCountRequest,
) -> Result<AgentSessionCounts, String> {
    crate::services::wsl::refresh_background();
    let budget = workbench::begin_request(
        AgentSessionActor { client: &actor },
        &request.consumer_id,
        request.request_id,
        workbench::COUNT_TIMEOUT,
    )?;
    let cancellation = budget.cancelled.clone();
    let result = tokio::time::timeout(
        workbench::COUNT_TIMEOUT,
        blocking("统计原生会话", move || {
            workbench::count(AgentSessionActor { client: &actor }, &request, &budget)
        }),
    )
    .await;
    cancellation.store(true, Ordering::Release);
    result.map_err(|_| "会话统计超时，后台读取已取消".to_owned())?
}

pub(crate) async fn get_agent_session_scope(
    actor: String,
    explicit_workdir: Option<String>,
) -> Result<AgentSessionScope, String> {
    crate::services::wsl::refresh_background();
    blocking("读取原生会话目录集合", move || {
        workbench::get_scope(
            AgentSessionActor { client: &actor },
            explicit_workdir.as_deref(),
        )
    })
    .await
}

pub(crate) async fn query_agent_sessions(
    actor: String,
    request: AgentSessionQuery,
) -> Result<AgentSessionPage, String> {
    let budget = workbench::begin_request(
        AgentSessionActor { client: &actor },
        &request.consumer_id,
        request.request_id,
        workbench::QUERY_TIMEOUT,
    )?;
    let cancellation = budget.cancelled.clone();
    match tokio::time::timeout(
        workbench::QUERY_TIMEOUT,
        blocking("检索原生会话", move || {
            workbench::query(AgentSessionActor { client: &actor }, &request, &budget)
        }),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            cancellation.store(true, Ordering::Release);
            Err("会话查询超时，后台读取已取消".into())
        }
    }
}

pub(crate) async fn get_agent_session_detail(
    actor: String,
    request: AgentSessionDetailRequest,
) -> Result<AgentSessionDetail, String> {
    let budget = workbench::begin_request(
        AgentSessionActor { client: &actor },
        &request.consumer_id,
        request.request_id,
        workbench::DETAIL_TIMEOUT,
    )?;
    let cancellation = budget.cancelled.clone();
    match tokio::time::timeout(
        workbench::DETAIL_TIMEOUT,
        blocking("读取原生会话详情", move || {
            workbench::detail(AgentSessionActor { client: &actor }, &request, &budget)
        }),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            cancellation.store(true, Ordering::Release);
            Err("会话详情读取超时，后台读取已取消".into())
        }
    }
}

pub(crate) async fn search_agent_session(
    actor: String,
    request: AgentSessionSearchRequest,
) -> Result<AgentSessionSearchResult, String> {
    let budget = workbench::begin_request(
        AgentSessionActor { client: &actor },
        &request.consumer_id,
        request.request_id,
        workbench::SEARCH_TIMEOUT,
    )?;
    let cancellation = budget.cancelled.clone();
    match tokio::time::timeout(
        workbench::SEARCH_TIMEOUT,
        blocking("搜索原生会话片段", move || {
            workbench::search(AgentSessionActor { client: &actor }, &request, &budget)
        }),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            cancellation.store(true, Ordering::Release);
            Err("整段会话搜索超时，后台读取已取消".into())
        }
    }
}

pub(crate) async fn get_agent_session_message_content(
    actor: String,
    request: AgentSessionMessageRequest,
) -> Result<AgentSessionMessageContent, String> {
    let budget = workbench::begin_request(
        AgentSessionActor { client: &actor },
        &request.consumer_id,
        request.request_id,
        workbench::DETAIL_TIMEOUT,
    )?;
    let cancellation = budget.cancelled.clone();
    let result = tokio::time::timeout(
        workbench::DETAIL_TIMEOUT,
        blocking("展开会话消息", move || {
            workbench::message_content(AgentSessionActor { client: &actor }, &request, &budget)
        }),
    )
    .await;
    cancellation.store(true, Ordering::Release);
    result.map_err(|_| "消息读取超时，后台读取已取消".to_owned())?
}

pub(crate) fn cancel_agent_session_query(actor: &str, request: AgentSessionCancelRequest) {
    workbench::cancel(
        AgentSessionActor { client: actor },
        &request.consumer_id,
        request.request_id,
    );
}
