use super::format::find_resources;
use super::ownership::{
    is_regular_file, is_safe_directory, latest_event_after, read_config, read_manifest,
    revision_for_missing, ConfigStatus,
};
use super::CodexHookService;
use crate::models::{
    AgentCliKind, AgentHookHealthState, AgentHookInspection, AgentHookTrust, AgentRuntimeScope,
};

impl CodexHookService {
    pub fn inspect(&self) -> AgentHookInspection {
        let config = read_config(&self.config_path);
        let manifest = read_manifest(&self.manifest_path);
        let helper_available = is_regular_file(&self.helper_path);
        let spool_available = is_safe_directory(&self.spool_root);
        let mut diagnostics = Vec::new();
        if let Some(message) = config.diagnostic.clone() {
            diagnostics.push(message);
        }
        if let Err(message) = &manifest {
            diagnostics.push(message.clone());
        }
        if !helper_available {
            diagnostics.push("BalanceHub Hook helper 不可用，请重新安装或启动当前版本".to_string());
        }
        if !spool_available {
            diagnostics.push("BalanceHub 数据目录不可用，Hook 将保持 fail-open".to_string());
        }

        let revision = config
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.revision.clone())
            .unwrap_or_else(revision_for_missing);
        let ownership = manifest.as_ref().ok().and_then(Clone::clone);
        let mut installed = false;
        let mut enabled = false;
        let mut conflict = config.status == ConfigStatus::Unsupported
            || config.status == ConfigStatus::Unsafe
            || manifest.is_err();
        let mut last_event_at = None;
        if let (Some(snapshot), Some(ownership_ref)) =
            (config.snapshot.as_ref(), ownership.as_ref())
        {
            if ownership_ref.agent_kind != AgentCliKind::Codex
                || ownership_ref.config_path != self.config_path.to_string_lossy()
            {
                conflict = true;
                diagnostics.push("Hook ownership manifest 与当前配置路径不匹配".to_string());
            } else {
                let found = find_resources(&snapshot.value);
                let mut all_present = true;
                for owned in &ownership_ref.resources {
                    let matching = found.iter().find(|item| {
                        item.event_name == owned.event_name
                            && item.structural_identity == owned.structural_identity
                    });
                    match matching {
                        Some(item) if item.fingerprint == owned.content_fingerprint => {}
                        Some(_) => {
                            conflict = true;
                            diagnostics.push(format!(
                                "Hook {} 已被其他修改，未覆盖用户变更",
                                owned.event_name
                            ));
                        }
                        None => {
                            all_present = false;
                            if ownership_ref.enabled {
                                conflict = true;
                                diagnostics.push(format!(
                                    "已启用的 Hook {} 缺失，未自动恢复",
                                    owned.event_name
                                ));
                            }
                        }
                    }
                }
                let unexpected_owned = found.iter().any(|item| {
                    !ownership_ref.resources.iter().any(|owned| {
                        owned.event_name == item.event_name
                            && owned.structural_identity == item.structural_identity
                            && owned.content_fingerprint == item.fingerprint
                    })
                });
                if unexpected_owned {
                    conflict = true;
                    diagnostics.push("检测到未登记的 BalanceHub Hook 节点，未自动接管".to_string());
                }
                installed = !ownership_ref.resources.is_empty();
                enabled = installed && all_present && !conflict;
                if enabled {
                    last_event_at =
                        latest_event_after(&self.spool_root, ownership_ref.installed_at);
                }
            }
        } else if let Some(snapshot) = config.snapshot.as_ref() {
            let found = find_resources(&snapshot.value);
            if !found.is_empty() {
                conflict = true;
                diagnostics.push(
                    "发现 BalanceHub Hook 节点但缺少 ownership manifest，未删除或覆盖".to_string(),
                );
            }
        }

        let state = if conflict {
            AgentHookHealthState::Conflict
        } else if !installed {
            AgentHookHealthState::NotInstalled
        } else if !enabled {
            AgentHookHealthState::Disabled
        } else if !helper_available {
            AgentHookHealthState::HelperMissing
        } else if !spool_available {
            AgentHookHealthState::SpoolBlocked
        } else if last_event_at.is_some() {
            AgentHookHealthState::Healthy
        } else {
            AgentHookHealthState::InstalledUnverified
        };
        let mut inspection = AgentHookInspection {
            agent_kind: AgentCliKind::Codex,
            runtime_scope: AgentRuntimeScope::Native,
            config_path: self.config_path.to_string_lossy().into_owned(),
            config_exists: config.status == ConfigStatus::Present,
            revision,
            state,
            installed,
            enabled,
            // Codex does not expose a stable trust-store fact through this
            // adapter. File presence and real event delivery are observable;
            // trust itself must remain unknown instead of being inferred.
            trusted: AgentHookTrust::Unknown,
            helper_available,
            spool_available,
            last_event_at,
            ownership,
            diagnostics,
            actions: Vec::new(),
        };
        inspection.set_actions(true);
        inspection
    }
}
