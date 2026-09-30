//! Schema-specific managed Hook adapters for Claude Code, Gemini CLI and Grok Build.

mod configuration;

use super::common;
use crate::models::{
    AgentCliKind, AgentHookChange, AgentHookChangeKind, AgentHookHealthState, AgentHookInspection,
    AgentHookMutation, AgentHookOwnedResource, AgentHookOwnership, AgentHookPlan, AgentHookTrust,
    AgentRuntimeScope,
};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

const CLAUDE_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "Stop",
    "StopFailure",
    "SessionEnd",
];
const GEMINI_EVENTS: &[&str] = &["SessionStart", "BeforeAgent", "AfterAgent", "SessionEnd"];
const GROK_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "Stop",
    "StopFailure",
    "SessionEnd",
];
const HELPER_VERSION: &str = "agent-hook-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GenericAgent {
    ClaudeCode,
    Gemini,
    Grok,
}

impl GenericAgent {
    fn kind(self) -> AgentCliKind {
        match self {
            Self::ClaudeCode => AgentCliKind::ClaudeCode,
            Self::Gemini => AgentCliKind::Gemini,
            Self::Grok => AgentCliKind::Grok,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Gemini => "Gemini CLI",
            Self::Grok => "Grok Build",
        }
    }
    fn key(self) -> &'static str {
        self.kind().key()
    }
    fn events(self) -> &'static [&'static str] {
        match self {
            Self::ClaudeCode => CLAUDE_EVENTS,
            Self::Gemini => GEMINI_EVENTS,
            Self::Grok => GROK_EVENTS,
        }
    }
    fn standalone(self) -> bool {
        matches!(self, Self::Grok)
    }

    fn trust(self) -> AgentHookTrust {
        match self {
            Self::Grok => AgentHookTrust::Trusted,
            Self::ClaudeCode | Self::Gemini => AgentHookTrust::NotApplicable,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GenericHookService {
    pub(crate) agent: GenericAgent,
    pub(crate) config_path: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) helper_path: PathBuf,
    pub(crate) spool_root: PathBuf,
}

#[derive(Debug, Clone)]
struct Definition {
    event_name: String,
    identity: String,
    handler: Value,
}

impl Definition {
    fn group(&self) -> Value {
        json!({"matcher":"*","hooks":[self.handler.clone()]})
    }

    fn fingerprint(&self) -> String {
        group_fingerprint(&self.group())
    }
}

#[derive(Debug, Clone)]
struct FoundResource {
    event_name: String,
    identity: String,
    fingerprint: String,
}

impl GenericHookService {
    pub(crate) fn from_app(app: &tauri::AppHandle, agent: GenericAgent) -> Result<Self, String> {
        use tauri::Manager;
        let home = common::home_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("无法定位 BalanceHub 数据目录: {error}"))?;
        let key = agent.key();
        Ok(Self::new(
            agent,
            common::config_path_for(&home, agent.kind()),
            app_data
                .join("agent-hooks")
                .join(key)
                .join("ownership.json"),
            std::env::current_exe()
                .map_err(|error| format!("无法定位 BalanceHub 可执行文件: {error}"))?,
            app_data,
        ))
    }

    pub(crate) fn new(
        agent: GenericAgent,
        config_path: PathBuf,
        manifest_path: PathBuf,
        helper_path: PathBuf,
        spool_root: PathBuf,
    ) -> Self {
        Self {
            agent,
            config_path,
            manifest_path,
            helper_path,
            spool_root,
        }
    }

    fn definitions(&self) -> Vec<Definition> {
        self.agent
            .events()
            .iter()
            .map(|event_name| {
                let identity = format!(
                    "balancehub:{}:{}:v1",
                    self.agent.key(),
                    event_name.to_ascii_lowercase()
                );
                let handler = match self.agent {
                    GenericAgent::ClaudeCode => json!({
                        "type": "command",
                        "command": self.helper_path.to_string_lossy(),
                        "args": helper_args(&self.spool_root, self.agent.key(), &identity),
                        "timeout": 3
                    }),
                    GenericAgent::Gemini => json!({
                        "name": identity,
                        "type": "command",
                        "command": helper_command(
                            &self.helper_path,
                            &self.spool_root,
                            self.agent.key(),
                            &identity
                        ),
                        "timeout": 2000
                    }),
                    GenericAgent::Grok => json!({
                        "type": "command",
                        "command": helper_command(
                            &self.helper_path,
                            &self.spool_root,
                            self.agent.key(),
                            &identity
                        ),
                        "timeout": 2
                    }),
                };
                Definition {
                    event_name: (*event_name).to_string(),
                    identity,
                    handler,
                }
            })
            .collect()
    }

    pub(crate) fn inspect(&self) -> AgentHookInspection {
        let config = common::read_json(&self.config_path, self.agent.label());
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
            if owned.agent_kind != self.agent.kind()
                || owned.config_path != self.config_path.to_string_lossy()
            {
                conflict = true;
                diagnostics.push("Hook ownership manifest 与当前配置路径不匹配".to_string());
            } else if self.agent.standalone() {
                installed = !owned.resources.is_empty();
                // The standalone Grok resource is byte-owned.  Re-encoding a
                // parsed JSON value would erase formatting changes and defeat
                // the exact fingerprint conflict guard.
                let current = snapshot.revision.clone();
                if installed {
                    let expected = owned
                        .resources
                        .first()
                        .map(|resource| resource.content_fingerprint.as_str());
                    if expected != Some(current.as_str()) {
                        conflict = true;
                        diagnostics.push(
                            "Grok Build BalanceHub Hook 文件已被修改，未覆盖用户变更".to_string(),
                        );
                    } else if owned.enabled {
                        enabled = true;
                    }
                }
                if enabled {
                    last_event_at = common::latest_event_after(
                        &self.spool_root,
                        self.agent.kind(),
                        owned.installed_at,
                    );
                }
            } else {
                let found = find_resources(&snapshot.value, self.agent);
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
                            if owned.enabled && !matches!(self.agent, GenericAgent::Gemini) {
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
                    item.identity
                        .starts_with(&format!("balancehub:{}:", self.agent.key()))
                        && !owned.resources.iter().any(|resource| {
                            resource.structural_identity == item.identity
                                && resource.content_fingerprint == item.fingerprint
                        })
                }) {
                    conflict = true;
                    diagnostics.push("检测到未登记的 BalanceHub Hook 节点，未自动接管".to_string());
                }
                installed = !owned.resources.is_empty();
                let disabled = matches!(self.agent, GenericAgent::Gemini)
                    && is_disabled(&snapshot.value, &owned.resources);
                enabled = installed && all_present && !disabled && owned.enabled && !conflict;
                if enabled {
                    last_event_at = common::latest_event_after(
                        &self.spool_root,
                        self.agent.kind(),
                        owned.installed_at,
                    );
                }
            }
        } else if let Some(snapshot) = config.snapshot.as_ref() {
            if !find_resources(&snapshot.value, self.agent).is_empty()
                || (self.agent.standalone() && config.status == common::ConfigStatus::Present)
            {
                conflict = true;
                diagnostics.push(
                    "发现 BalanceHub Hook 节点但缺少 ownership manifest，未删除或覆盖".to_string(),
                );
            }
        }
        // A disabled owned standalone file is intentionally absent and is not a conflict.
        if self.agent.standalone()
            && ownership.as_ref().is_some_and(|owned| {
                !owned.enabled
                    && owned.agent_kind == self.agent.kind()
                    && owned.config_path == self.config_path.to_string_lossy()
            })
            && config.status == common::ConfigStatus::Missing
        {
            conflict = false;
            installed = true;
            enabled = false;
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
            agent_kind: self.agent.kind(),
            runtime_scope: AgentRuntimeScope::Native,
            config_path: self.config_path.to_string_lossy().into_owned(),
            config_exists: config.status == common::ConfigStatus::Present,
            revision,
            state,
            installed,
            enabled,
            trusted: self.agent.trust(),
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

    pub(crate) fn plan(&self, mutation: AgentHookMutation) -> AgentHookPlan {
        let inspection = self.inspect();
        let manifest = common::read_manifest(&self.manifest_path).ok().flatten();
        let config = common::read_json(&self.config_path, self.agent.label());
        let found = config
            .snapshot
            .as_ref()
            .map(|snapshot| find_resources(&snapshot.value, self.agent))
            .unwrap_or_default();
        let definitions = self.definitions();
        let resources = manifest
            .as_ref()
            .map(|manifest| manifest.resources.as_slice())
            .unwrap_or(&[]);
        let conflict = inspection.state == AgentHookHealthState::Conflict
            || (manifest.as_ref().is_some_and(|m| m.enabled)
                && config.status == common::ConfigStatus::Missing);
        let mut changes = Vec::new();
        if self.agent.standalone() {
            let definition = definitions.first().expect("standalone agent has events");
            let fingerprint = standalone_fingerprint(&definitions, self.agent.label());
            let kind = if config.status == common::ConfigStatus::Present {
                AgentHookChangeKind::Keep
            } else {
                AgentHookChangeKind::Add
            };
            if matches!(
                mutation,
                AgentHookMutation::Install | AgentHookMutation::Enable
            ) || !resources.is_empty()
            {
                changes.push(AgentHookChange {
                    event_name: "balancehub-runtime.json".to_string(),
                    structural_identity: format!("balancehub:{}:file:v1", self.agent.key()),
                    fingerprint: if kind == AgentHookChangeKind::Keep {
                        config
                            .snapshot
                            .as_ref()
                            .map(|s| s.revision.clone())
                            .unwrap_or(fingerprint)
                    } else {
                        fingerprint
                    },
                    kind: if matches!(
                        mutation,
                        AgentHookMutation::Disable | AgentHookMutation::Remove
                    ) {
                        if config.status == common::ConfigStatus::Present {
                            AgentHookChangeKind::Remove
                        } else {
                            AgentHookChangeKind::Keep
                        }
                    } else {
                        kind
                    },
                });
            } else {
                let _ = definition;
            }
        } else {
            for definition in &definitions {
                let present = found.iter().find(|item| {
                    item.event_name == definition.event_name && item.identity == definition.identity
                });
                let kind = match present {
                    Some(item) if item.fingerprint == definition.fingerprint() => {
                        AgentHookChangeKind::Keep
                    }
                    _ => AgentHookChangeKind::Add,
                };
                changes.push(AgentHookChange {
                    event_name: definition.event_name.clone(),
                    structural_identity: definition.identity.clone(),
                    fingerprint: definition.fingerprint(),
                    kind: if mutation == AgentHookMutation::Disable
                        && matches!(self.agent, GenericAgent::Gemini)
                    {
                        AgentHookChangeKind::Keep
                    } else if matches!(
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
            match self.prepare_configuration(
                mutation,
                &config,
                manifest.as_ref(),
                &definitions,
                structural_changes > 0,
            ) {
                Ok(prepared) => {
                    let mut contents = prepared.content_changes(
                        &self.config_path,
                        config
                            .snapshot
                            .as_ref()
                            .map(|snapshot| snapshot.text.as_str()),
                    );
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
            agent_kind: self.agent.kind(),
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
                format!("确认后将{action} {} 会话接入", self.agent.label())
            } else {
                format!(
                    "确认后将{action} {changed} 个 {} Hook 节点",
                    self.agent.label()
                )
            },
        }
    }

    pub(crate) fn apply(&self, plan: AgentHookPlan) -> Result<AgentHookInspection, String> {
        if !plan.supported
            || plan.conflict
            || plan.agent_kind != self.agent.kind()
            || plan.runtime_scope != AgentRuntimeScope::Native
            || plan.config_path != self.config_path.to_string_lossy()
        {
            return Err("Hook 计划存在冲突或不受支持，未修改配置".to_string());
        }
        let parent = self.config_path.parent().ok_or("Hook 配置目录无效")?;
        let config_root = if self.agent.standalone() {
            parent.parent().ok_or("Hook 配置根目录无效")?
        } else {
            parent
        };
        super::locking::with_locked_sources(
            &self.config_path,
            &self.manifest_path,
            config_root,
            || self.apply_locked(plan),
        )
    }

    fn apply_locked(&self, plan: AgentHookPlan) -> Result<AgentHookInspection, String> {
        let current = self.inspect();
        if current.revision != plan.expected_revision {
            return Err(format!(
                "{} Hook 配置已变化，请重新生成计划后再试",
                self.agent.label()
            ));
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
        let config = common::read_json(&self.config_path, self.agent.label());
        let definitions = self.definitions();
        let manifest = common::read_manifest(&self.manifest_path)?;
        let prepared = self.prepare_configuration(
            plan.mutation,
            &config,
            manifest.as_ref(),
            &definitions,
            current_plan
                .changes
                .iter()
                .any(|change| change.kind != AgentHookChangeKind::Keep),
        )?;
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
                common::ensure_managed_parent(&self.manifest_path)?;
                let installed_at = manifest
                    .as_ref()
                    .map(|manifest| manifest.installed_at)
                    .unwrap_or_else(common::now_millis);
                let ownership =
                    ownership_for(self, &definitions, installed_at, true, &prepared.value);
                self.commit_configuration(&prepared)?;
                common::write_manifest(&self.manifest_path, &ownership)?;
            }
            AgentHookMutation::Disable => {
                let mut ownership =
                    manifest.ok_or("缺少 Hook ownership manifest，未修改任何配置")?;
                self.commit_configuration(&prepared)?;
                ownership.enabled = false;
                common::write_manifest(&self.manifest_path, &ownership)?;
            }
            AgentHookMutation::Remove => {
                manifest.ok_or("缺少 Hook ownership manifest，未删除任何配置")?;
                self.commit_configuration(&prepared)?;
                common::remove_manifest(&self.manifest_path)?;
            }
        }
        Ok(self.inspect())
    }
}

pub(super) fn owned_resources_changed(
    agent: GenericAgent,
    before: &Value,
    after: &Value,
    ownership: &AgentHookOwnership,
) -> bool {
    let before = find_resources(before, agent);
    let after = find_resources(after, agent);
    ownership.resources.iter().any(|owned| {
        let matches = |found: &FoundResource| {
            found.event_name == owned.event_name
                && found.identity == owned.structural_identity
                && found.fingerprint == owned.content_fingerprint
        };
        before.iter().filter(|resource| matches(resource)).count() == 1
            && after.iter().filter(|resource| matches(resource)).count() != 1
    })
}

pub(super) fn owned_resource_selected(
    agent: GenericAgent,
    document: &Value,
    event: &str,
    group: &Value,
    ownership: &AgentHookOwnership,
) -> bool {
    if group.get("hooks").and_then(Value::as_array).map(Vec::len) != Some(1) {
        return false;
    }
    let fingerprint = group_fingerprint(group);
    let found = find_resources(document, agent);
    ownership.resources.iter().any(|owned| {
        owned.event_name == event
            && owned.content_fingerprint == fingerprint
            && found
                .iter()
                .filter(|resource| {
                    resource.event_name == owned.event_name
                        && resource.identity == owned.structural_identity
                        && resource.fingerprint == owned.content_fingerprint
                })
                .count()
                == 1
    })
}

fn helper_command(executable: &Path, spool_root: &Path, agent: &str, identity: &str) -> String {
    #[cfg(windows)]
    {
        format!(
            "{} --balancehub-hook-ingest --agent {} --balancehub-hook-node={} --spool-root {}",
            quote_windows(&executable.to_string_lossy()),
            agent,
            identity,
            quote_windows(&spool_root.to_string_lossy())
        )
    }
    #[cfg(not(windows))]
    {
        format!(
            "{} --balancehub-hook-ingest --agent {} --balancehub-hook-node={} --spool-root {}",
            quote_unix(&executable.to_string_lossy()),
            agent,
            identity,
            quote_unix(&spool_root.to_string_lossy())
        )
    }
}

fn helper_args(spool_root: &Path, agent: &str, identity: &str) -> Vec<String> {
    vec![
        "--balancehub-hook-ingest".to_string(),
        "--agent".to_string(),
        agent.to_string(),
        format!("--balancehub-hook-node={identity}"),
        "--spool-root".to_string(),
        spool_root.to_string_lossy().into_owned(),
    ]
}

fn ensure_owned_leaf_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "目标文件缺少父目录".to_string())?;
    if parent.exists() {
        return common::ensure_parent(path);
    }
    let owner_root = parent
        .parent()
        .ok_or_else(|| "Hook 目录缺少已存在的父目录".to_string())?;
    let metadata = std::fs::symlink_metadata(owner_root)
        .map_err(|error| format!("读取 Hook 父目录失败: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Hook 父目录不是安全的普通目录".to_string());
    }
    std::fs::create_dir(parent).map_err(|error| format!("创建 Hook 目录失败: {error}"))?;
    common::ensure_parent(path)
}

fn add_nested_definition(
    root: &mut Value,
    definition: &Definition,
    agent: GenericAgent,
) -> Result<(), String> {
    let object = root
        .as_object_mut()
        .ok_or_else(|| format!("{} 配置顶层必须是 JSON 对象", agent.label()))?;
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "Hook 配置的 hooks 必须是对象".to_string())?;
    let events = hooks
        .entry(&definition.event_name)
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| format!("Hook {} 必须是数组", definition.event_name))?;
    if let Some(existing) = events.iter().find(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|handlers| {
                handlers
                    .iter()
                    .any(|handler| handler_identity(handler) == Some(definition.identity.as_str()))
            })
    }) {
        if existing != &definition.group() {
            return Err(format!(
                "Hook {} 已被修改，无法安全覆盖",
                definition.event_name
            ));
        }
        return Ok(());
    }
    events.push(definition.group());
    Ok(())
}

fn remove_nested_definitions(root: &mut Value, definitions: &[Definition]) -> Result<bool, String> {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(false);
    };
    let mut changed = false;
    for definition in definitions {
        let Some(groups) = hooks
            .get_mut(&definition.event_name)
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        let before = groups.len();
        groups.retain(|group| {
            !(group_fingerprint(group) == definition.fingerprint()
                && group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .is_some_and(|handlers| {
                        handlers.iter().any(|handler| {
                            handler_identity(handler) == Some(definition.identity.as_str())
                        })
                    }))
        });
        changed |= groups.len() != before;
    }
    Ok(changed)
}

fn find_resources(root: &Value, agent: GenericAgent) -> Vec<FoundResource> {
    let Some(hooks) = root.get("hooks").and_then(Value::as_object) else {
        return Vec::new();
    };
    hooks
        .iter()
        .filter(|(event, _)| agent.events().contains(&event.as_str()))
        .flat_map(|(event, groups)| {
            groups
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(move |group| {
                    let fingerprint = group_fingerprint(group);
                    group
                        .get("hooks")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(move |handler| {
                            handler_identity(handler)
                                .filter(|identity| {
                                    identity.starts_with(&format!("balancehub:{}:", agent.key()))
                                })
                                .map(|identity| FoundResource {
                                    event_name: event.clone(),
                                    identity: identity.to_string(),
                                    fingerprint: fingerprint.clone(),
                                })
                        })
                })
        })
        .collect()
}

fn handler_identity(handler: &Value) -> Option<&str> {
    handler
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| {
            handler
                .get("args")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .find_map(identity_from_command_fragment)
        })
        .or_else(|| {
            handler
                .get("command")
                .and_then(Value::as_str)
                .and_then(identity_from_command_fragment)
        })
}

fn identity_from_command_fragment(value: &str) -> Option<&str> {
    value
        .split("--balancehub-hook-node=")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .map(|value| value.trim_matches(['\'', '"']))
}

fn group_fingerprint(group: &Value) -> String {
    common::revision_for_bytes(canonical_json(group).as_bytes())
}
fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            format!(
                "{{{}}}",
                entries
                    .into_iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_default(),
                        canonical_json(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}
fn ownership_for(
    service: &GenericHookService,
    definitions: &[Definition],
    installed_at: i64,
    enabled: bool,
    value: &Value,
) -> AgentHookOwnership {
    let resources = if service.agent.standalone() {
        vec![AgentHookOwnedResource {
            event_name: "balancehub-runtime.json".to_string(),
            structural_identity: format!("balancehub:{}:file:v1", service.agent.key()),
            content_fingerprint: common::revision_for_bytes(
                &common::encode_json(value, service.agent.label()).unwrap_or_default(),
            ),
        }]
    } else {
        definitions
            .iter()
            .map(|definition| AgentHookOwnedResource {
                event_name: definition.event_name.clone(),
                structural_identity: definition.identity.clone(),
                content_fingerprint: definition.fingerprint(),
            })
            .collect()
    };
    AgentHookOwnership {
        agent_kind: service.agent.kind(),
        runtime_scope: AgentRuntimeScope::Native,
        config_path: service.config_path.to_string_lossy().into_owned(),
        helper_version: HELPER_VERSION.to_string(),
        installed_at,
        enabled,
        resources,
    }
}
fn standalone_document(definitions: &[Definition]) -> Value {
    let mut hooks = Map::new();
    for definition in definitions {
        hooks.insert(
            definition.event_name.clone(),
            json!([{"hooks":[definition.handler]}]),
        );
    }
    json!({"hooks": hooks})
}
fn standalone_fingerprint(definitions: &[Definition], label: &str) -> String {
    common::revision_for_bytes(
        &common::encode_json(&standalone_document(definitions), label).unwrap_or_default(),
    )
}
fn is_disabled(value: &Value, resources: &[AgentHookOwnedResource]) -> bool {
    value
        .get("hooksConfig")
        .and_then(|config| config.get("disabled"))
        .and_then(Value::as_array)
        .is_some_and(|disabled| {
            resources.iter().any(|resource| {
                disabled
                    .iter()
                    .any(|name| name.as_str() == Some(resource.structural_identity.as_str()))
            })
        })
}
fn is_definitions_disabled(value: &Value, definitions: &[Definition]) -> bool {
    value
        .get("hooksConfig")
        .and_then(|config| config.get("disabled"))
        .and_then(Value::as_array)
        .is_some_and(|disabled| {
            definitions.iter().any(|definition| {
                disabled
                    .iter()
                    .any(|name| name.as_str() == Some(definition.identity.as_str()))
            })
        })
}
fn add_disabled(value: &mut Value, definitions: &[Definition]) -> Result<bool, String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Gemini CLI 配置顶层必须是 JSON 对象".to_string())?;
    let config = object
        .entry("hooksConfig")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| "Gemini hooksConfig 必须是对象".to_string())?;
    let disabled = config
        .entry("disabled")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| "Gemini hooksConfig.disabled 必须是数组".to_string())?;
    let mut changed = false;
    for definition in definitions {
        if !disabled
            .iter()
            .any(|item| item.as_str() == Some(definition.identity.as_str()))
        {
            disabled.push(Value::String(definition.identity.clone()));
            changed = true;
        }
    }
    Ok(changed)
}
fn remove_disabled(value: &mut Value, definitions: &[Definition]) -> bool {
    if let Some(disabled) = value
        .get_mut("hooksConfig")
        .and_then(|config| config.get_mut("disabled"))
        .and_then(Value::as_array_mut)
    {
        let before = disabled.len();
        disabled.retain(|item| {
            !definitions
                .iter()
                .any(|definition| item.as_str() == Some(definition.identity.as_str()))
        });
        return disabled.len() != before;
    }
    false
}
#[cfg(not(windows))]
fn quote_unix(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
#[cfg(windows)]
fn quote_windows(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgentHookHealthState, AgentHookMutation, AgentHookTrust};
    use std::fs;

    fn fixture(agent: GenericAgent) -> (PathBuf, GenericHookService) {
        let root = tempfile::Builder::new()
            .prefix(&format!("balancehub-managed-{}-", agent.key()))
            .tempdir()
            .unwrap()
            .keep()
            .canonicalize()
            .unwrap();
        let config = match agent {
            GenericAgent::ClaudeCode => root.join(".claude/settings.json"),
            GenericAgent::Gemini => root.join(".gemini/settings.json"),
            GenericAgent::Grok => root.join(".grok/hooks/balancehub-runtime.json"),
        };
        if matches!(agent, GenericAgent::Grok) {
            fs::create_dir_all(root.join(".grok")).unwrap();
        } else {
            fs::create_dir_all(config.parent().unwrap()).unwrap();
        }
        fs::create_dir_all(root.join("app")).unwrap();
        fs::write(root.join("app/helper"), b"helper").unwrap();
        if !matches!(agent, GenericAgent::Grok) {
            fs::write(&config, b"{}\n").unwrap();
        }
        let service = GenericHookService::new(
            agent,
            config,
            root.join("app/ownership.json"),
            root.join("app/helper"),
            root.join("app"),
        );
        (root, service)
    }

    #[test]
    fn claude_and_gemini_install_preserve_user_data_and_are_idempotent() {
        for agent in [GenericAgent::ClaudeCode, GenericAgent::Gemini] {
            let (root, service) = fixture(agent);
            let path = service.config_path.clone();
            fs::write(&path, b"{\"custom\":{\"keep\":true}}\n").unwrap();
            service
                .apply(service.plan(AgentHookMutation::Install))
                .unwrap();
            let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(value["custom"]["keep"], true);
            assert_eq!(
                service.inspect().state,
                AgentHookHealthState::InstalledUnverified
            );
            assert!(service
                .plan(AgentHookMutation::Install)
                .changes
                .iter()
                .all(|change| change.kind == AgentHookChangeKind::Keep));
            let before_second_install = fs::read(&path).unwrap();
            service
                .apply(service.plan(AgentHookMutation::Install))
                .unwrap();
            assert_eq!(fs::read(&path).unwrap(), before_second_install);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn generic_service_rejects_forged_plan_target_and_changes() {
        let (root, service) = fixture(GenericAgent::ClaudeCode);
        let path = service.config_path.clone();
        let original = fs::read(&path).unwrap();

        let mut wrong_scope = service.plan(AgentHookMutation::Install);
        wrong_scope.runtime_scope = AgentRuntimeScope::Wsl {
            distro_id: "Ubuntu".to_string(),
        };
        assert!(service.apply(wrong_scope).is_err());

        let mut changed_plan = service.plan(AgentHookMutation::Install);
        changed_plan.changes[0].fingerprint = "forged".to_string();
        assert!(service.apply(changed_plan).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn definitions_follow_each_installed_agent_hook_schema() {
        let (claude_root, claude) = fixture(GenericAgent::ClaudeCode);
        let claude_handler = &claude.definitions()[0].handler;
        assert_eq!(
            claude_handler["command"],
            claude.helper_path.to_string_lossy().as_ref()
        );
        assert_eq!(claude_handler["timeout"], 3);
        assert!(claude_handler["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg == "--balancehub-hook-node=balancehub:claudeCode:sessionstart:v1"));

        let (gemini_root, gemini) = fixture(GenericAgent::Gemini);
        let gemini_handler = &gemini.definitions()[0].handler;
        assert_eq!(gemini_handler["timeout"], 2000);
        assert_eq!(gemini_handler["name"], "balancehub:gemini:sessionstart:v1");

        let (grok_root, grok) = fixture(GenericAgent::Grok);
        assert_eq!(grok.definitions()[0].handler["timeout"], 2);
        fs::remove_dir_all(claude_root).unwrap();
        fs::remove_dir_all(gemini_root).unwrap();
        fs::remove_dir_all(grok_root).unwrap();
    }

    #[test]
    fn trust_reports_only_grok_personal_hooks_as_inherently_trusted() {
        for agent in [GenericAgent::ClaudeCode, GenericAgent::Gemini] {
            let (root, service) = fixture(agent);
            assert_eq!(service.inspect().trusted, AgentHookTrust::NotApplicable);
            fs::remove_dir_all(root).unwrap();
        }
        let (root, service) = fixture(GenericAgent::Grok);
        assert_eq!(service.inspect().trusted, AgentHookTrust::Trusted);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn gemini_disable_uses_owned_names_and_enable_restores_without_touching_others() {
        let (root, service) = fixture(GenericAgent::Gemini);
        service
            .apply(service.plan(AgentHookMutation::Install))
            .unwrap();
        let path = service.config_path.clone();
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["hooksConfig"]["disabled"] = json!(["user-hook"]);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        service
            .apply(service.plan(AgentHookMutation::Disable))
            .unwrap();
        let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let disabled = value["hooksConfig"]["disabled"].as_array().unwrap();
        assert!(disabled.iter().any(|item| item == "user-hook"));
        assert_eq!(disabled.len(), 5);
        assert!(!service.inspect().enabled);
        service
            .apply(service.plan(AgentHookMutation::Enable))
            .unwrap();
        let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["hooksConfig"]["disabled"], json!(["user-hook"]));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeated_disable_is_byte_stable_and_gemini_remove_cleans_only_owned_names() {
        for agent in [GenericAgent::ClaudeCode, GenericAgent::Gemini] {
            let (root, service) = fixture(agent);
            service
                .apply(service.plan(AgentHookMutation::Install))
                .unwrap();
            let path = service.config_path.clone();
            if matches!(agent, GenericAgent::Gemini) {
                let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                value["hooksConfig"]["disabled"] = json!(["user-hook"]);
                fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            }
            service
                .apply(service.plan(AgentHookMutation::Disable))
                .unwrap();
            let disabled_bytes = fs::read(&path).unwrap();
            service
                .apply(service.plan(AgentHookMutation::Disable))
                .unwrap();
            assert_eq!(fs::read(&path).unwrap(), disabled_bytes);

            service
                .apply(service.plan(AgentHookMutation::Remove))
                .unwrap();
            if matches!(agent, GenericAgent::Gemini) {
                let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                assert_eq!(value["hooksConfig"]["disabled"], json!(["user-hook"]));
            }
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn matcher_group_drift_is_a_conflict_and_is_never_removed() {
        let (root, service) = fixture(GenericAgent::ClaudeCode);
        service
            .apply(service.plan(AgentHookMutation::Install))
            .unwrap();
        let path = service.config_path.clone();
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["hooks"]["SessionStart"][0]["matcher"] = json!("startup");
        let drifted = serde_json::to_vec_pretty(&value).unwrap();
        fs::write(&path, &drifted).unwrap();
        let plan = service.plan(AgentHookMutation::Remove);
        assert!(plan.conflict);
        assert!(service.apply(plan).is_err());
        assert_eq!(fs::read(&path).unwrap(), drifted);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn grok_standalone_file_is_exactly_owned_and_drift_is_not_overwritten() {
        let (root, service) = fixture(GenericAgent::Grok);
        assert!(!service.config_path.parent().unwrap().exists());
        service
            .apply(service.plan(AgentHookMutation::Install))
            .unwrap();
        assert_eq!(
            service.inspect().state,
            AgentHookHealthState::InstalledUnverified
        );
        let path = service.config_path.clone();
        let before = fs::read(&path).unwrap();
        service
            .apply(service.plan(AgentHookMutation::Install))
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::write(&path, b"{\"user\":true}\n").unwrap();
        let plan = service.plan(AgentHookMutation::Remove);
        assert!(plan.conflict);
        assert!(service.apply(plan).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{\"user\":true}\n");
        fs::write(&path, before).unwrap();
        service
            .apply(service.plan(AgentHookMutation::Remove))
            .unwrap();
        assert!(!path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn grok_install_rejects_a_symlinked_hooks_directory() {
        use std::os::unix::fs::symlink;

        let (root, service) = fixture(GenericAgent::Grok);
        let redirected = root.join("redirected");
        fs::create_dir(&redirected).unwrap();
        symlink(&redirected, service.config_path.parent().unwrap()).unwrap();
        let error = service
            .apply(service.plan(AgentHookMutation::Install))
            .unwrap_err();
        assert!(error.contains("路径不可安全访问"));
        assert_eq!(fs::read_dir(&redirected).unwrap().count(), 0);
        assert!(!redirected.join("balancehub-runtime.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
