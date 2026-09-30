use super::snapshot;
use crate::{
    models::{AppData, CliEnvironmentProbeResult, CliRuntimeSnapshot},
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

    pub(crate) fn probe_tools(&self, deep: bool) -> CliEnvironmentProbeResult {
        let result = agent_cli::probe_all(&self.data().settings, deep);
        if let Ok(root) = self.app.path().app_data_dir() {
            static SNAPSHOT_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let _guard = SNAPSHOT_WRITE
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let _ = agent_cli::cache::write(&root.join("agent-cli-probe.json"), &result);
        }
        result
    }

    pub(crate) fn cached_tools(&self) -> Option<CliEnvironmentProbeResult> {
        agent_cli::cache::read(
            &self
                .app
                .path()
                .app_data_dir()
                .ok()?
                .join("agent-cli-probe.json"),
        )
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
