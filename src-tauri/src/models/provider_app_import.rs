use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CcSwitchAgent {
    Codex,
    Claude,
    Gemini,
    Grokbuild,
    Opencode,
    Openclaw,
    Hermes,
}

impl CcSwitchAgent {
    pub(crate) const ALL: [Self; 7] = [
        Self::Codex,
        Self::Claude,
        Self::Gemini,
        Self::Grokbuild,
        Self::Opencode,
        Self::Openclaw,
        Self::Hermes,
    ];

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Grokbuild => "grokbuild",
            Self::Opencode => "opencode",
            Self::Openclaw => "openclaw",
            Self::Hermes => "hermes",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex CLI",
            Self::Claude => "Claude Code",
            Self::Gemini => "Gemini CLI",
            Self::Grokbuild => "Grok Build",
            Self::Opencode => "OpenCode",
            Self::Openclaw => "OpenClaw",
            Self::Hermes => "Hermes",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "app", content = "agent", rename_all = "camelCase")]
pub enum ProviderAppImportTarget {
    Magpie,
    CcSwitch(CcSwitchAgent),
}

impl ProviderAppImportTarget {
    pub(crate) fn app_label(self) -> &'static str {
        match self {
            Self::Magpie => "Magpie",
            Self::CcSwitch(_) => "CC Switch",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Magpie => "Magpie",
            Self::CcSwitch(agent) => agent.label(),
        }
    }
}

/// Derived IPC state only; no import links or credentials are stored here.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAppImportOption {
    pub target: ProviderAppImportTarget,
    pub label: &'static str,
    pub unavailable_reason: Option<String>,
    pub note: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAppImportResult {
    pub app_label: &'static str,
    pub target_label: &'static str,
    pub launch_error: Option<String>,
    /// Returned only on launch failure, for an explicit user copy action.
    pub import_url: Option<String>,
}
