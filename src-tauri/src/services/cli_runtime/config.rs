#[cfg(test)]
mod tests;

use crate::{
    models::{AgentCliKind, CliConfigFile, CliConfigPreview, CliConfigSnapshot, Provider},
    services::agent_cli,
};

pub(super) fn config_snapshot(providers: &[Provider], cli_kind: AgentCliKind) -> CliConfigSnapshot {
    let definition = agent_cli::definition(cli_kind);
    definition.default_config().map_or_else(
        || CliConfigSnapshot {
            cli_kind,
            configured: false,
            provider_id: None,
            modified_at: None,
            error_message: Some(format!("{} 当前不支持默认配置读取", definition.label)),
        },
        |adapter| adapter.snapshot(cli_kind, providers),
    )
}

pub fn preview_config(
    provider: &Provider,
    cli_kind: AgentCliKind,
) -> Result<CliConfigPreview, String> {
    let definition = agent_cli::definition(cli_kind);
    let adapter = definition
        .default_config()
        .ok_or_else(|| format!("{} 当前不支持默认配置切换", definition.label))?;
    adapter.preview(cli_kind, provider)
}

pub fn switch_config(
    provider: &Provider,
    cli_kind: AgentCliKind,
    expected_revision: Option<&str>,
    files: &[CliConfigFile],
) -> Result<(), String> {
    let definition = agent_cli::definition(cli_kind);
    let adapter = definition
        .default_config()
        .ok_or_else(|| format!("{} 当前不支持默认配置切换", definition.label))?;
    adapter.switch(cli_kind, provider, expected_revision, files)
}
