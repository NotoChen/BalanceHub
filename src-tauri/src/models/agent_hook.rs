use serde::{Deserialize, Serialize};

use super::{AgentCliKind, AgentRuntimeScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHookMutation {
    Install,
    Remove,
    Enable,
    Disable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHookActionKind {
    Install,
    Remove,
    Enable,
    Disable,
    Health,
    Verify,
    Repair,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHookAction {
    pub action: AgentHookActionKind,
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHookTrust {
    Unknown,
    Trusted,
    Required,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHookHealthState {
    NotInstalled,
    InstalledUntrusted,
    InstalledUnverified,
    Healthy,
    Disabled,
    Conflict,
    HelperMissing,
    SpoolBlocked,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHookChangeKind {
    Add,
    Remove,
    Keep,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHookChange {
    pub event_name: String,
    pub structural_identity: String,
    pub fingerprint: String,
    pub kind: AgentHookChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHookOwnership {
    pub agent_kind: AgentCliKind,
    pub runtime_scope: AgentRuntimeScope,
    pub config_path: String,
    pub helper_version: String,
    pub installed_at: i64,
    pub enabled: bool,
    pub resources: Vec<AgentHookOwnedResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHookOwnedResource {
    pub event_name: String,
    pub structural_identity: String,
    pub content_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHookInspection {
    pub agent_kind: AgentCliKind,
    pub runtime_scope: AgentRuntimeScope,
    pub config_path: String,
    pub config_exists: bool,
    pub revision: String,
    pub state: AgentHookHealthState,
    pub installed: bool,
    pub enabled: bool,
    pub trusted: AgentHookTrust,
    pub helper_available: bool,
    pub spool_available: bool,
    pub last_event_at: Option<i64>,
    pub ownership: Option<AgentHookOwnership>,
    pub diagnostics: Vec<String>,
    #[serde(default)]
    pub actions: Vec<AgentHookAction>,
}

impl AgentHookInspection {
    pub fn set_actions(&mut self, installation_available: bool) {
        let can_write = !matches!(
            self.state,
            AgentHookHealthState::Conflict | AgentHookHealthState::Unsupported
        );
        let install_reason = if installation_available {
            None
        } else {
            Some("未检测到 Agent CLI，安装后才能创建 Hook".to_string())
        };
        let unavailable_reason = if !can_write {
            Some("当前 Hook 配置存在冲突或暂不支持写入".to_string())
        } else {
            None
        };
        let installed_reason = if !self.installed {
            Some("Hook 尚未安装".to_string())
        } else {
            unavailable_reason.clone()
        };
        let repair_needed = matches!(
            self.state,
            AgentHookHealthState::Conflict
                | AgentHookHealthState::HelperMissing
                | AgentHookHealthState::SpoolBlocked
        );
        self.actions = vec![
            AgentHookAction {
                action: AgentHookActionKind::Install,
                available: installation_available && can_write && !self.installed,
                reason: if !installation_available {
                    install_reason.clone()
                } else if self.installed {
                    Some("Hook 已安装".to_string())
                } else {
                    unavailable_reason.clone()
                },
            },
            AgentHookAction {
                action: AgentHookActionKind::Enable,
                available: installation_available && can_write && self.installed && !self.enabled,
                reason: if !installation_available {
                    install_reason.clone()
                } else if !self.installed {
                    Some("Hook 尚未安装".to_string())
                } else if self.enabled {
                    Some("Hook 已启用".to_string())
                } else {
                    unavailable_reason.clone()
                },
            },
            AgentHookAction {
                action: AgentHookActionKind::Disable,
                available: can_write && self.installed && self.enabled,
                reason: installed_reason.clone(),
            },
            AgentHookAction {
                action: AgentHookActionKind::Remove,
                available: self.installed
                    && self.ownership.is_some()
                    && !matches!(self.state, AgentHookHealthState::Conflict),
                reason: if !self.installed {
                    Some("Hook 尚未安装".to_string())
                } else if self.ownership.is_none() {
                    Some("缺少 BalanceHub ownership，不能删除".to_string())
                } else if matches!(self.state, AgentHookHealthState::Conflict) {
                    Some("配置冲突，不能删除".to_string())
                } else {
                    None
                },
            },
            AgentHookAction {
                action: AgentHookActionKind::Health,
                available: true,
                reason: None,
            },
            AgentHookAction {
                action: AgentHookActionKind::Verify,
                available: self.installed,
                reason: if self.installed {
                    None
                } else {
                    Some("Hook 尚未安装".to_string())
                },
            },
            AgentHookAction {
                action: AgentHookActionKind::Repair,
                available: installation_available && self.installed && repair_needed,
                reason: if !installation_available {
                    install_reason
                } else if !self.installed {
                    Some("没有需要修复的已安装 Hook".to_string())
                } else if !repair_needed {
                    Some("当前 Hook 状态不需要修复".to_string())
                } else {
                    None
                },
            },
        ];
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHookPlan {
    pub agent_kind: AgentCliKind,
    pub mutation: AgentHookMutation,
    pub runtime_scope: AgentRuntimeScope,
    pub config_path: String,
    pub expected_revision: String,
    pub supported: bool,
    pub conflict: bool,
    pub changes: Vec<AgentHookChange>,
    pub summary: String,
}
