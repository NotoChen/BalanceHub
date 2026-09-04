//! Shared contract for Agent sessions observed by BalanceHub.
//!
//! This module deliberately contains facts and capabilities only.  Hook adapters,
//! launch repositories and session parsers feed this contract; consumers must not
//! infer provider, liveness or terminal ownership from an Agent name or workdir.

use super::{AgentCliKind, TemporaryCliTerminalKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AgentRuntimeScope {
    #[default]
    Native,
    Wsl {
        distro_id: String,
    },
}

impl AgentRuntimeScope {
    pub fn key(&self) -> String {
        match self {
            Self::Native => "native".to_string(),
            Self::Wsl { distro_id } => format!("wsl:{distro_id}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeOrigin {
    BalancehubLaunch,
    ExternalHook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeState {
    Starting,
    Busy,
    Idle,
    Ended,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeEvidenceSource {
    LaunchRegistration,
    LaunchStatus,
    Hook,
    Process,
    Terminal,
    SessionAdapter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeConfidence {
    Weak,
    Observed,
    Exact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeEvidence {
    pub source: AgentRuntimeEvidenceSource,
    pub confidence: AgentRuntimeConfidence,
    pub observed_at: i64,
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeProviderRef {
    pub provider_id: String,
    pub provider_name: String,
    #[serde(default)]
    pub account_label: String,
    #[serde(default)]
    pub api_key_local_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeProcessEvidence {
    pub pid: u32,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeTerminalEvidence {
    pub kind: TemporaryCliTerminalKind,
    /// Opaque terminal locator.  It is intentionally not interpreted by the
    /// runtime reducer or exposed as a path/command.
    pub locator: Option<String>,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeActions {
    pub can_activate_terminal: bool,
    pub can_view_detail: bool,
    pub can_resume: bool,
    pub can_dismiss: bool,
}

impl Default for AgentRuntimeActions {
    fn default() -> Self {
        Self {
            can_activate_terminal: false,
            can_view_detail: true,
            can_resume: false,
            can_dismiss: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeSession {
    pub runtime_id: String,
    pub runtime_scope: AgentRuntimeScope,
    pub origin: AgentRuntimeOrigin,
    pub agent_kind: AgentCliKind,
    pub agent_session_id: Option<String>,
    pub balancehub_instance_id: Option<String>,
    pub provider: Option<AgentRuntimeProviderRef>,
    pub workdir: Option<String>,
    pub title: Option<String>,
    pub model: Option<String>,
    pub process: Option<AgentRuntimeProcessEvidence>,
    pub terminal: Option<AgentRuntimeTerminalEvidence>,
    pub state: AgentRuntimeState,
    pub evidence: Vec<AgentRuntimeEvidence>,
    pub started_at: Option<i64>,
    pub last_activity_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub actions: AgentRuntimeActions,
    /// Internal lifecycle clock. It is intentionally not part of the IPC
    /// contract: enrichment timestamps must never keep an external session
    /// alive or delay its timeout transition.
    #[serde(skip)]
    pub(crate) state_observed_at: Option<i64>,
}
