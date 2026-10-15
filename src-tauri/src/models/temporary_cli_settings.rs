use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::{
    AgentAssetCategory, AgentCliKind, AgentModelCatalogContext, AgentNativeSetting,
    AgentSettingChoice, AgentSettingReference, AgentSettingValueSummary,
};

fn yes() -> bool {
    true
}

/// Explicit per-invocation overrides. Missing options inherit the native Agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct TemporaryCliRunSettings {
    pub keep_session: bool,
    /// Only keys advertised by the selected native CLI are accepted at launch.
    /// Missing keys inherit; list semantics are declared by each native adapter.
    pub native_options: BTreeMap<String, String>,
    pub load_rules: bool,
    pub initial_prompt: String,
    pub instructions: String,
    pub additional_directories: Vec<String>,
    pub disabled_assets: Vec<String>,
    pub disable_skills: bool,
    pub disable_mcp: bool,
    pub disable_plugins: bool,
    pub disable_hooks: bool,
}

impl Default for TemporaryCliRunSettings {
    fn default() -> Self {
        Self {
            keep_session: yes(),
            load_rules: yes(),
            native_options: BTreeMap::new(),
            initial_prompt: String::new(),
            instructions: String::new(),
            additional_directories: Vec::new(),
            disabled_assets: Vec::new(),
            disable_skills: false,
            disable_mcp: false,
            disable_plugins: false,
            disable_hooks: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryCliProfile {
    pub id: String,
    pub name: String,
    pub cli_kind: AgentCliKind,
    pub settings: TemporaryCliRunSettings,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryCliSettingsCapabilities {
    pub can_discard_session: bool,
    pub retention_note: String,
    pub native_settings: Vec<AgentNativeSetting>,
    pub model: AgentSettingValueSummary,
    pub default_model: AgentSettingValueSummary,
    pub model_native_key: String,
    pub model_choices: Vec<AgentSettingChoice>,
    pub model_catalog: AgentModelCatalogContext,
    pub notices: Vec<String>,
    pub asset_controls: Vec<TemporaryCliAssetControl>,
    pub run_references: BTreeMap<String, AgentSettingReference>,
    pub can_skip_rules: bool,
    pub can_add_directories: bool,
    pub can_disable_skills: bool,
    pub can_disable_mcp: bool,
    pub can_disable_plugins: bool,
    pub can_disable_hooks: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryCliAssetControl {
    pub key: String,
    pub label: String,
    pub native_key: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryCliAssetOption {
    pub id: String,
    pub label: String,
    pub category: AgentAssetCategory,
    pub configurable: bool,
    pub note: String,
    pub current_label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryCliAssetListing {
    pub assets: Vec<TemporaryCliAssetOption>,
    pub notice: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryCliSettingsRequest {
    #[serde(default)]
    pub native_options: BTreeMap<String, String>,
    pub cli_kind: AgentCliKind,
    pub cli_path: String,
    pub workdir: String,
    pub model: String,
    #[serde(default)]
    pub inherited_model: String,
    pub session_mode: super::TemporaryCliSessionMode,
}
