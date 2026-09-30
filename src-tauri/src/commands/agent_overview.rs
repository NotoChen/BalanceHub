use crate::{
    models::{AgentCliKind, AgentOverviewSnapshot},
    services::agent_cli::overview::OverviewRefresh,
    state::AppState,
};
use std::{path::PathBuf, time::Duration};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub(crate) async fn get_cached_agent_overview(
    app: AppHandle,
    workspace: Option<String>,
) -> Result<Vec<AgentOverviewSnapshot>, String> {
    let service = app.state::<AppState>().agent_overview(&app)?;
    super::run_blocking("读取 Agent 摘要", move || {
        Ok(service.cached(workspace.as_deref().map(std::path::Path::new)))
    })
    .await
}

#[tauri::command]
pub(crate) async fn refresh_agent_overview(
    app: AppHandle,
    agent_kind: AgentCliKind,
    workspace: Option<String>,
    force: bool,
) -> Result<AgentOverviewSnapshot, String> {
    let state = app.state::<AppState>();
    let service = state.agent_overview(&app)?;
    let configuration = state
        .agent_configuration(&app)
        .map_err(|_| "无法读取配置来源")?;
    let settings = state
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    let workspace = workspace.filter(|path| !path.is_empty()).map(PathBuf::from);
    let gate = service.gate(agent_kind, workspace.as_deref());
    let permit = tokio::time::timeout(Duration::from_secs(60), gate.lock_owned())
        .await
        .map_err(|_| "等待 Agent 刷新超时")?;
    super::run_blocking("核对 Agent 变化", move || {
        let _permit = permit;
        service.refresh(OverviewRefresh {
            kind: agent_kind,
            workspace: workspace.as_deref(),
            settings: &settings,
            configuration: &configuration,
            force,
        })
    })
    .await
}
