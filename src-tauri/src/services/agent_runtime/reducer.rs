use crate::models::{
    AgentCliKind, AgentRuntimeActions, AgentRuntimeConfidence, AgentRuntimeEvidence,
    AgentRuntimeEvidenceSource, AgentRuntimeOrigin, AgentRuntimeProcessEvidence,
    AgentRuntimeProviderRef, AgentRuntimeScope, AgentRuntimeSession, AgentRuntimeState,
    AgentRuntimeTerminalEvidence, TemporaryCliTerminalKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_REMEMBERED_EVENT_IDS: usize = 4096;
const MAX_SESSION_EVIDENCE: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeLaunchSnapshot {
    pub status: AgentRuntimeState,
    pub started_at: Option<i64>,
    pub pid: Option<u32>,
    pub ended_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    #[serde(default)]
    pub native_session: Option<crate::models::AgentSessionLaunchIdentity>,
    pub account_label: Option<String>,
    #[serde(default)]
    pub api_key_local_id: Option<String>,
    pub workdir: Option<String>,
    pub terminal_kind: Option<TemporaryCliTerminalKind>,
    pub terminal_locator: Option<String>,
    pub can_activate_terminal: bool,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeEnrichmentSource {
    Hook,
    SessionAdapter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEnrichment {
    pub source: RuntimeEnrichmentSource,
    pub title: Option<String>,
    pub model: Option<String>,
    pub workdir: Option<String>,
    pub source_last_activity_at: Option<i64>,
    pub source_revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum AgentRuntimeEventKind {
    LaunchSnapshot(AgentRuntimeLaunchSnapshot),
    LaunchRunning { pid: Option<u32> },
    LaunchExited { exit_code: Option<i32> },
    HookStart { pid: Option<u32> },
    HookBusy,
    HookStop,
    HookSessionEnd,
    ExternalTimeout,
    Resume,
    Enrichment(RuntimeEnrichment),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeEvent {
    pub event_id: String,
    pub runtime_id: Option<String>,
    pub balancehub_instance_id: Option<String>,
    pub agent_session_id: Option<String>,
    pub runtime_scope: AgentRuntimeScope,
    pub origin: AgentRuntimeOrigin,
    pub agent_kind: AgentCliKind,
    pub observed_at: i64,
    pub kind: AgentRuntimeEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentRuntimeProjection {
    sessions: BTreeMap<String, AgentRuntimeSession>,
    remembered_event_ids: BTreeSet<String>,
    event_order: VecDeque<String>,
}

impl AgentRuntimeProjection {
    pub fn sessions(&self) -> impl Iterator<Item = &AgentRuntimeSession> {
        self.sessions.values()
    }
}

/// Pure projection reducer.  The input is consumed so callers cannot observe a
/// partially updated projection; duplicate event IDs return the projection
/// unchanged.  Event IDs are retained in a bounded FIFO window.
pub fn reduce(
    mut projection: AgentRuntimeProjection,
    event: AgentRuntimeEvent,
) -> AgentRuntimeProjection {
    if event.event_id.trim().is_empty() || projection.remembered_event_ids.contains(&event.event_id)
    {
        return projection;
    }
    remember_event(&mut projection, event.event_id.clone());

    let runtime_id = resolve_runtime_id(&event);
    let existing = projection.sessions.remove(&runtime_id);
    let session = reduce_session(existing, &event, runtime_id.clone());
    projection.sessions.insert(runtime_id, session);
    projection
}

fn remember_event(projection: &mut AgentRuntimeProjection, event_id: String) {
    projection.remembered_event_ids.insert(event_id.clone());
    projection.event_order.push_back(event_id);
    while projection.event_order.len() > MAX_REMEMBERED_EVENT_IDS {
        if let Some(oldest) = projection.event_order.pop_front() {
            projection.remembered_event_ids.remove(&oldest);
        }
    }
}

pub(crate) fn resolve_runtime_id(event: &AgentRuntimeEvent) -> String {
    if let Some(id) = event
        .runtime_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
    {
        return id.to_string();
    }
    if let Some(instance_id) = event
        .balancehub_instance_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
    {
        return format!("balancehub:{instance_id}");
    }
    if let Some(session_id) = event
        .agent_session_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
    {
        return external_runtime_key(&event.runtime_scope, event.agent_kind, session_id);
    }
    format!(
        "external:{}:{}:event:{}",
        event.runtime_scope.key(),
        event.agent_kind.key(),
        event.event_id
    )
}

fn external_runtime_key(
    scope: &AgentRuntimeScope,
    agent_kind: AgentCliKind,
    session_id: &str,
) -> String {
    format!(
        "external:{}:{}:{}",
        scope.key(),
        agent_kind.key(),
        session_id
    )
}

fn reduce_session(
    existing: Option<AgentRuntimeSession>,
    event: &AgentRuntimeEvent,
    runtime_id: String,
) -> AgentRuntimeSession {
    let mut session = existing.unwrap_or_else(|| new_session(runtime_id, event));
    let is_resume = matches!(&event.kind, AgentRuntimeEventKind::Resume);
    let is_enrichment = matches!(&event.kind, AgentRuntimeEventKind::Enrichment(_));
    let is_old_event = session
        .state_observed_at
        .is_some_and(|last| event.observed_at < last);

    if session.agent_session_id.is_none() {
        session.agent_session_id = event.agent_session_id.clone();
    }
    if session.balancehub_instance_id.is_none() {
        session.balancehub_instance_id = event.balancehub_instance_id.clone();
    }
    if event.origin == AgentRuntimeOrigin::BalancehubLaunch {
        session.origin = AgentRuntimeOrigin::BalancehubLaunch;
    }
    if !is_enrichment
        && (session.started_at.is_none()
            || event.observed_at < session.started_at.unwrap_or(i64::MAX))
    {
        session.started_at = Some(event.observed_at);
    }
    if !is_enrichment && !matches!(&event.kind, AgentRuntimeEventKind::ExternalTimeout) {
        session.last_activity_at =
            Some(session.last_activity_at.unwrap_or(0).max(event.observed_at));
    }
    if !is_enrichment {
        session.state_observed_at = Some(
            session
                .state_observed_at
                .unwrap_or(0)
                .max(event.observed_at),
        );
    }
    push_evidence(&mut session, event);

    if is_resume && session.state == AgentRuntimeState::Ended {
        session.state = AgentRuntimeState::Idle;
        session.ended_at = None;
        session.exit_code = None;
    }

    if !is_old_event || is_resume || is_enrichment {
        apply_event(&mut session, event);
    }
    session.actions.can_activate_terminal =
        session.state != AgentRuntimeState::Ended && session.actions.can_activate_terminal;
    session
}

fn new_session(runtime_id: String, event: &AgentRuntimeEvent) -> AgentRuntimeSession {
    AgentRuntimeSession {
        runtime_id,
        runtime_scope: event.runtime_scope.clone(),
        origin: event.origin,
        agent_kind: event.agent_kind,
        agent_session_id: event.agent_session_id.clone(),
        native_session: None,
        balancehub_instance_id: event.balancehub_instance_id.clone(),
        provider: None,
        workdir: None,
        title: None,
        model: None,
        process: None,
        terminal: None,
        state: AgentRuntimeState::Unknown,
        evidence: Vec::new(),
        started_at: Some(event.observed_at),
        last_activity_at: Some(event.observed_at),
        ended_at: None,
        exit_code: None,
        actions: AgentRuntimeActions::default(),
        state_observed_at: Some(event.observed_at),
    }
}

fn push_evidence(session: &mut AgentRuntimeSession, event: &AgentRuntimeEvent) {
    let (source, confidence) = match &event.kind {
        AgentRuntimeEventKind::LaunchSnapshot(_)
        | AgentRuntimeEventKind::LaunchRunning { .. }
        | AgentRuntimeEventKind::LaunchExited { .. } => (
            AgentRuntimeEvidenceSource::LaunchStatus,
            AgentRuntimeConfidence::Exact,
        ),
        AgentRuntimeEventKind::HookStart { .. }
        | AgentRuntimeEventKind::HookBusy
        | AgentRuntimeEventKind::HookStop
        | AgentRuntimeEventKind::HookSessionEnd => (
            AgentRuntimeEvidenceSource::Hook,
            AgentRuntimeConfidence::Observed,
        ),
        AgentRuntimeEventKind::ExternalTimeout => (
            AgentRuntimeEvidenceSource::Process,
            AgentRuntimeConfidence::Weak,
        ),
        AgentRuntimeEventKind::Resume => (
            AgentRuntimeEvidenceSource::Hook,
            AgentRuntimeConfidence::Observed,
        ),
        AgentRuntimeEventKind::Enrichment(enrichment) => (
            match enrichment.source {
                RuntimeEnrichmentSource::Hook => AgentRuntimeEvidenceSource::Hook,
                RuntimeEnrichmentSource::SessionAdapter => {
                    AgentRuntimeEvidenceSource::SessionAdapter
                }
            },
            AgentRuntimeConfidence::Observed,
        ),
    };
    session.evidence.push(AgentRuntimeEvidence {
        source,
        confidence,
        observed_at: event.observed_at,
        event_id: event.event_id.clone(),
    });
    if session.evidence.len() > MAX_SESSION_EVIDENCE {
        session
            .evidence
            .drain(..session.evidence.len() - MAX_SESSION_EVIDENCE);
    }
}

fn apply_event(session: &mut AgentRuntimeSession, event: &AgentRuntimeEvent) {
    match &event.kind {
        AgentRuntimeEventKind::LaunchSnapshot(snapshot) => {
            apply_launch_snapshot(session, snapshot, event.observed_at);
        }
        AgentRuntimeEventKind::LaunchRunning { pid } => {
            if matches!(
                session.state,
                AgentRuntimeState::Starting | AgentRuntimeState::Unknown
            ) {
                session.state = AgentRuntimeState::Idle;
            }
            if let Some(pid) = pid {
                session.process = Some(AgentRuntimeProcessEvidence {
                    pid: *pid,
                    observed_at: event.observed_at,
                });
            }
        }
        AgentRuntimeEventKind::LaunchExited { exit_code } => {
            if session.origin == AgentRuntimeOrigin::BalancehubLaunch {
                end_session(session, event.observed_at, *exit_code);
            }
        }
        AgentRuntimeEventKind::HookStart { pid } => {
            if session.state != AgentRuntimeState::Ended {
                session.state = AgentRuntimeState::Idle;
            }
            if let Some(pid) = pid {
                session.process = Some(AgentRuntimeProcessEvidence {
                    pid: *pid,
                    observed_at: event.observed_at,
                });
            }
        }
        AgentRuntimeEventKind::HookBusy => {
            if session.state != AgentRuntimeState::Ended {
                session.state = AgentRuntimeState::Busy;
            }
        }
        AgentRuntimeEventKind::HookStop => {
            if session.state != AgentRuntimeState::Ended {
                session.state = AgentRuntimeState::Idle;
            }
        }
        AgentRuntimeEventKind::HookSessionEnd => {
            end_session(session, event.observed_at, None);
        }
        AgentRuntimeEventKind::ExternalTimeout => {
            if session.origin == AgentRuntimeOrigin::ExternalHook
                && session.state != AgentRuntimeState::Ended
            {
                session.state = AgentRuntimeState::Unknown;
            }
        }
        AgentRuntimeEventKind::Resume => {
            if session.state != AgentRuntimeState::Ended {
                session.state = AgentRuntimeState::Idle;
            }
        }
        AgentRuntimeEventKind::Enrichment(enrichment) => {
            set_if_present(&mut session.title, enrichment.title.clone());
            set_if_present(&mut session.model, enrichment.model.clone());
            set_if_present(&mut session.workdir, enrichment.workdir.clone());
            if let Some(activity) = enrichment.source_last_activity_at {
                session.last_activity_at =
                    Some(session.last_activity_at.unwrap_or(0).max(activity));
            }
        }
    }
}

fn apply_launch_snapshot(
    session: &mut AgentRuntimeSession,
    snapshot: &AgentRuntimeLaunchSnapshot,
    observed_at: i64,
) {
    if let Some(started_at) = snapshot.started_at {
        session.started_at = Some(session.started_at.unwrap_or(started_at).min(started_at));
    }
    session.state = match snapshot.status {
        AgentRuntimeState::Unknown
            if snapshot.native_session.is_some()
                && session.state == AgentRuntimeState::Starting =>
        {
            AgentRuntimeState::Unknown
        }
        AgentRuntimeState::Ended => AgentRuntimeState::Ended,
        AgentRuntimeState::Busy if session.state != AgentRuntimeState::Ended => {
            AgentRuntimeState::Busy
        }
        AgentRuntimeState::Idle
            if matches!(
                session.state,
                AgentRuntimeState::Starting | AgentRuntimeState::Unknown
            ) =>
        {
            AgentRuntimeState::Idle
        }
        AgentRuntimeState::Starting if session.state == AgentRuntimeState::Unknown => {
            AgentRuntimeState::Starting
        }
        _ => session.state,
    };
    if let Some(pid) = snapshot.pid {
        session.process = Some(AgentRuntimeProcessEvidence { pid, observed_at });
    }
    if let Some(identity) = &snapshot.native_session {
        session.native_session = Some(identity.clone());
    }
    if let Some(provider_id) = snapshot.provider_id.as_deref() {
        session.provider = Some(AgentRuntimeProviderRef {
            provider_id: provider_id.to_string(),
            provider_name: snapshot.provider_name.clone().unwrap_or_default(),
            account_label: snapshot.account_label.clone().unwrap_or_default(),
            api_key_local_id: snapshot.api_key_local_id.clone(),
        });
    }
    set_if_present(&mut session.workdir, snapshot.workdir.clone());
    set_if_present(&mut session.title, snapshot.title.clone());
    if let Some(kind) = snapshot.terminal_kind {
        session.terminal = Some(AgentRuntimeTerminalEvidence {
            kind,
            locator: snapshot.terminal_locator.clone(),
            observed_at,
        });
    }
    session.actions.can_activate_terminal |= snapshot.can_activate_terminal;
    if snapshot.status == AgentRuntimeState::Ended {
        session.ended_at = snapshot.ended_at.or(session.last_activity_at);
        session.exit_code = snapshot.exit_code;
    }
}

fn end_session(session: &mut AgentRuntimeSession, ended_at: i64, exit_code: Option<i32>) {
    session.state = AgentRuntimeState::Ended;
    session.ended_at = Some(ended_at);
    session.exit_code = exit_code;
    session.actions.can_activate_terminal = false;
}

fn set_if_present(target: &mut Option<String>, value: Option<String>) {
    if value
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        *target = value;
    }
}
