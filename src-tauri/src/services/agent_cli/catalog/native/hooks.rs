//! Private, adapter-owned Hook documents and exact node changes.
//!
//! These values are never serialized to inventory or used as client authority.
//! The shared-definition editor returns the original field values. Persisted
//! anchors describe the original node and group; an array index alone never
//! authorizes restoring or rebinding a Hook after another writer changes it.

use crate::models::{
    AgentAssetDeclaredState, AgentAssetRecord, AgentAssetScope, AgentCliKind,
    AgentConfigurationContext, AgentEnvironmentInventory,
};
use crate::services::agent_cli::environment::mutation::MutationInventory;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HookNativeDefinition {
    pub event: String,
    /// One native matcher group retaining its metadata and exactly one handler.
    pub group: Value,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HookNativeAnchor {
    pub event: String,
    pub group_index: usize,
    pub handler_index: usize,
    /// Complete original group, including sibling nodes and original ordering.
    pub original_group: Value,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct HookNativeRule {
    pub source_id: String,
    /// Present only when this exact current node has a projected inventory row.
    pub native_asset_id: Option<String>,
    pub definition: HookNativeDefinition,
    pub anchor: HookNativeAnchor,
    /// Configured native enablement; runtime trust is a separate inventory fact.
    pub enabled: AgentAssetDeclaredState,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HookNativeDestination {
    pub source_id: String,
    pub scope: AgentAssetScope,
    /// Adapter-owned native document role, not a pathname or client JSON pointer.
    pub role: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookNativeSwitchMode {
    /// The adapter plans the complete native enablement policy and its impacts.
    Native,
    /// The catalog durably retains a node before removing it from native config.
    Suspend,
}

#[derive(Clone)]
pub(crate) enum HookNativeEdit {
    Add {
        definition: HookNativeDefinition,
    },
    Replace {
        original: HookNativeRule,
        definition: HookNativeDefinition,
    },
    Remove {
        original: HookNativeRule,
    },
    Restore {
        original: HookNativeRule,
        definition: HookNativeDefinition,
    },
    SetEnabled {
        original: HookNativeRule,
        enabled: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookNativeCapability {
    /// Add, edit or remove native rule definitions.
    Configuration,
    /// Change state through the adapter's native enablement policy.
    NativeSwitch,
}

impl HookNativeEdit {
    pub fn capability(&self) -> HookNativeCapability {
        if matches!(self, Self::SetEnabled { .. }) {
            HookNativeCapability::NativeSwitch
        } else {
            HookNativeCapability::Configuration
        }
    }
}

#[derive(Clone)]
pub(crate) struct HookNativeWrite {
    pub source_id: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct HookNativeExpectation {
    pub source_id: String,
    pub definition: HookNativeDefinition,
    /// Expected exact-definition cardinality after the operation (zero = absent).
    pub occurrences: usize,
    pub enabled: Option<AgentAssetDeclaredState>,
}

#[derive(Clone)]
pub(crate) struct HookNativePrepared {
    /// Structural edits can also migrate native state. Report those real
    /// dependencies so adapters without native state control cannot authorize them.
    pub required_capabilities: Vec<HookNativeCapability>,
    /// All inputs affecting this change, including untouched policy sources.
    pub read_source_ids: Vec<String>,
    /// At most one final replacement for each source. No raw filesystem writes.
    pub writes: Vec<HookNativeWrite>,
    pub expectations: Vec<HookNativeExpectation>,
    pub affected_asset_ids: Vec<String>,
    pub notes: Vec<String>,
}

pub(crate) struct NativeHookAdapter {
    pub switch_mode: HookNativeSwitchMode,
    pub agent_kind: AgentCliKind,
    pub parser_version: u32,
    /// Default document role for a new rule; existing rules keep their source.
    pub default_role: &'static str,
    pub read_hook: fn(&MutationInventory, &AgentAssetRecord) -> Result<HookNativeRule, String>,
    /// A readable in-place rule can still depend on sibling handlers. This
    /// gate proves that its standalone definition is safe to put in the library.
    pub validate_adoption: fn(&HookNativeRule) -> Result<(), String>,
    pub hook_targets:
        fn(&AgentEnvironmentInventory, &AgentConfigurationContext) -> Vec<HookNativeDestination>,
    /// Inspect all configured nodes in this one exact source, without executing.
    pub inspect_source: fn(&MutationInventory, &str) -> Result<Vec<HookNativeRule>, String>,
    pub validate_definition: fn(&HookNativeDefinition) -> Result<(), String>,
    /// Pure preparation over verified snapshots. Compose all edits together,
    /// including edits to the same native event array; preserve unknown nodes.
    pub prepare_hook_edits: fn(
        &MutationInventory,
        &HookNativeDestination,
        &[HookNativeEdit],
    ) -> Result<HookNativePrepared, String>,
}

impl NativeHookAdapter {
    pub fn qualify(
        &self,
        context: &AgentConfigurationContext,
        capability: HookNativeCapability,
    ) -> Result<(), String> {
        if context.agent_kind != self.agent_kind || context.parser_version != self.parser_version {
            return Err("Hook 配置上下文与原生格式不匹配，请刷新资产".to_owned());
        }
        if capability == HookNativeCapability::NativeSwitch
            && self.switch_mode != HookNativeSwitchMode::Native
        {
            return Err("该 Agent 没有独立的 Hook 开关，请使用暂停或恢复".to_owned());
        }
        Ok(())
    }
}
