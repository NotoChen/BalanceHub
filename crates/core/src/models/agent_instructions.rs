use super::{AgentCliKind, AgentConfigurationSourceRequest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentInstructionSet {
    pub id: String,
    pub revision: String,
    pub name: String,
    pub common: String,
    pub additions: BTreeMap<AgentCliKind, String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentInstructionSaveRequest {
    pub id: Option<String>,
    pub expected_revision: Option<String>,
    pub name: String,
    pub common: String,
    pub additions: BTreeMap<AgentCliKind, String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentInstructionEditRequest {
    pub id: String,
    pub expected_revision: String,
    pub source: AgentConfigurationSourceRequest,
    #[serde(default)]
    pub remove: bool,
}
