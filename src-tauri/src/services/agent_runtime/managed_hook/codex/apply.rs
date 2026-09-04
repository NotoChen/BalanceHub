use super::format::{
    add_definition, definition_fingerprint, definitions, remove_owned_definitions,
};
use super::ownership::{
    encode_config, ensure_managed_parent, ensure_parent, now_millis, read_config, read_manifest,
    remove_manifest, write_atomic, write_manifest, ConfigStatus,
};
use super::{CodexHookService, HELPER_VERSION, MUTATION_LOCK};
use crate::models::{
    AgentCliKind, AgentHookInspection, AgentHookMutation, AgentHookOwnedResource,
    AgentHookOwnership, AgentHookPlan, AgentRuntimeScope,
};
use serde_json::json;
use std::sync::Mutex;

impl CodexHookService {
    pub fn apply(&self, plan: AgentHookPlan) -> Result<AgentHookInspection, String> {
        if !plan.supported
            || plan.conflict
            || plan.agent_kind != AgentCliKind::Codex
            || plan.runtime_scope != AgentRuntimeScope::Native
            || plan.config_path != self.config_path.to_string_lossy()
        {
            return Err("Hook 计划存在冲突或不受支持，未修改配置".to_string());
        }
        let _guard = MUTATION_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let current = self.inspect();
        if current.revision != plan.expected_revision {
            return Err("Codex Hook 配置已变化，请重新生成计划后再试".to_string());
        }
        let current_plan = self.plan(plan.mutation);
        if current_plan.conflict
            || current_plan.expected_revision != plan.expected_revision
            || current_plan.changes != plan.changes
        {
            return Err("Codex Hook 配置或所有权已变化，请重新生成计划后再试".to_string());
        }
        let config = read_config(&self.config_path);
        if config.status == ConfigStatus::Unsupported || config.status == ConfigStatus::Unsafe {
            return Err(config
                .diagnostic
                .unwrap_or_else(|| "Codex Hook 配置格式不受支持".to_string()));
        }
        let mut value = config
            .snapshot
            .map(|snapshot| snapshot.value)
            .unwrap_or_else(|| json!({}));
        let definitions = definitions(&self.helper_path, &self.spool_root);
        match plan.mutation {
            AgentHookMutation::Install | AgentHookMutation::Enable => {
                for definition in &definitions {
                    add_definition(&mut value, definition)?;
                }
                let installed_at = read_manifest(&self.manifest_path)
                    .ok()
                    .flatten()
                    .map(|manifest| manifest.installed_at)
                    .unwrap_or_else(now_millis);
                let ownership = AgentHookOwnership {
                    agent_kind: AgentCliKind::Codex,
                    runtime_scope: AgentRuntimeScope::Native,
                    config_path: self.config_path.to_string_lossy().into_owned(),
                    helper_version: HELPER_VERSION.to_string(),
                    installed_at,
                    enabled: true,
                    resources: definitions
                        .iter()
                        .map(|definition| AgentHookOwnedResource {
                            event_name: definition.event_name.to_string(),
                            structural_identity: definition.identity.clone(),
                            content_fingerprint: definition_fingerprint(definition),
                        })
                        .collect(),
                };
                let bytes = encode_config(&value)?;
                let config_needs_write = current_plan
                    .changes
                    .iter()
                    .any(|change| change.kind != crate::models::AgentHookChangeKind::Keep);
                if config_needs_write {
                    ensure_parent(&self.config_path)?;
                    write_atomic(&self.config_path, &bytes)?;
                }
                ensure_managed_parent(&self.manifest_path)?;
                write_manifest(&self.manifest_path, &ownership)?;
            }
            AgentHookMutation::Disable | AgentHookMutation::Remove => {
                let Some(mut ownership) = read_manifest(&self.manifest_path)
                    .map_err(|error| format!("无法读取 Hook ownership manifest: {error}"))?
                else {
                    if config.status == ConfigStatus::Missing {
                        return Ok(self.inspect());
                    }
                    return Err("缺少 Hook ownership manifest，未删除任何配置".to_string());
                };
                remove_owned_definitions(&mut value, &ownership)?;
                let preserve_disabled_bytes =
                    plan.mutation == AgentHookMutation::Remove && !ownership.enabled;
                if config.status == ConfigStatus::Present && !preserve_disabled_bytes {
                    let bytes = encode_config(&value)?;
                    write_atomic(&self.config_path, &bytes)?;
                }
                if plan.mutation == AgentHookMutation::Disable {
                    ownership.enabled = false;
                    write_manifest(&self.manifest_path, &ownership)?;
                } else {
                    remove_manifest(&self.manifest_path)?;
                }
            }
        }
        Ok(self.inspect())
    }
}
