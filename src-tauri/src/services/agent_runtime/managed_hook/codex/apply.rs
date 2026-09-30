use super::format::{definition_fingerprint, definitions};
use super::ownership::{
    ensure_managed_parent, ensure_parent, now_millis, read_config, read_manifest, remove_manifest,
    write_atomic, write_manifest, ConfigStatus,
};
use super::{CodexHookService, HELPER_VERSION};
use crate::models::{
    AgentCliKind, AgentHookInspection, AgentHookMutation, AgentHookOwnedResource,
    AgentHookOwnership, AgentHookPlan, AgentRuntimeScope,
};

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
        super::super::locking::with_locked_sources(
            &self.config_path,
            &self.manifest_path,
            self.config_path.parent().ok_or("Hook 配置目录无效")?,
            || self.apply_locked(plan),
        )
    }

    fn apply_locked(&self, plan: AgentHookPlan) -> Result<AgentHookInspection, String> {
        let current = self.inspect();
        if current.revision != plan.expected_revision {
            return Err("Codex Hook 配置已变化，请重新生成计划后再试".to_string());
        }
        let current_plan = self.plan(plan.mutation);
        if !current_plan.supported
            || current_plan.conflict
            || current_plan.expected_revision != plan.expected_revision
            || current_plan.changes != plan.changes
            || current_plan.content_changes != plan.content_changes
        {
            return Err("Codex Hook 配置或所有权已变化，请重新生成计划后再试".to_string());
        }
        let config = read_config(&self.config_path);
        let manifest = read_manifest(&self.manifest_path)?;
        let prepared = self.prepare_configuration(
            plan.mutation,
            &config,
            manifest.as_ref(),
            current_plan
                .changes
                .iter()
                .any(|change| change.kind != crate::models::AgentHookChangeKind::Keep),
        )?;
        let definitions = definitions(&self.helper_path, &self.spool_root);
        prepared.verify_preview(
            &self.config_path,
            config
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.text.as_str()),
            &plan.content_changes,
        )?;
        match plan.mutation {
            AgentHookMutation::Install | AgentHookMutation::Enable => {
                let installed_at = manifest
                    .as_ref()
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
                if prepared.write {
                    ensure_parent(&self.config_path)?;
                    write_atomic(
                        &self.config_path,
                        prepared
                            .after
                            .as_deref()
                            .ok_or("缺少待写入的 Hook 配置")?
                            .as_bytes(),
                    )?;
                }
                ensure_managed_parent(&self.manifest_path)?;
                write_manifest(&self.manifest_path, &ownership)?;
            }
            AgentHookMutation::Disable | AgentHookMutation::Remove => {
                let Some(mut ownership) = manifest else {
                    if config.status == ConfigStatus::Missing {
                        return Ok(self.inspect());
                    }
                    return Err("缺少 Hook ownership manifest，未删除任何配置".to_string());
                };
                if prepared.write {
                    write_atomic(
                        &self.config_path,
                        prepared
                            .after
                            .as_deref()
                            .ok_or("缺少待写入的 Hook 配置")?
                            .as_bytes(),
                    )?;
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
