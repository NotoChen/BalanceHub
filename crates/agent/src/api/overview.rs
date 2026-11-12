//! Agent 摘要命令。

use crate::{
    api::blocking,
    models::{AgentCliKind, AgentOverviewSnapshot, AgentSelfCheckReport},
    services::agent_cli::overview::{self_check_report, OverviewRefresh},
};
use std::{path::PathBuf, time::Duration};

pub(crate) async fn get_cached_agent_overview(
    workspace: Option<String>,
) -> Result<Vec<AgentOverviewSnapshot>, String> {
    let service = crate::state::services().overview()?;
    blocking("读取 Agent 摘要", move || {
        Ok(service.cached(workspace.as_deref().map(std::path::Path::new)))
    })
    .await
}

pub(crate) async fn refresh_agent_overview(
    agent_kind: AgentCliKind,
    workspace: Option<String>,
    force: bool,
) -> Result<AgentOverviewSnapshot, String> {
    let services = crate::state::services();
    let service = services.overview()?;
    let configuration = services.configuration().map_err(|_| "无法读取配置来源")?;
    let settings = crate::state::app_data().settings;
    let workspace = workspace.filter(|path| !path.is_empty()).map(PathBuf::from);
    let gate = service.gate(agent_kind, workspace.as_deref());
    let permit = tokio::time::timeout(Duration::from_secs(60), gate.lock_owned())
        .await
        .map_err(|_| "等待 Agent 刷新超时")?;
    blocking("核对 Agent 变化", move || {
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

pub(crate) async fn get_agent_environment_report(
    workspace: Option<String>,
) -> Result<AgentSelfCheckReport, String> {
    let service = crate::state::services().overview()?;
    let workspace = workspace.filter(|path| !path.is_empty()).map(PathBuf::from);
    let session_scope = blocking("读取 Agent 会话来源", || {
        crate::services::cli_sessions::workbench::get_scope(
            crate::services::cli_sessions::workbench::AgentSessionActor {
                client: "environment-self-check",
            },
            None,
        )
    })
    .await;
    let hooks = blocking("检查 Agent Hook 状态", || {
        let mut hooks = std::collections::BTreeMap::new();
        for definition in crate::services::agent_cli::definitions() {
            let Ok(service) =
                crate::services::agent_runtime::managed_hook::service(definition.kind)
            else {
                continue;
            };
            hooks.insert(
                definition.kind,
                crate::services::agent_runtime::managed_hook::inspect(&service),
            );
        }
        Ok(hooks)
    })
    .await?;
    blocking("读取 Agent 环境自检", move || {
        let (session_scope, session_scope_error) = match session_scope {
            Ok(scope) => (Some(scope), String::new()),
            Err(error) => (None, error),
        };
        Ok(self_check_report(
            service.cached(workspace.as_deref()),
            session_scope.as_ref(),
            &session_scope_error,
            &hooks,
        ))
    })
    .await
}
