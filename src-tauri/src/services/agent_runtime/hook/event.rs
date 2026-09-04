//! Versioned, privacy-bounded Hook event contract.

use super::super::reducer::{
    AgentRuntimeEvent, AgentRuntimeEventKind, RuntimeEnrichment, RuntimeEnrichmentSource,
};
use crate::models::{AgentCliKind, AgentRuntimeOrigin, AgentRuntimeScope};
use serde::{Deserialize, Serialize};

pub const NORMALIZED_HOOK_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizedHookLifecycle {
    Start,
    Busy,
    Idle,
    Ended,
    Unknown,
}

/// Only this allow-listed structure is persisted. Unknown input fields are
/// ignored, so prompt/response/tool/environment/credential data cannot enter
/// the spool by accident.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedHookEvent {
    pub schema_version: u16,
    pub event_id: String,
    pub agent_kind: AgentCliKind,
    pub runtime_scope: AgentRuntimeScope,
    pub native_event: String,
    pub lifecycle: NormalizedHookLifecycle,
    pub session_id: Option<String>,
    pub balancehub_instance_id: Option<String>,
    pub cwd: Option<String>,
    pub transcript_path: Option<String>,
    pub model: Option<String>,
    pub title: Option<String>,
    pub observed_at: i64,
    pub received_at: i64,
}

impl NormalizedHookEvent {
    pub fn into_runtime_event(self) -> AgentRuntimeEvent {
        let kind = match self.lifecycle {
            NormalizedHookLifecycle::Start => AgentRuntimeEventKind::HookStart { pid: None },
            NormalizedHookLifecycle::Busy => AgentRuntimeEventKind::HookBusy,
            NormalizedHookLifecycle::Idle => AgentRuntimeEventKind::HookStop,
            NormalizedHookLifecycle::Ended => AgentRuntimeEventKind::HookSessionEnd,
            NormalizedHookLifecycle::Unknown => AgentRuntimeEventKind::ExternalTimeout,
        };
        AgentRuntimeEvent {
            event_id: self.event_id,
            runtime_id: None,
            balancehub_instance_id: self.balancehub_instance_id,
            agent_session_id: self.session_id,
            runtime_scope: self.runtime_scope,
            origin: AgentRuntimeOrigin::ExternalHook,
            agent_kind: self.agent_kind,
            observed_at: self.observed_at,
            kind,
        }
    }

    /// Converts one Hook record to lifecycle plus optional metadata events for
    /// the reducer. The metadata ID remains in the same bounded idempotency
    /// window as the lifecycle event.
    pub fn into_runtime_events(self) -> Vec<AgentRuntimeEvent> {
        let event_id = self.event_id.clone();
        let has_enrichment = self.title.is_some() || self.model.is_some() || self.cwd.is_some();
        let metadata = RuntimeEnrichment {
            source: RuntimeEnrichmentSource::Hook,
            title: self.title.clone(),
            model: self.model.clone(),
            workdir: self.cwd.clone(),
            source_last_activity_at: Some(self.observed_at),
            source_revision: None,
        };
        let mut lifecycle = self.into_runtime_event();
        let mut events = vec![lifecycle.clone()];
        if has_enrichment {
            lifecycle.event_id = format!("{event_id}:enrichment");
            lifecycle.kind = AgentRuntimeEventKind::Enrichment(metadata);
            events.push(lifecycle);
        }
        events
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecodeError {
    InvalidPayload,
    UnsupportedSchema(u16),
    InvalidField(&'static str),
    UnsupportedNativeEvent(String),
}

impl std::fmt::Display for HookDecodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPayload => formatter.write_str("Hook payload 格式无效"),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "Hook schemaVersion {version} 不受支持")
            }
            Self::InvalidField(field) => write!(formatter, "Hook 字段无效: {field}"),
            Self::UnsupportedNativeEvent(event) => {
                write!(formatter, "Hook 事件类型不受支持: {event}")
            }
        }
    }
}

pub trait HookEventDecoder: Send + Sync {
    fn decode(
        &self,
        payload: &[u8],
        received_at: i64,
    ) -> Result<NormalizedHookEvent, HookDecodeError>;
}
