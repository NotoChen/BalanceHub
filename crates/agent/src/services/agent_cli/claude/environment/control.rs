//! Private decoded control facts. Raw matchers are never exposed by Debug/IPC.
use crate::models::{AgentAssetCategory, AgentAssetDeclarationRole};
use std::{collections::BTreeSet, fmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceState {
    Missing,
    Readable,
    Malformed,
    Blocked,
}
impl SourceState {
    pub(super) fn invalid(self) -> bool {
        matches!(self, Self::Malformed | Self::Blocked)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ListFamily {
    Allowed,
    Denied,
    Approved,
    Rejected,
    PersonalEnabled,
    PersonalDisabled,
}
impl ListFamily {
    pub(super) fn field(self) -> &'static str {
        match self {
            Self::Allowed => "allowedMcpServers",
            Self::Denied => "deniedMcpServers",
            Self::Approved => "enabledMcpjsonServers",
            Self::Rejected => "disabledMcpjsonServers",
            Self::PersonalEnabled => "enabledMcpServers",
            Self::PersonalDisabled => "disabledMcpServers",
        }
    }
    pub(super) fn personal(self) -> bool {
        matches!(self, Self::PersonalEnabled | Self::PersonalDisabled)
    }
    pub(super) fn policy(self) -> bool {
        matches!(self, Self::Allowed | Self::Denied)
    }
    pub(super) fn role(self) -> AgentAssetDeclarationRole {
        if self.personal() {
            AgentAssetDeclarationRole::StateOverlay
        } else {
            AgentAssetDeclarationRole::PolicyOverlay
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListState {
    Absent,
    Valid,
    Invalid,
    NativeFailClosed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BooleanField {
    ApproveAll,
    ManagedMcpOnly,
    DisableAllHooks,
    ManagedHooksOnly,
}
impl BooleanField {
    pub(super) fn field(self) -> &'static str {
        match self {
            Self::ApproveAll => "enableAllProjectMcpServers",
            Self::ManagedMcpOnly => "allowManagedMcpServersOnly",
            Self::DisableAllHooks => "disableAllHooks",
            Self::ManagedHooksOnly => "allowManagedHooksOnly",
        }
    }
    pub(super) fn managed(self) -> bool {
        matches!(self, Self::ManagedMcpOnly | Self::ManagedHooksOnly)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BooleanValue {
    Absent,
    Bool(bool),
    /// Claude's managed-only decoders determinately enable an invalid guard.
    NativeFailClosed,
    Invalid,
}
impl BooleanValue {
    pub(super) fn effective(self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(value),
            Self::NativeFailClosed => Some(true),
            Self::Absent | Self::Invalid => None,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum PolicyMatcher {
    Name(String),
    Command(Vec<String>),
    Url(String),
}
impl PolicyMatcher {
    pub(super) fn field(&self) -> &'static str {
        match self {
            Self::Name(_) => "serverName",
            Self::Command(_) => "serverCommand",
            Self::Url(_) => "serverUrl",
        }
    }
}
impl fmt::Debug for PolicyMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, count) = match self {
            Self::Name(_) => ("Name", 1),
            Self::Command(values) => ("Command", values.len()),
            Self::Url(_) => ("Url", 1),
        };
        f.debug_struct("PolicyMatcher")
            .field("kind", &kind)
            .field("count", &count)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum NamespaceState {
    Absent,
    Object(BTreeSet<String>),
    InvalidContainer,
}

#[derive(Clone)]
pub(crate) enum ClaudeControlPayload {
    SourceRoot {
        state: SourceState,
    },
    ListRoot {
        family: ListFamily,
        state: ListState,
        expected_entry_count: usize,
    },
    RuleEntry {
        family: ListFamily,
        ordinal: usize,
        matcher: Option<PolicyMatcher>,
    },
    NameEntry {
        family: ListFamily,
        ordinal: usize,
        name: Option<String>,
    },
    Boolean {
        field: BooleanField,
        value: BooleanValue,
    },
    PluginRoot {
        state: ListState,
        expected_entry_count: usize,
    },
    PluginEntry {
        enabled: Option<bool>,
    },
    ManagedNamespace {
        state: NamespaceState,
    },
}
impl fmt::Debug for ClaudeControlPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("ClaudeControlPayload");
        match self {
            Self::SourceRoot { state } => debug.field("kind", &"SourceRoot").field("state", state),
            Self::ListRoot {
                family,
                state,
                expected_entry_count,
            } => debug
                .field("kind", &"ListRoot")
                .field("family", family)
                .field("state", state)
                .field("entry_count", expected_entry_count),
            Self::RuleEntry {
                family,
                ordinal,
                matcher,
            } => debug
                .field("kind", &"RuleEntry")
                .field("family", family)
                .field("ordinal", ordinal)
                .field("matcher", matcher),
            Self::NameEntry {
                family,
                ordinal,
                name,
            } => debug
                .field("kind", &"NameEntry")
                .field("family", family)
                .field("ordinal", ordinal)
                .field("valid", &name.is_some()),
            Self::Boolean { field, value } => debug
                .field("kind", &"Boolean")
                .field("field", field)
                .field("value", value),
            Self::PluginRoot {
                state,
                expected_entry_count,
            } => debug
                .field("kind", &"PluginRoot")
                .field("state", state)
                .field("entry_count", expected_entry_count),
            Self::PluginEntry { enabled } => debug
                .field("kind", &"PluginEntry")
                .field("valid", &enabled.is_some()),
            Self::ManagedNamespace { state } => {
                let (kind, count) = match state {
                    NamespaceState::Absent => ("Absent", 0),
                    NamespaceState::Object(keys) => ("Object", keys.len()),
                    NamespaceState::InvalidContainer => ("InvalidContainer", 0),
                };
                debug
                    .field("kind", &"ManagedNamespace")
                    .field("state", &kind)
                    .field("entry_count", &count)
            }
        }
        .finish()
    }
}

pub(super) fn source_role(category: AgentAssetCategory) -> AgentAssetDeclarationRole {
    if category == AgentAssetCategory::Plugin {
        AgentAssetDeclarationRole::StateOverlay
    } else {
        AgentAssetDeclarationRole::PolicyOverlay
    }
}
