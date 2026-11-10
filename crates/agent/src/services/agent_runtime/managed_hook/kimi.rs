//! Kimi Code user-scope Hook adapter.
//!
//! Kimi Code keeps hooks as a TOML array table in `config.toml`:
//!
//! ```toml
//! [[hooks]]
//! event = "SessionStart"
//! command = "..."
//! timeout = 10
//! ```
//!
//! The native schema has no rule name and rejects unknown fields, so
//! ownership is carried by the helper's `--balancehub-hook-node=` argument in
//! `command`.  Edits use `toml_edit` to preserve comments, ordering and the
//! formatting of unrelated hooks.

use super::common;
use crate::models::{
    AgentAssetPlanChange, AgentCliKind, AgentHookChange, AgentHookChangeKind, AgentHookHealthState,
    AgentHookInspection, AgentHookMutation, AgentHookOwnedResource, AgentHookOwnership,
    AgentHookPlan, AgentHookTrust, AgentRuntimeScope,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use toml_edit::{value as toml_value, ArrayOfTables, Document, Item, Table};

const HELPER_VERSION: &str = "kimi-code-hook-v1";
const EVENTS: [&str; 4] = ["SessionStart", "UserPromptSubmit", "Stop", "SessionEnd"];
const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;
const HOOK_NODE_FLAG: &str = "--balancehub-hook-node=";
const IDENTITY_PREFIX: &str = "balancehub:kimiCode:";

#[derive(Debug, Clone)]
pub struct KimiHookService {
    config_path: PathBuf,
    manifest_path: PathBuf,
    helper_path: PathBuf,
    spool_root: PathBuf,
}

#[derive(Clone)]
struct Definition {
    event_name: &'static str,
    identity: String,
    fingerprint: String,
    table: Table,
}

impl KimiHookService {
    pub fn from_host() -> Result<Self, String> {
        let home = common::home_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
        let app_data = balancehub_core::host::data_dir()
            .map_err(|error| format!("无法定位 BalanceHub 数据目录: {error}"))?;
        Ok(Self {
            config_path: common::config_path_for(&home, AgentCliKind::KimiCode),
            manifest_path: app_data
                .join("agent-hooks")
                .join(AgentCliKind::KimiCode.key())
                .join("ownership.json"),
            helper_path: std::env::current_exe()
                .map_err(|error| format!("无法定位 BalanceHub 可执行文件: {error}"))?,
            spool_root: app_data,
        })
    }

    fn definitions(&self) -> Vec<Definition> {
        EVENTS
            .iter()
            .map(|event_name| {
                let identity = format!("{IDENTITY_PREFIX}{}:v1", event_name.to_ascii_lowercase());
                let command = common::helper_command(
                    &self.helper_path,
                    &self.spool_root,
                    AgentCliKind::KimiCode.key(),
                    &identity,
                );
                let mut table = Table::new();
                table["event"] = toml_value(*event_name);
                table["command"] = toml_value(command);
                table["timeout"] = toml_value(10);
                let fingerprint = fingerprint(&table);
                Definition {
                    event_name,
                    identity,
                    fingerprint,
                    table,
                }
            })
            .collect()
    }

    pub fn inspect(&self) -> AgentHookInspection {
        let config = read_config(&self.config_path);
        let manifest_result = common::read_manifest(&self.manifest_path);
        let ownership = manifest_result.as_ref().ok().and_then(Clone::clone);
        let helper_available = common::is_regular_file(&self.helper_path);
        let spool_available = common::is_safe_directory(&self.spool_root);
        let mut diagnostics = Vec::new();
        if let Some(message) = config.diagnostic.clone() {
            diagnostics.push(message);
        }
        if let Err(error) = &manifest_result {
            diagnostics.push(error.clone());
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
            .unwrap_or_else(common::revision_for_missing);
        let mut conflict = config.status == common::ConfigStatus::Unsupported
            || config.status == common::ConfigStatus::Unsafe
            || manifest_result.is_err();
        let mut installed = false;
        let mut enabled = false;
        let mut last_event_at = None;
        if let (Some(snapshot), Some(owned)) = (config.snapshot.as_ref(), ownership.as_ref()) {
            if owned.agent_kind != AgentCliKind::KimiCode
                || owned.config_path != self.config_path.to_string_lossy()
            {
                conflict = true;
                diagnostics.push("Hook ownership manifest 与当前配置路径不匹配".to_string());
            } else {
                let found = find_resources(&snapshot.document);
                let mut all_present = true;
                for owned_resource in &owned.resources {
                    match found.iter().find(|item| {
                        item.event_name == owned_resource.event_name
                            && item.identity == owned_resource.structural_identity
                    }) {
                        Some(item) if item.fingerprint == owned_resource.content_fingerprint => {}
                        Some(_) => {
                            conflict = true;
                            diagnostics.push(format!(
                                "Hook {} 已被其他修改，未覆盖用户变更",
                                owned_resource.event_name
                            ));
                        }
                        None => {
                            all_present = false;
                            if owned.enabled {
                                conflict = true;
                                diagnostics.push(format!(
                                    "已启用的 Hook {} 缺失，未自动恢复",
                                    owned_resource.event_name
                                ));
                            }
                        }
                    }
                }
                if found.iter().any(|item| {
                    item.identity.starts_with(IDENTITY_PREFIX)
                        && !owned.resources.iter().any(|owned| {
                            owned.structural_identity == item.identity
                                && owned.content_fingerprint == item.fingerprint
                        })
                }) {
                    conflict = true;
                    diagnostics.push("检测到未登记的 BalanceHub Hook 节点，未自动接管".to_string());
                }
                installed = !owned.resources.is_empty();
                enabled = installed && all_present && owned.enabled && !conflict;
                if enabled {
                    last_event_at = common::latest_event_after(
                        &self.spool_root,
                        AgentCliKind::KimiCode,
                        owned.installed_at,
                    );
                }
            }
        } else if let Some(snapshot) = config.snapshot.as_ref() {
            if !find_resources(&snapshot.document).is_empty() {
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
            agent_kind: AgentCliKind::KimiCode,
            runtime_scope: AgentRuntimeScope::Native,
            config_path: self.config_path.to_string_lossy().into_owned(),
            config_exists: config.status == common::ConfigStatus::Present,
            revision,
            state,
            installed,
            enabled,
            trusted: AgentHookTrust::NotApplicable,
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

    pub fn plan(&self, mutation: AgentHookMutation) -> AgentHookPlan {
        let inspection = self.inspect();
        let manifest = common::read_manifest(&self.manifest_path).ok().flatten();
        let config = read_config(&self.config_path);
        let found = config
            .snapshot
            .as_ref()
            .map(|snapshot| find_resources(&snapshot.document))
            .unwrap_or_default();
        let definitions = self.definitions();
        let resources = manifest
            .as_ref()
            .map(|manifest| manifest.resources.as_slice())
            .unwrap_or(&[]);
        let conflict = inspection.state == AgentHookHealthState::Conflict
            || (manifest.as_ref().is_some_and(|manifest| manifest.enabled)
                && config.status == common::ConfigStatus::Missing);
        let mut changes = Vec::new();
        for definition in &definitions {
            let present = found.iter().find(|item| {
                item.event_name == definition.event_name && item.identity == definition.identity
            });
            let kind = match present {
                Some(item) if item.fingerprint == definition.fingerprint => {
                    AgentHookChangeKind::Keep
                }
                _ => AgentHookChangeKind::Add,
            };
            changes.push(AgentHookChange {
                event_name: definition.event_name.to_string(),
                structural_identity: definition.identity.clone(),
                fingerprint: definition.fingerprint.clone(),
                kind: if matches!(
                    mutation,
                    AgentHookMutation::Remove | AgentHookMutation::Disable
                ) {
                    if present.is_some()
                        && resources
                            .iter()
                            .any(|resource| resource.event_name == definition.event_name)
                    {
                        AgentHookChangeKind::Remove
                    } else {
                        AgentHookChangeKind::Keep
                    }
                } else {
                    kind
                },
            });
        }
        let action = match mutation {
            AgentHookMutation::Install => "安装",
            AgentHookMutation::Remove => "删除",
            AgentHookMutation::Enable => "启用",
            AgentHookMutation::Disable => "禁用",
        };
        let structural_changes = changes
            .iter()
            .filter(|change| change.kind != AgentHookChangeKind::Keep)
            .count();
        let state_change = match mutation {
            AgentHookMutation::Disable => usize::from(inspection.enabled),
            AgentHookMutation::Enable => usize::from(!inspection.enabled && inspection.installed),
            _ => 0,
        };
        let changed = structural_changes + state_change;
        let (content_changes, preview_error) = if conflict {
            (Vec::new(), None)
        } else {
            match self.prepare(mutation, &config, &definitions, structural_changes > 0) {
                Ok(after) => {
                    let mut contents =
                        content_changes(&self.config_path, config.snapshot.as_ref(), after);
                    common::append_state_change(
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
            agent_kind: AgentCliKind::KimiCode,
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
                format!("无需{action}，当前状态已满足请求")
            } else if changed == 0 {
                format!("确认后将{action} Kimi Code 会话接入")
            } else {
                format!("确认后将{action} {changed} 个 Kimi Code Hook 节点")
            },
        }
    }

    pub fn apply(&self, plan: AgentHookPlan) -> Result<AgentHookInspection, String> {
        if !plan.supported
            || plan.conflict
            || plan.agent_kind != AgentCliKind::KimiCode
            || plan.runtime_scope != AgentRuntimeScope::Native
            || plan.config_path != self.config_path.to_string_lossy()
        {
            return Err("Hook 计划存在冲突或不受支持，未修改配置".to_string());
        }
        let parent = self.config_path.parent().ok_or("Hook 配置目录无效")?;
        super::locking::with_locked_sources(&self.config_path, &self.manifest_path, parent, || {
            self.apply_locked(plan)
        })
    }

    fn apply_locked(&self, plan: AgentHookPlan) -> Result<AgentHookInspection, String> {
        let current = self.inspect();
        if current.revision != plan.expected_revision {
            return Err("Kimi Code Hook 配置已变化，请重新生成计划后再试".to_string());
        }
        let current_plan = self.plan(plan.mutation);
        if !current_plan.supported
            || current_plan.conflict
            || current_plan.expected_revision != plan.expected_revision
            || current_plan.changes != plan.changes
            || current_plan.content_changes != plan.content_changes
        {
            return Err("Hook 配置或所有权已变化，请重新生成计划后再试".to_string());
        }
        let config = read_config(&self.config_path);
        let manifest = common::read_manifest(&self.manifest_path)?;
        let definitions = self.definitions();
        let structural = current_plan
            .changes
            .iter()
            .any(|change| change.kind != AgentHookChangeKind::Keep);
        let after = self.prepare(plan.mutation, &config, &definitions, structural)?;
        let actual = content_changes(&self.config_path, config.snapshot.as_ref(), after.clone());
        let approved = plan
            .content_changes
            .iter()
            .filter(|change| change.path.is_some())
            .cloned()
            .collect::<Vec<_>>();
        if actual != approved {
            return Err("Hook 配置已变化，请重新预览内容差异".to_string());
        }
        match plan.mutation {
            AgentHookMutation::Install | AgentHookMutation::Enable => {
                let installed_at = manifest
                    .as_ref()
                    .map(|manifest| manifest.installed_at)
                    .unwrap_or_else(common::now_millis);
                let ownership = AgentHookOwnership {
                    agent_kind: AgentCliKind::KimiCode,
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
                            content_fingerprint: definition.fingerprint.clone(),
                        })
                        .collect(),
                };
                if let Some(after) = after {
                    common::ensure_managed_parent(&self.config_path)?;
                    common::write_atomic(&self.config_path, after.as_bytes())?;
                }
                common::ensure_managed_parent(&self.manifest_path)?;
                common::write_manifest(&self.manifest_path, &ownership)?;
            }
            AgentHookMutation::Disable => {
                let mut ownership =
                    manifest.ok_or("缺少 Hook ownership manifest，未修改任何配置")?;
                if let Some(after) = after {
                    common::ensure_managed_parent(&self.config_path)?;
                    common::write_atomic(&self.config_path, after.as_bytes())?;
                }
                ownership.enabled = false;
                common::ensure_managed_parent(&self.manifest_path)?;
                common::write_manifest(&self.manifest_path, &ownership)?;
            }
            AgentHookMutation::Remove => {
                manifest.ok_or("缺少 Hook ownership manifest，未删除任何配置")?;
                if let Some(after) = after {
                    common::ensure_managed_parent(&self.config_path)?;
                    common::write_atomic(&self.config_path, after.as_bytes())?;
                }
                common::remove_manifest(&self.manifest_path)?;
            }
        }
        Ok(self.inspect())
    }

    fn prepare(
        &self,
        mutation: AgentHookMutation,
        config: &ConfigRead,
        definitions: &[Definition],
        structural_changes: bool,
    ) -> Result<Option<String>, String> {
        if !structural_changes {
            return Ok(None);
        }
        let mut document = match config.snapshot.as_ref() {
            Some(snapshot) => snapshot
                .text
                .parse::<Document>()
                .map_err(|_| "Kimi Code 配置不是有效的 TOML".to_string())?,
            None => Document::new(),
        };
        match mutation {
            AgentHookMutation::Install | AgentHookMutation::Enable => {
                for definition in definitions {
                    add_definition(&mut document, definition)?;
                }
            }
            AgentHookMutation::Disable | AgentHookMutation::Remove => {
                for definition in definitions {
                    remove_definition(&mut document, definition)?;
                }
            }
        }
        let after = document.to_string();
        let before = config
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.text.as_str())
            .unwrap_or_default();
        Ok((after != before).then_some(after))
    }
}

struct ConfigSnapshot {
    document: Document,
    text: String,
    revision: String,
}

struct ConfigRead {
    status: common::ConfigStatus,
    snapshot: Option<ConfigSnapshot>,
    diagnostic: Option<String>,
}

fn read_config(path: &Path) -> ConfigRead {
    if !path.is_absolute() {
        return ConfigRead {
            status: common::ConfigStatus::Unsafe,
            snapshot: None,
            diagnostic: Some("Kimi Code 配置路径必须是绝对路径".to_string()),
        };
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return ConfigRead {
                status: common::ConfigStatus::Missing,
                snapshot: None,
                diagnostic: None,
            }
        }
        Err(error) => {
            return ConfigRead {
                status: common::ConfigStatus::Unsafe,
                snapshot: None,
                diagnostic: Some(format!("无法读取 Kimi Code 配置: {error}")),
            }
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return ConfigRead {
            status: common::ConfigStatus::Unsafe,
            snapshot: None,
            diagnostic: Some("Kimi Code 配置不是普通文件，已拒绝访问".to_string()),
        };
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return ConfigRead {
            status: common::ConfigStatus::Unsupported,
            snapshot: None,
            diagnostic: Some("Kimi Code 配置文件超过读取上限".to_string()),
        };
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return ConfigRead {
                status: common::ConfigStatus::Unsafe,
                snapshot: None,
                diagnostic: Some(format!("无法读取 Kimi Code 配置: {error}")),
            }
        }
    };
    let text = match String::from_utf8(bytes.clone()) {
        Ok(text) => text,
        Err(_) => {
            return ConfigRead {
                status: common::ConfigStatus::Unsupported,
                snapshot: None,
                diagnostic: Some("Kimi Code 配置不是 UTF-8 文本".to_string()),
            }
        }
    };
    match text.parse::<Document>() {
        Ok(document) => ConfigRead {
            status: common::ConfigStatus::Present,
            snapshot: Some(ConfigSnapshot {
                document,
                text,
                revision: common::revision_for_bytes(&bytes),
            }),
            diagnostic: None,
        },
        Err(error) => ConfigRead {
            status: common::ConfigStatus::Unsupported,
            snapshot: None,
            diagnostic: Some(format!("Kimi Code 配置 TOML 无法解析: {error}")),
        },
    }
}

struct FoundResource {
    event_name: String,
    identity: String,
    fingerprint: String,
}

fn find_resources(document: &Document) -> Vec<FoundResource> {
    let Some(hooks) = document.get("hooks").and_then(Item::as_array_of_tables) else {
        return Vec::new();
    };
    hooks
        .iter()
        .filter_map(|table| {
            let command = table.get("command").and_then(Item::as_str)?;
            let identity = identity_from_command(command)?;
            if !identity.starts_with(IDENTITY_PREFIX) {
                return None;
            }
            Some(FoundResource {
                event_name: table
                    .get("event")
                    .and_then(Item::as_str)
                    .unwrap_or_default()
                    .to_string(),
                identity: identity.to_string(),
                fingerprint: fingerprint(table),
            })
        })
        .collect()
}

fn add_definition(document: &mut Document, definition: &Definition) -> Result<(), String> {
    if document.get("hooks").is_none() {
        document.insert("hooks", Item::ArrayOfTables(ArrayOfTables::new()));
    }
    let hooks = document
        .get_mut("hooks")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or("Kimi Code 的 hooks 必须是 [[hooks]] 数组表")?;
    if let Some(existing) = hooks.iter().find(|table| {
        table
            .get("command")
            .and_then(Item::as_str)
            .and_then(identity_from_command)
            == Some(definition.identity.as_str())
    }) {
        if fingerprint(existing) == definition.fingerprint {
            return Ok(());
        }
        return Err(format!(
            "Hook {} 已被修改，无法安全覆盖",
            definition.event_name
        ));
    }
    hooks.push(definition.table.clone());
    Ok(())
}

fn remove_definition(document: &mut Document, definition: &Definition) -> Result<bool, String> {
    let Some(hooks) = document
        .get_mut("hooks")
        .and_then(Item::as_array_of_tables_mut)
    else {
        return Ok(false);
    };
    let mut kept = Vec::with_capacity(hooks.len());
    let mut changed = false;
    for table in hooks.iter() {
        let matched = table
            .get("command")
            .and_then(Item::as_str)
            .and_then(identity_from_command)
            == Some(definition.identity.as_str());
        if matched {
            if fingerprint(table) != definition.fingerprint {
                return Err(format!(
                    "Hook {} 已被修改，无法安全移除",
                    definition.event_name
                ));
            }
            changed = true;
            continue;
        }
        kept.push(table.clone());
    }
    if changed {
        hooks.clear();
        for table in kept {
            hooks.push(table);
        }
    }
    Ok(changed)
}

fn content_changes(
    path: &Path,
    snapshot: Option<&ConfigSnapshot>,
    after: Option<String>,
) -> Vec<AgentAssetPlanChange> {
    let before = snapshot.map(|snapshot| snapshot.text.as_str());
    let Some(after) = after else {
        return Vec::new();
    };
    if before == Some(after.as_str()) {
        return Vec::new();
    }
    vec![AgentAssetPlanChange {
        label: "Hook 配置".to_string(),
        path: Some(path.to_string_lossy().into_owned()),
        before: before.map(str::to_owned),
        after: Some(after),
    }]
}

fn fingerprint(table: &Table) -> String {
    let value = serde_json::json!({
        "event": table.get("event").and_then(Item::as_str),
        "matcher": table.get("matcher").and_then(Item::as_str),
        "command": table.get("command").and_then(Item::as_str),
        "timeout": table.get("timeout").and_then(Item::as_integer),
    });
    common::revision_for_bytes(serde_json::to_string(&value).unwrap_or_default().as_bytes())
}

fn identity_from_command(command: &str) -> Option<&str> {
    let start = command.find(HOOK_NODE_FLAG)? + HOOK_NODE_FLAG.len();
    let rest = &command[start..];
    let end = rest
        .find(|character: char| character.is_whitespace() || character == '\'' || character == '"')
        .unwrap_or(rest.len());
    rest.get(..end).filter(|identity| !identity.is_empty())
}

/// Catalog edits rewrite the whole TOML document. Ownership is released only
/// when an exact owned rule disappears from the edited document.
pub(super) fn owned_resources_changed(
    before: &[u8],
    after: &[u8],
    ownership: &AgentHookOwnership,
) -> bool {
    let before = parse_document(before);
    let after = parse_document(after);
    let before = before.as_ref().map(find_resources).unwrap_or_default();
    let after = after.as_ref().map(find_resources).unwrap_or_default();
    ownership.resources.iter().any(|owned| {
        let matches = |resource: &FoundResource| {
            resource.event_name == owned.event_name
                && resource.identity == owned.structural_identity
                && resource.fingerprint == owned.content_fingerprint
        };
        before.iter().filter(|resource| matches(resource)).count() == 1
            && after.iter().filter(|resource| matches(resource)).count() != 1
    })
}

fn parse_document(bytes: &[u8]) -> Option<Document> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.parse::<Document>().ok())
}
