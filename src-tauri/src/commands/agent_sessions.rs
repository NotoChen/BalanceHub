use super::run_blocking;
use crate::{
    models::{
        AgentSessionCancelRequest, AgentSessionCountRequest, AgentSessionCounts,
        AgentSessionDetail, AgentSessionDetailRequest, AgentSessionPage, AgentSessionQuery,
        AgentSessionScope,
    },
    services::cli_sessions::workbench::{self, AgentSessionActor},
};
use tauri::{AppHandle, WebviewWindow};

#[tauri::command]
pub(crate) async fn count_agent_sessions(
    app: AppHandle,
    window: WebviewWindow,
    request: AgentSessionCountRequest,
) -> Result<AgentSessionCounts, String> {
    let label = window.label().to_string();
    let budget = workbench::begin_request(
        AgentSessionActor {
            app: &app,
            window_label: &label,
        },
        &request.consumer_id,
        request.request_id,
        workbench::COUNT_TIMEOUT,
    )?;
    let cancellation = budget.cancelled.clone();
    let result = tokio::time::timeout(
        workbench::COUNT_TIMEOUT,
        run_blocking("统计原生会话", move || {
            workbench::count(
                AgentSessionActor {
                    app: &app,
                    window_label: &label,
                },
                &request,
                &budget,
            )
        }),
    )
    .await;
    cancellation.store(true, std::sync::atomic::Ordering::Release);
    result.map_err(|_| "会话统计超时，后台读取已取消".to_owned())?
}

#[tauri::command]
pub(crate) async fn get_agent_session_scope(
    app: AppHandle,
    window: WebviewWindow,
    explicit_workdir: Option<String>,
) -> Result<AgentSessionScope, String> {
    let label = window.label().to_string();
    run_blocking("读取原生会话目录集合", move || {
        workbench::get_scope(
            AgentSessionActor {
                app: &app,
                window_label: &label,
            },
            explicit_workdir.as_deref(),
        )
    })
    .await
}
#[tauri::command]
pub(crate) async fn query_agent_sessions(
    app: AppHandle,
    window: WebviewWindow,
    request: AgentSessionQuery,
) -> Result<AgentSessionPage, String> {
    let label = window.label().to_string();
    let budget = workbench::begin_request(
        AgentSessionActor {
            app: &app,
            window_label: &label,
        },
        &request.consumer_id,
        request.request_id,
        workbench::QUERY_TIMEOUT,
    )?;
    let cancellation = budget.cancelled.clone();
    match tokio::time::timeout(
        workbench::QUERY_TIMEOUT,
        run_blocking("检索原生会话", move || {
            workbench::query(
                AgentSessionActor {
                    app: &app,
                    window_label: &label,
                },
                &request,
                &budget,
            )
        }),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            cancellation.store(true, std::sync::atomic::Ordering::Release);
            Err("会话查询超时，后台读取已取消".into())
        }
    }
}
#[tauri::command]
pub(crate) async fn get_agent_session_detail(
    app: AppHandle,
    window: WebviewWindow,
    request: AgentSessionDetailRequest,
) -> Result<AgentSessionDetail, String> {
    let label = window.label().to_string();
    let budget = workbench::begin_request(
        AgentSessionActor {
            app: &app,
            window_label: &label,
        },
        &request.consumer_id,
        request.request_id,
        workbench::DETAIL_TIMEOUT,
    )?;
    let cancellation = budget.cancelled.clone();
    match tokio::time::timeout(
        workbench::DETAIL_TIMEOUT,
        run_blocking("读取原生会话详情", move || {
            workbench::detail(
                AgentSessionActor {
                    app: &app,
                    window_label: &label,
                },
                &request,
                &budget,
            )
        }),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            cancellation.store(true, std::sync::atomic::Ordering::Release);
            Err("会话详情读取超时，后台读取已取消".into())
        }
    }
}
#[tauri::command]
pub(crate) fn cancel_agent_session_query(
    app: AppHandle,
    window: WebviewWindow,
    request: AgentSessionCancelRequest,
) {
    workbench::cancel(
        AgentSessionActor {
            app: &app,
            window_label: window.label(),
        },
        &request.consumer_id,
        request.request_id,
    );
}
