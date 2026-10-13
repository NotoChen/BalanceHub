use crate::{models::AgentMcpCheck, state::AppState};
use tauri::{AppHandle, Manager, Window};

#[tauri::command]
pub(crate) fn start_agent_mcp_check(
    app: AppHandle,
    window: Window,
    connection: String,
) -> Result<AgentMcpCheck, String> {
    let state = app.state::<AppState>();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| "窗口已关闭")?;
    let settings = state
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    state.mcp_checks.start(actor, connection, settings)
}

#[tauri::command]
pub(crate) fn get_agent_mcp_check(
    app: AppHandle,
    window: Window,
    id: String,
) -> Result<AgentMcpCheck, String> {
    let state = app.state::<AppState>();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| "窗口已关闭")?;
    state.mcp_checks.get(&actor, &id, false)
}

#[tauri::command]
pub(crate) fn cancel_agent_mcp_check(
    app: AppHandle,
    window: Window,
    id: String,
) -> Result<AgentMcpCheck, String> {
    let state = app.state::<AppState>();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| "窗口已关闭")?;
    state.mcp_checks.get(&actor, &id, true)
}
