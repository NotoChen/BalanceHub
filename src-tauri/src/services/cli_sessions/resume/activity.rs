use crate::{
    models::{
        AgentCliKind, AgentRuntimeScope, AgentSessionLaunchIdentity, TemporaryCliInstanceStatus,
    },
    services::{agent_runtime::runtime_id_for_instance, cli_runtime},
};

pub(crate) struct NativeSessionActivity {
    pub cli_kind: AgentCliKind,
    pub native_session: AgentSessionLaunchIdentity,
    pub runtime_id: String,
}

impl NativeSessionActivity {
    pub(crate) fn matches(
        &self,
        cli_kind: AgentCliKind,
        source_identity: &str,
        native_session_id: &str,
    ) -> bool {
        self.cli_kind == cli_kind
            && self.native_session.matches_source_session(
                &AgentRuntimeScope::Native,
                source_identity,
                native_session_id,
            )
    }
}

/// One read per query/detail: source-proved launches with a live process only.
/// A Hook ID without a verified native source cannot create a history binding.
pub(crate) fn native_session_activity() -> Vec<NativeSessionActivity> {
    cli_runtime::runtime_instances()
        .into_iter()
        .filter_map(|instance| {
            if instance.status != TemporaryCliInstanceStatus::Running || instance.pid.is_none() {
                return None;
            }
            let native_session = instance.native_session?;
            Some(NativeSessionActivity {
                cli_kind: instance.cli_kind,
                native_session,
                runtime_id: runtime_id_for_instance(&instance.id),
            })
        })
        .collect()
}
