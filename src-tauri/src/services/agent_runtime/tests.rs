use super::{
    launcher::event_from_temporary_cli,
    reducer::{reduce, AgentRuntimeEvent, AgentRuntimeEventKind, AgentRuntimeProjection},
};
use crate::models::{
    AgentCliKind, AgentRuntimeOrigin, AgentRuntimeScope, AgentRuntimeState,
    TemporaryCliInstanceStatus, TemporaryCliTerminalKind,
};

fn event(
    id: &str,
    session_id: Option<&str>,
    observed_at: i64,
    kind: AgentRuntimeEventKind,
) -> AgentRuntimeEvent {
    AgentRuntimeEvent {
        event_id: id.to_string(),
        runtime_id: None,
        balancehub_instance_id: None,
        agent_session_id: session_id.map(str::to_string),
        runtime_scope: AgentRuntimeScope::Native,
        origin: AgentRuntimeOrigin::ExternalHook,
        agent_kind: AgentCliKind::Codex,
        observed_at,
        kind,
    }
}

#[test]
fn hook_events_are_idempotent_and_stop_returns_to_idle() {
    let projection = reduce(
        reduce(
            AgentRuntimeProjection::default(),
            event(
                "start",
                Some("session-1"),
                10,
                AgentRuntimeEventKind::HookStart { pid: Some(42) },
            ),
        ),
        event(
            "stop",
            Some("session-1"),
            20,
            AgentRuntimeEventKind::HookStop,
        ),
    );
    let projection = reduce(
        projection,
        event(
            "stop",
            Some("session-1"),
            20,
            AgentRuntimeEventKind::HookStop,
        ),
    );
    let session = projection.sessions().next().unwrap();
    assert_eq!(session.state, AgentRuntimeState::Idle);
    assert_eq!(
        session.process.as_ref().map(|process| process.pid),
        Some(42)
    );
    assert_eq!(session.evidence.len(), 2);
}

#[test]
fn session_start_is_idle_until_busy_evidence_arrives() {
    let projection = reduce(
        AgentRuntimeProjection::default(),
        event(
            "start-idle",
            Some("session-1"),
            10,
            AgentRuntimeEventKind::HookStart { pid: None },
        ),
    );
    assert_eq!(
        projection.sessions().next().unwrap().state,
        AgentRuntimeState::Idle
    );
}

#[test]
fn launch_snapshot_does_not_overwrite_newer_hook_busy_state() {
    let launch = AgentRuntimeEvent {
        event_id: "launch-running".to_string(),
        runtime_id: Some("balancehub:instance-1".to_string()),
        balancehub_instance_id: Some("instance-1".to_string()),
        agent_session_id: Some("session-1".to_string()),
        runtime_scope: AgentRuntimeScope::Native,
        origin: AgentRuntimeOrigin::BalancehubLaunch,
        agent_kind: AgentCliKind::Codex,
        observed_at: 30,
        kind: AgentRuntimeEventKind::LaunchRunning { pid: Some(42) },
    };
    let projection = reduce(
        reduce(
            AgentRuntimeProjection::default(),
            AgentRuntimeEvent {
                runtime_id: Some("balancehub:instance-1".to_string()),
                balancehub_instance_id: Some("instance-1".to_string()),
                ..event(
                    "hook-busy",
                    Some("session-1"),
                    20,
                    AgentRuntimeEventKind::HookBusy,
                )
            },
        ),
        launch,
    );
    let session = projection.sessions().next().unwrap();
    assert_eq!(session.state, AgentRuntimeState::Busy);
    assert_eq!(session.origin, AgentRuntimeOrigin::BalancehubLaunch);
}

#[test]
fn old_events_do_not_regress_state_and_ended_is_absorbing() {
    let projection = reduce(
        reduce(
            reduce(
                AgentRuntimeProjection::default(),
                event(
                    "busy",
                    Some("session-1"),
                    30,
                    AgentRuntimeEventKind::HookBusy,
                ),
            ),
            event(
                "end",
                Some("session-1"),
                40,
                AgentRuntimeEventKind::HookSessionEnd,
            ),
        ),
        event(
            "old-stop",
            Some("session-1"),
            20,
            AgentRuntimeEventKind::HookStop,
        ),
    );
    assert_eq!(
        projection.sessions().next().unwrap().state,
        AgentRuntimeState::Ended
    );
}

#[test]
fn resume_can_reopen_an_ended_agent_session() {
    let projection = reduce(
        reduce(
            AgentRuntimeProjection::default(),
            event(
                "end",
                Some("session-1"),
                40,
                AgentRuntimeEventKind::HookSessionEnd,
            ),
        ),
        event(
            "resume",
            Some("session-1"),
            50,
            AgentRuntimeEventKind::Resume,
        ),
    );
    let session = projection.sessions().next().unwrap();
    assert_eq!(session.state, AgentRuntimeState::Idle);
    assert_eq!(session.ended_at, None);
    assert!(!session.actions.can_resume);
}

#[test]
fn external_timeout_is_unknown_but_launch_exit_is_ended() {
    let external = reduce(
        AgentRuntimeProjection::default(),
        event(
            "timeout",
            Some("session-1"),
            50,
            AgentRuntimeEventKind::ExternalTimeout,
        ),
    );
    assert_eq!(
        external.sessions().next().unwrap().state,
        AgentRuntimeState::Unknown
    );

    let launch_event = AgentRuntimeEvent {
        event_id: "launch".to_string(),
        runtime_id: Some("balancehub:instance-1".to_string()),
        balancehub_instance_id: Some("instance-1".to_string()),
        agent_session_id: None,
        runtime_scope: AgentRuntimeScope::Native,
        origin: AgentRuntimeOrigin::BalancehubLaunch,
        agent_kind: AgentCliKind::ClaudeCode,
        observed_at: 60,
        kind: AgentRuntimeEventKind::LaunchExited { exit_code: Some(7) },
    };
    let launch = reduce(AgentRuntimeProjection::default(), launch_event);
    let session = launch.sessions().next().unwrap();
    assert_eq!(session.state, AgentRuntimeState::Ended);
    assert_eq!(session.exit_code, Some(7));
}

#[test]
fn same_agent_session_id_in_different_scopes_does_not_merge() {
    let first = event("native", Some("same"), 1, AgentRuntimeEventKind::HookBusy);
    let second = AgentRuntimeEvent {
        runtime_scope: AgentRuntimeScope::Wsl {
            distro_id: "Ubuntu".to_string(),
        },
        ..event("wsl", Some("same"), 2, AgentRuntimeEventKind::HookBusy)
    };
    let projection = reduce(reduce(AgentRuntimeProjection::default(), first), second);
    assert_eq!(projection.sessions().count(), 2);
}

#[test]
fn launch_adapter_preserves_non_secret_source_and_runtime_id() {
    let instance = crate::models::TemporaryCliInstance {
        id: "instance-1".to_string(),
        provider_id: "provider-1".to_string(),
        provider_name: "Relay".to_string(),
        session_title: "Session".to_string(),
        account_label: "Account".to_string(),
        cli_kind: AgentCliKind::Gemini,
        workdir: "/workspace".to_string(),
        terminal_kind: TemporaryCliTerminalKind::Terminal,
        terminal_name: "Terminal".to_string(),
        terminal_locator: Some(r#"{"kind":"ghostty","terminalId":"terminal-1"}"#.to_string()),
        started_at: "100".to_string(),
        ended_at: None,
        pid: Some(99),
        status: TemporaryCliInstanceStatus::Running,
        exit_code: None,
        can_activate: false,
        api_key_local_id: Some("local-key-1".to_string()),
    };
    let event = event_from_temporary_cli(&instance, 100);
    assert_eq!(event.runtime_id.as_deref(), Some("balancehub:instance-1"));
    let session = reduce(AgentRuntimeProjection::default(), event)
        .sessions()
        .next()
        .unwrap()
        .clone();
    assert_eq!(session.origin, AgentRuntimeOrigin::BalancehubLaunch);
    assert_eq!(session.state, AgentRuntimeState::Idle);
    assert_eq!(session.provider.as_ref().unwrap().provider_id, "provider-1");
    assert_eq!(
        session
            .provider
            .as_ref()
            .unwrap()
            .api_key_local_id
            .as_deref(),
        Some("local-key-1")
    );
    assert_eq!(session.process.as_ref().unwrap().pid, 99);
    assert_eq!(
        session.terminal.as_ref().unwrap().locator.as_deref(),
        Some(r#"{"kind":"ghostty","terminalId":"terminal-1"}"#)
    );
}

#[test]
fn launch_snapshot_event_id_is_stable_until_snapshot_content_changes() {
    let mut instance = crate::models::TemporaryCliInstance {
        id: "instance-stable".to_string(),
        provider_id: "provider-1".to_string(),
        provider_name: "Relay".to_string(),
        session_title: "Session".to_string(),
        account_label: "Account".to_string(),
        api_key_local_id: Some("local-key-1".to_string()),
        cli_kind: AgentCliKind::Codex,
        workdir: "/workspace".to_string(),
        terminal_kind: TemporaryCliTerminalKind::Terminal,
        terminal_name: "Terminal".to_string(),
        terminal_locator: None,
        started_at: "100".to_string(),
        ended_at: None,
        pid: Some(99),
        status: TemporaryCliInstanceStatus::Running,
        exit_code: None,
        can_activate: false,
    };
    let first = event_from_temporary_cli(&instance, 100);
    let later_observation = event_from_temporary_cli(&instance, 200);
    assert_eq!(first.event_id, later_observation.event_id);

    instance.pid = Some(100);
    let changed = event_from_temporary_cli(&instance, 200);
    assert_ne!(first.event_id, changed.event_id);
}

#[test]
fn ended_runtime_does_not_invent_resume_capability() {
    let projection = reduce(
        AgentRuntimeProjection::default(),
        event(
            "ended-without-capability",
            Some("session-1"),
            10,
            AgentRuntimeEventKind::HookSessionEnd,
        ),
    );
    assert!(!projection.sessions().next().unwrap().actions.can_resume);
}

#[test]
fn session_evidence_is_bounded() {
    let mut projection = AgentRuntimeProjection::default();
    for index in 0..300 {
        projection = reduce(
            projection,
            event(
                &format!("evidence-{index}"),
                Some("session-1"),
                index,
                AgentRuntimeEventKind::HookBusy,
            ),
        );
    }
    assert_eq!(projection.sessions().next().unwrap().evidence.len(), 256);
}
