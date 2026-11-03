use super::format::{definition_fingerprint, definitions, find_resources};
use super::ownership::{read_config, read_manifest};
use super::CodexHookService;
use crate::models::{
    AgentCliKind, AgentHookChange, AgentHookChangeKind, AgentHookHealthState, AgentHookMutation,
    AgentHookPlan, AgentRuntimeScope,
};

impl CodexHookService {
    pub fn plan(&self, mutation: AgentHookMutation) -> AgentHookPlan {
        let inspection = self.inspect();
        let mut changes = Vec::new();
        let manifest = read_manifest(&self.manifest_path).ok().flatten();
        let config = read_config(&self.config_path);
        let found = config
            .snapshot
            .as_ref()
            .map(|snapshot| find_resources(&snapshot.value))
            .unwrap_or_default();
        let definitions = definitions(&self.helper_path, &self.spool_root);
        let resources = manifest
            .as_ref()
            .map(|manifest| manifest.resources.as_slice())
            .unwrap_or(&[]);
        let conflict = inspection.state == AgentHookHealthState::Conflict
            || (manifest.as_ref().is_some_and(|manifest| manifest.enabled)
                && !inspection.config_exists);
        match mutation {
            AgentHookMutation::Install | AgentHookMutation::Enable => {
                for definition in &definitions {
                    let present = found.iter().find(|item| {
                        item.event_name == definition.event_name
                            && item.structural_identity == definition.identity
                    });
                    changes.push(match present {
                        Some(item) if item.fingerprint == definition_fingerprint(definition) => {
                            AgentHookChange {
                                event_name: definition.event_name.to_string(),
                                structural_identity: definition.identity.clone(),
                                fingerprint: item.fingerprint.clone(),
                                kind: AgentHookChangeKind::Keep,
                            }
                        }
                        _ => AgentHookChange {
                            event_name: definition.event_name.to_string(),
                            structural_identity: definition.identity.clone(),
                            fingerprint: definition_fingerprint(definition),
                            kind: AgentHookChangeKind::Add,
                        },
                    });
                }
            }
            AgentHookMutation::Disable | AgentHookMutation::Remove => {
                for resource in resources {
                    let kind = found.iter().any(|item| {
                        item.event_name == resource.event_name
                            && item.structural_identity == resource.structural_identity
                            && item.fingerprint == resource.content_fingerprint
                    });
                    changes.push(AgentHookChange {
                        event_name: resource.event_name.clone(),
                        structural_identity: resource.structural_identity.clone(),
                        fingerprint: resource.content_fingerprint.clone(),
                        kind: if kind {
                            AgentHookChangeKind::Remove
                        } else {
                            AgentHookChangeKind::Keep
                        },
                    });
                }
            }
        }
        let action_word = match mutation {
            AgentHookMutation::Install => "安装",
            AgentHookMutation::Remove => "删除",
            AgentHookMutation::Enable => "启用",
            AgentHookMutation::Disable => "禁用",
        };
        let changed = changes
            .iter()
            .filter(|change| change.kind != AgentHookChangeKind::Keep)
            .count();
        let (content_changes, preview_error) = if conflict {
            (Vec::new(), None)
        } else {
            match self.prepare_configuration(mutation, &config, manifest.as_ref(), changed > 0) {
                Ok(prepared) => {
                    let mut contents = prepared.content_changes(
                        &self.config_path,
                        config
                            .snapshot
                            .as_ref()
                            .map(|snapshot| snapshot.text.as_str()),
                    );
                    super::super::common::append_state_change(
                        &mut contents,
                        inspection.installed,
                        inspection.enabled,
                        mutation,
                    );
                    (contents, None)
                }
                Err(error) => (Vec::new(), Some(error)),
            }
        };
        let has_content_changes = !content_changes.is_empty();
        AgentHookPlan {
            agent_kind: AgentCliKind::Codex,
            mutation,
            runtime_scope: AgentRuntimeScope::Native,
            config_path: self.config_path.to_string_lossy().into_owned(),
            expected_revision: inspection.revision,
            supported: preview_error.is_none(),
            conflict,
            changes,
            content_changes,
            summary: if let Some(error) = preview_error {
                error
            } else if conflict {
                "检测到配置或所有权冲突，未生成可应用变更".to_string()
            } else if changed == 0 && !has_content_changes {
                format!("无需{action_word}，当前状态已满足请求")
            } else if changed == 0 {
                format!("确认后将{action_word} Codex 会话接入")
            } else {
                format!("确认后将{action_word} {changed} 个 Codex Hook 节点")
            },
        }
    }
}
