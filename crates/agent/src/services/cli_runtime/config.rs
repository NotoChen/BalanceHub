#[cfg(test)]
mod tests;

use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::agent_cli,
};

pub(super) fn config_snapshot(providers: &[Provider], cli_kind: AgentCliKind) -> CliConfigSnapshot {
    let definition = agent_cli::definition(cli_kind);
    definition.default_config().map_or_else(
        || CliConfigSnapshot {
            cli_kind,
            configured: false,
            provider_id: None,
            api_key_local_id: None,
            modified_at: None,
            error_message: Some(format!("{} 当前不支持默认配置读取", definition.label)),
        },
        |adapter| adapter.snapshot(cli_kind, providers),
    )
}
