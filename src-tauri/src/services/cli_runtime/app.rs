use super::{preview_config, snapshot, switch_config};
use crate::{
    models::{
        AgentCliKind, AppData, CliConfigFile, CliConfigPreview, CliEnvironmentProbeResult,
        CliRuntimeSnapshot, Provider,
    },
    services::agent_cli,
    state::AppState,
};
use tauri::{AppHandle, Manager};

pub(crate) struct CliRuntimeService<'a> {
    app: &'a AppHandle,
}

impl<'a> CliRuntimeService<'a> {
    pub(crate) fn new(app: &'a AppHandle) -> Self {
        Self { app }
    }

    pub(crate) fn snapshot(&self) -> CliRuntimeSnapshot {
        let data = self.data();
        snapshot(&data.providers)
    }

    pub(crate) fn preview_config(
        &self,
        provider_id: &str,
        cli_kind: AgentCliKind,
    ) -> Result<CliConfigPreview, String> {
        let data = self.data();
        preview_config(find_provider(&data, provider_id)?, cli_kind)
    }

    pub(crate) fn switch_config(
        &self,
        provider_id: &str,
        cli_kind: AgentCliKind,
        revision: &str,
        files: &[CliConfigFile],
    ) -> Result<CliRuntimeSnapshot, String> {
        let data = self.data();
        switch_config(
            find_provider(&data, provider_id)?,
            cli_kind,
            Some(revision),
            files,
        )?;
        Ok(snapshot(&data.providers))
    }

    pub(crate) fn probe_tools(&self, deep: bool) -> CliEnvironmentProbeResult {
        agent_cli::probe_all(&self.data().settings, deep)
    }

    fn data(&self) -> AppData {
        self.app
            .state::<AppState>()
            .data
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

fn find_provider<'a>(data: &'a AppData, id: &str) -> Result<&'a Provider, String> {
    data.providers
        .iter()
        .find(|provider| provider.identity.id == id)
        .ok_or_else(|| "中转站不存在".to_string())
}
