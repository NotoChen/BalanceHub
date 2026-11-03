use super::snapshot;
use crate::{
    models::{AppData, CliEnvironmentProbeResult, CliRuntimeSnapshot},
    services::agent_cli,
};

#[derive(Default)]
pub(crate) struct CliRuntimeService;

impl CliRuntimeService {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn snapshot(&self) -> CliRuntimeSnapshot {
        let data = self.data();
        snapshot(&data.providers)
    }

    pub(crate) fn probe_tools(&self, deep: bool) -> CliEnvironmentProbeResult {
        let result = agent_cli::probe_all(&self.data().settings, deep);
        if let Ok(root) = balancehub_core::host::data_dir() {
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
            &balancehub_core::host::data_dir()
                .ok()?
                .join("agent-cli-probe.json"),
        )
    }

    fn data(&self) -> AppData {
        crate::state::app_data()
    }
}
