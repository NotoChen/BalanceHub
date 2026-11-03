//! MCP 连接检测命令。

use crate::models::AgentMcpCheck;

pub(crate) fn start_agent_mcp_check(
    actor: &str,
    connection: String,
) -> Result<AgentMcpCheck, String> {
    let actor = crate::state::asset_actor(actor).map_err(|_| "窗口已关闭")?;
    let settings = crate::state::app_data().settings;
    crate::state::services()
        .mcp_checks
        .start(actor, connection, settings)
}

pub(crate) fn get_agent_mcp_check(actor: &str, id: String) -> Result<AgentMcpCheck, String> {
    let actor = crate::state::asset_actor(actor).map_err(|_| "窗口已关闭")?;
    crate::state::services().mcp_checks.get(&actor, &id, false)
}

pub(crate) fn cancel_agent_mcp_check(actor: &str, id: String) -> Result<AgentMcpCheck, String> {
    let actor = crate::state::asset_actor(actor).map_err(|_| "窗口已关闭")?;
    crate::state::services().mcp_checks.get(&actor, &id, true)
}
