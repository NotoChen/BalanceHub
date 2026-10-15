use super::{AgentAssetScope, AgentCliKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettingChoice {
    pub value: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentModelCatalogContext {
    pub cli_kind: AgentCliKind,
    pub cli_path: String,
    pub config_root: String,
    /// An explicit Codex catalog replaces the native/account catalog.
    pub catalog_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelCatalog {
    pub choices: Vec<AgentSettingChoice>,
    pub source: String,
    pub notice: String,
    pub stale: bool,
    pub refreshed_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSettingControl {
    Model,
    ModelMappings,
    Select,
    Number,
    MultiSelect,
    Rules,
    Readonly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSettingValueState {
    Configured,
    Default,
    Unset,
    ReadError,
}

/// What was actually resolved, independently of whether this editor writes an override.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettingValueSummary {
    pub value: Option<String>,
    pub label: String,
    pub source: String,
    pub state: AgentSettingValueState,
    pub detail: String,
}

/// Editable parts of a native model alias. Connection and generation settings
/// remain in their original documents and never travel through this editor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentModelMappingValue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelMappingEntry {
    pub id: String,
    pub label: String,
    pub native_key: String,
    pub current: AgentSettingValueSummary,
    pub target_default: Option<AgentSettingValueSummary>,
    pub reset_value: Option<AgentSettingValueSummary>,
    pub locked_reason: String,
    pub supports_display_name: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelMappingOptions {
    pub allow_custom_aliases: bool,
    pub display_names: bool,
    pub inheritance: bool,
    pub entries: Vec<AgentModelMappingEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNativeSetting {
    pub id: String,
    pub section: String,
    pub label: String,
    pub native_key: String,
    pub source_references: Vec<AgentSettingSourceReference>,
    pub description: String,
    pub hint: String,
    pub control: AgentSettingControl,
    pub current_value: Option<String>,
    pub current_native_value: Option<String>,
    pub current_label: String,
    pub current_state: AgentSettingValueState,
    pub current_detail: String,
    pub reset_value: Option<AgentSettingEffectiveValue>,
    pub source: String,
    pub choices: Vec<AgentSettingChoice>,
    pub model_mappings: Option<AgentModelMappingOptions>,
    pub allow_custom: bool,
    pub minimum: Option<i64>,
    pub maximum: Option<i64>,
    /// A useful drag endpoint from native metadata, not a validation bound.
    pub slider_maximum: Option<i64>,
    /// Native context capacity used only to display the token value as a percentage.
    pub percentage_base: Option<i64>,
    pub minimum_items: usize,
    pub unit: String,
    pub locked_reason: String,
    pub when_option: Option<(String, String)>,
    pub exclusive_group: Option<String>,
}

/// A key present in the current draft. Positions use CodeMirror's UTF-16 offsets
/// with CRLF counted as one newline; missing keys have no source reference.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettingSourceReference {
    pub key: String,
    pub row_id: Option<String>,
    pub from: usize,
    pub to: usize,
    pub value_end: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettingReference {
    pub native_key: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentConfigurationFormRequest {
    pub edit_id: String,
    pub source_id: String,
    pub expected_revision: String,
    pub text: String,
    pub changes: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettingEffectiveValue {
    #[serde(flatten)]
    pub summary: AgentSettingValueSummary,
    pub model_mappings: Option<AgentModelMappingOptions>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationForm {
    pub source_id: String,
    pub scope: AgentAssetScope,
    pub profile: Option<String>,
    pub text: String,
    pub fields: Vec<AgentNativeSetting>,
    pub model_catalog: AgentModelCatalogContext,
    pub values: BTreeMap<String, String>,
    pub effective: BTreeMap<String, AgentSettingEffectiveValue>,
    pub inherited: BTreeMap<String, AgentSettingEffectiveValue>,
    pub notices: Vec<String>,
}
