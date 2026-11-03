//! Native Gemini settings locations and exact-case disabled-list policy.

use super::hooks;
use crate::{
    models::*,
    services::agent_cli::{
        catalog::native::{
            hook_codec::{self, HookDocument, HookDocumentFormat},
            hooks::HookNativeDestination,
        },
        environment::mutation::MutationInventory,
        native_agent_kinds::gemini::AGENT_KIND,
    },
};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

fn settings_locations(
    context: &AgentConfigurationContext,
) -> Vec<(&'static str, AgentAssetScope, PathBuf, PathBuf)> {
    let root = Path::new(&context.config_root);
    let managed = super::sources::managed_root();
    let mut locations = vec![
        (
            "system-defaults",
            AgentAssetScope::System,
            managed.join("system-defaults.json"),
            managed.clone(),
        ),
        (
            "settings",
            AgentAssetScope::User,
            root.join("settings.json"),
            root.to_path_buf(),
        ),
        (
            "system-settings",
            AgentAssetScope::Managed,
            managed.join("settings.json"),
            managed,
        ),
    ];
    if let Some(workspace) = context.workspace_id.as_deref() {
        let workspace = Path::new(workspace);
        locations.push((
            "workspace-settings",
            AgentAssetScope::Workspace,
            workspace.join(".gemini/settings.json"),
            workspace.to_path_buf(),
        ));
    }
    locations
}

pub(in super::super) fn settings_role(
    source: &AgentAssetSource,
    context: &AgentConfigurationContext,
) -> Option<&'static str> {
    if context.agent_kind != AGENT_KIND {
        return None;
    }
    settings_locations(context)
        .into_iter()
        .find_map(|(role, scope, path, root)| {
            (Path::new(&source.allowed_root) == root
                && hook_codec::exact_source(
                    source,
                    context,
                    scope,
                    &path,
                    AgentAssetInstallationOrigin::ConfigEntry,
                ))
            .then_some(role)
        })
}

pub(in super::super) fn targets(
    inventory: &AgentEnvironmentInventory,
    context: &AgentConfigurationContext,
) -> Vec<HookNativeDestination> {
    // Published snapshots omit allowed_root authority. Candidate discovery uses
    // native metadata; settings_role and guarded reads still validate live writes.
    inventory.sources.iter().filter_map(|source| {
        if context.agent_kind != AGENT_KIND { return None; }
        settings_locations(context).into_iter().find_map(|(role, scope, path, _)| {
            hook_codec::exact_source(source, context, scope, &path, AgentAssetInstallationOrigin::ConfigEntry)
                .then(|| hook_codec::destination(source, role)).flatten()
        })
    }).collect()
}

pub(in super::super) struct SettingsDocument {
    pub(in super::super) document: HookDocument,
    pub(in super::super) destination: Option<HookNativeDestination>,
    pub(in super::super) participates: bool,
}

pub(in super::super) fn load_settings(
    snapshot: &MutationInventory,
    context: &AgentConfigurationContext,
) -> Result<Vec<SettingsDocument>, String> {
    let mut documents = Vec::new();
    for (role, _, _, _) in settings_locations(context) {
        let mut candidates = snapshot
            .inventory
            .sources
            .iter()
            .filter(|source| settings_role(source, context) == Some(role));
        let source = candidates.next().ok_or("Gemini Hook 策略来源不完整")?;
        if candidates.next().is_some() {
            return Err("Gemini Hook 策略来源存在歧义".to_owned());
        }
        documents.push(SettingsDocument {
            document: HookDocument::observe(snapshot, &source.id, HookDocumentFormat::Jsonc)?,
            destination: hook_codec::destination(source, role),
            participates: source.scope != AgentAssetScope::Workspace
                || context.trust_context == AgentTrustState::Trusted,
        });
    }
    Ok(documents)
}

pub(in super::super) fn trust_source_id(
    snapshot: &MutationInventory,
    context: &AgentConfigurationContext,
) -> Result<Option<String>, String> {
    if context.workspace_id.is_none() {
        return Ok(None);
    }
    let root = Path::new(&context.config_root);
    let mut sources = snapshot.inventory.sources.iter().filter(|source| {
        source.context_id == context.id
            && source.environment_id == context.environment_id
            && source.scope == AgentAssetScope::User
            && source.origin == AgentAssetInstallationOrigin::ConfigEntry
            && source.source_kind == AgentAssetSourceKind::File
            && Path::new(&source.allowed_root) == root
            && Path::new(&source.path) == root.join("trustedFolders.json")
    });
    let source = sources.next().ok_or("Gemini 工作区信任来源不完整")?;
    if sources.next().is_some() {
        return Err("Gemini 工作区信任来源存在歧义".to_owned());
    }
    Ok(Some(source.id.clone()))
}

pub(in super::super) fn disabled_set(root: &Value) -> Result<BTreeSet<String>, String> {
    hooks::disabled_names(root.as_object().ok_or("Gemini Hook 配置根节点无效")?)
        .map(|names| {
            names
                .unwrap_or_default()
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
        .map_err(|_| "Gemini hooksConfig.disabled 策略无效，无法确认启停状态".to_owned())
}

pub(in super::super) fn configured_state(
    documents: &[SettingsDocument],
    desired: &BTreeMap<String, Value>,
    identity: &str,
) -> AgentAssetDeclaredState {
    let mut invalid = false;
    for document in documents.iter().filter(|document| document.participates) {
        let root = desired
            .get(&document.document.source_id)
            .unwrap_or(&document.document.root);
        match disabled_set(root) {
            Ok(names) if names.contains(identity) => return AgentAssetDeclaredState::Disabled,
            Ok(_) => (),
            Err(_) => invalid = true,
        }
    }
    if invalid {
        AgentAssetDeclaredState::Unknown
    } else {
        AgentAssetDeclaredState::Enabled
    }
}

pub(in super::super) fn request_switch(
    requests: &mut BTreeMap<String, bool>,
    identity: String,
    enabled: bool,
) -> Result<(), String> {
    if requests
        .insert(identity, enabled)
        .is_some_and(|previous| previous != enabled)
    {
        return Err("同一 Gemini Hook 原生身份存在冲突的启停请求".to_owned());
    }
    Ok(())
}

pub(in super::super) fn edit_disabled(
    root: &mut Value,
    identity: &str,
    enabled: bool,
) -> Result<(), String> {
    let names = disabled_set(root)?;
    if names.contains(identity) != enabled {
        return Ok(());
    }
    let config = root
        .as_object_mut()
        .ok_or("Gemini Hook 配置根节点无效")?
        .entry("hooksConfig")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("Gemini hooksConfig 配置无效")?;
    let disabled = config
        .entry("disabled")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or("Gemini hooksConfig.disabled 配置无效")?;
    if enabled {
        disabled.retain(|value| value.as_str() != Some(identity));
    } else {
        disabled.push(Value::String(identity.to_owned()));
    }
    Ok(())
}
