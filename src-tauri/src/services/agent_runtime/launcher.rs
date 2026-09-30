use super::reducer::{AgentRuntimeEvent, AgentRuntimeEventKind, AgentRuntimeLaunchSnapshot};
use crate::models::{
    AgentRuntimeOrigin, AgentRuntimeScope, AgentRuntimeState, TemporaryCliInstance,
    TemporaryCliInstanceStatus,
};
use sha2::{Digest, Sha256};

/// The runtime ID is independent from an Agent session ID and remains stable
/// across Hook enrichment for the same BalanceHub launch.
pub fn runtime_id_for_instance(instance_id: &str) -> String {
    format!("balancehub:{instance_id}")
}

/// Converts the existing launch repository record into a normalized event.  It
/// preserves launch-only facts while keeping secrets out of the runtime model.
pub fn event_from_temporary_cli(
    instance: &TemporaryCliInstance,
    observed_at: i64,
) -> AgentRuntimeEvent {
    event_from_temporary_cli_with_api_key(instance, observed_at, None)
}

/// Variant used by the launch orchestrator when a freshly selected local API
/// Key ID is available. The ID is a local reference only; the key value never
/// enters this contract.
pub fn event_from_temporary_cli_with_api_key(
    instance: &TemporaryCliInstance,
    observed_at: i64,
    api_key_local_id: Option<&str>,
) -> AgentRuntimeEvent {
    let status = match instance.status {
        TemporaryCliInstanceStatus::Starting
            if instance.native_session.is_some()
                && parse_timestamp(&instance.started_at)
                    .is_some_and(|started| observed_at.saturating_sub(started) >= 120_000) =>
        {
            AgentRuntimeState::Unknown
        }
        TemporaryCliInstanceStatus::Starting => AgentRuntimeState::Starting,
        TemporaryCliInstanceStatus::Running => AgentRuntimeState::Idle,
        TemporaryCliInstanceStatus::Exited => AgentRuntimeState::Ended,
    };
    let api_key_local_id = api_key_local_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| instance.api_key_local_id.clone());
    let snapshot = AgentRuntimeLaunchSnapshot {
        status,
        started_at: parse_timestamp(&instance.started_at),
        pid: instance.pid,
        ended_at: instance.ended_at.as_deref().and_then(parse_timestamp),
        exit_code: instance.exit_code,
        provider_id: instance.provider_id.clone(),
        provider_name: instance.provider_name.clone(),
        native_session: instance.native_session.clone(),
        account_label: non_empty(instance.account_label.clone()),
        api_key_local_id,
        workdir: Some(instance.workdir.clone()),
        terminal_kind: Some(instance.terminal_kind),
        terminal_locator: instance.terminal_locator.clone(),
        can_activate_terminal: instance.can_activate,
        title: non_empty(instance.session_title.clone()),
    };
    AgentRuntimeEvent {
        event_id: launch_snapshot_event_id(&instance.id, &snapshot),
        runtime_id: Some(runtime_id_for_instance(&instance.id)),
        balancehub_instance_id: Some(instance.id.clone()),
        agent_session_id: instance
            .native_session
            .as_ref()
            .map(|identity| identity.native_session_id.clone()),
        runtime_scope: AgentRuntimeScope::Native,
        origin: AgentRuntimeOrigin::BalancehubLaunch,
        agent_kind: instance.cli_kind,
        observed_at,
        kind: AgentRuntimeEventKind::LaunchSnapshot(snapshot),
    }
}

fn launch_snapshot_event_id(instance_id: &str, snapshot: &AgentRuntimeLaunchSnapshot) -> String {
    let mut hasher = Sha256::new();
    hasher.update(instance_id.as_bytes());
    let bytes = serde_json::to_vec(snapshot).expect("launch snapshot is serializable");
    hasher.update(bytes);
    format!("launch-status:{instance_id}:{:x}", hasher.finalize())
}

fn parse_timestamp(value: &str) -> Option<i64> {
    value.trim().parse().ok()
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}
