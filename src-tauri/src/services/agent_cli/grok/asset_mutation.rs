use crate::{
    models::*,
    services::agent_cli::environment::mutation::{
        catalog::{
            native_records, safe_native_id, schema_error, unavailable_error, NativeMechanismSpec,
        },
        ExactCliCommand, MutationExecution, MutationPreparation, MutationVerification,
        PreparedMutation,
    },
};
use std::{ffi::OsString, path::Path};
use toml_edit::{Array, Document, Item, Table};

pub(super) fn mechanisms(kind: AgentCliKind) -> Vec<AgentAssetMechanismRecord> {
    let mut records = native_records(NativeMechanismSpec {
        key: "grok-mcp-persistent-cli", agent: kind, category: AgentAssetCategory::Mcp, schema: super::environment::PARSER_VERSION,
        source_schema: Some("config.toml disabled_mcp_servers plus mcp_servers[id].enabled; enable also unsticks project declarations"),
        argv: &["mcp", "enable|disable", "<native-id>"], scopes: &[AgentAssetScope::User, AgentAssetScope::Workspace],
        reload: "更新个人 MCP 状态；启用也会解除已知工作区声明的禁用，现有会话由原生 watcher 重新加载",
    });
    records.extend(native_records(NativeMechanismSpec {
        key: "grok-plugin-enabled-toml",
        agent: kind,
        category: AgentAssetCategory::Plugin,
        schema: super::environment::PARSER_VERSION,
        source_schema: Some(
            "user config.toml plugins.enabled / plugins.disabled; trust is independent",
        ),
        argv: &[],
        scopes: &[AgentAssetScope::User, AgentAssetScope::Workspace],
        reload: "更新用户范围 Plugin 状态；保留信任与管理策略，现有会话需重新加载",
    }));
    records.extend(native_records(NativeMechanismSpec {
        key: "grok-skill-disabled-toml",
        agent: kind,
        category: AgentAssetCategory::Skill,
        schema: super::environment::PARSER_VERSION,
        source_schema: Some("user config.toml skills.disabled exact normalized native names"),
        argv: &[],
        scopes: &[AgentAssetScope::User, AgentAssetScope::Workspace],
        reload: "更新用户范围的技能禁用名单；已有会话需重新加载或新建会话",
    }));
    records
}

pub(super) fn unavailable_reason(
    category: AgentAssetCategory,
    _: AgentAssetActionKind,
) -> AgentAssetActionUnavailableReason {
    match category {
        AgentAssetCategory::Hook => AgentAssetActionUnavailableReason::ManagedByAssetCatalog,
        AgentAssetCategory::StatusUi => AgentAssetActionUnavailableReason::NoReversibleMechanism,
        _ => AgentAssetActionUnavailableReason::NoOfficialMechanism,
    }
}

pub(super) fn prepare(
    request: MutationPreparation<'_>,
) -> Result<PreparedMutation, AgentAssetMutationError> {
    let enabled = request.desired_enabled()?;
    let id = safe_native_id(&request.asset.native_id)?;
    let root = Path::new(&request.context.config_root);
    let user = request.source_at(&root.join("config.toml"))?;
    let mut files = request.context_files()?;
    if !files.iter().any(|file| file.source_id == user.id) {
        files.push(request.file(user)?);
    }
    let user_file = files
        .iter()
        .find(|file| file.source_id == user.id)
        .ok_or_else(schema_error)?;
    let mut writes = vec![user.id.clone()];
    let mut changes = vec![request.boolean_change(Path::new(&user.path), "用户范围的原生启用状态")];
    let execution = match request.asset.category {
        AgentAssetCategory::Skill => MutationExecution::AtomicFile {
            source_id: user.id.clone(),
            replacement: edit_skill(user_file.bytes(), id, enabled)?,
        },
        AgentAssetCategory::Plugin => {
            for file in &files {
                if file
                    .path()
                    .file_name()
                    .is_some_and(|name| name == "config.toml")
                {
                    validate_config(file.bytes())?;
                    let doc = document(file.bytes())?;
                    if enabled
                        && file.source_id != user.id
                        && doc
                            .get("plugins")
                            .and_then(Item::as_table_like)
                            .and_then(|plugins| plugins.get("disabled"))
                            .map(string_array)
                            .transpose()?
                            .is_some_and(|list| {
                                list.iter()
                                    .filter_map(toml_edit::Value::as_str)
                                    .any(|name| name == id || name.rsplit('/').next() == Some(id))
                            })
                    {
                        return Err(unavailable_error(
                            AgentAssetActionUnavailableReason::ScopeAmbiguous,
                        ));
                    }
                }
            }
            MutationExecution::AtomicFile {
                source_id: user.id.clone(),
                replacement: edit_plugin(user_file.bytes(), id, enabled)?,
            }
        }
        AgentAssetCategory::Mcp => {
            require_known_project_sources(request)?;
            for file in &files {
                if file
                    .path()
                    .file_name()
                    .is_some_and(|name| name == "config.toml")
                {
                    validate_config(file.bytes())?;
                }
            }
            if enabled && request.asset.category == AgentAssetCategory::Mcp {
                // Native enable may clear every sticky project declaration of
                // this name. Include all discovered project config files.
                for source in request.inventory.sources.iter().filter(|source| {
                    source.context_id == request.context.id
                        && source.scope == AgentAssetScope::Workspace
                        && Path::new(&source.path)
                            .file_name()
                            .is_some_and(|name| name == "config.toml")
                }) {
                    if !files.iter().any(|file| file.source_id == source.id) {
                        files.push(request.file(source)?);
                    }
                    writes.push(source.id.clone());
                    changes.push(
                        request
                            .boolean_change(Path::new(&source.path), "解除工作区 MCP 声明的禁用"),
                    );
                }
            }
            MutationExecution::ExactCli(Box::new(ExactCliCommand::new(
                request.installation,
                vec![
                    "mcp".into(),
                    if enabled { "enable" } else { "disable" }.into(),
                    id.into(),
                ],
                request.cwd(),
                vec![
                    (OsString::from("HOME"), request.home.as_os_str().to_owned()),
                    (OsString::from("GROK_HOME"), root.as_os_str().to_owned()),
                ],
            )?))
        }
        category => {
            return Err(unavailable_error(unavailable_reason(
                category,
                request.action,
            )))
        }
    };
    let affected = if request.asset.category == AgentAssetCategory::Plugin {
        request.asset.relationships.affected_asset_ids.clone()
    } else {
        request
            .inventory
            .assets
            .iter()
            .filter(|asset| {
                asset.agent_kind == request.asset.agent_kind
                    && asset.category == request.asset.category
                    && asset.native_id == id
                    && asset.context_id == request.context.id
            })
            .map(|asset| asset.stable_id.clone())
            .collect()
    };
    PreparedMutation::new(request, execution, files, writes, changes, affected, verify)
}

fn document(bytes: Option<&[u8]>) -> Result<Document, AgentAssetMutationError> {
    std::str::from_utf8(bytes.unwrap_or_default())
        .map_err(|_| schema_error())?
        .parse()
        .map_err(|_| schema_error())
}

fn validate_config(bytes: Option<&[u8]>) -> Result<(), AgentAssetMutationError> {
    let document = document(bytes)?;
    if let Some(disabled) = document.get("disabled_mcp_servers") {
        string_array(disabled)?;
    }
    if let Some(plugins) = document.get("plugins") {
        let plugins = plugins.as_table_like().ok_or_else(schema_error)?;
        for key in ["enabled", "disabled"] {
            if let Some(list) = plugins.get(key) {
                string_array(list)?;
            }
        }
    }
    if let Some(servers) = document.get("mcp_servers") {
        for (_, entry) in servers.as_table_like().ok_or_else(schema_error)?.iter() {
            let entry = entry.as_table_like().ok_or_else(schema_error)?;
            if entry
                .get("enabled")
                .is_some_and(|enabled| enabled.as_bool().is_none())
            {
                return Err(schema_error());
            }
        }
    }
    Ok(())
}

fn string_array(item: &Item) -> Result<&Array, AgentAssetMutationError> {
    item.as_array()
        .filter(|list| list.iter().all(|value| value.is_str()))
        .ok_or_else(schema_error)
}

fn edit_skill(
    bytes: Option<&[u8]>,
    id: &str,
    enabled: bool,
) -> Result<Vec<u8>, AgentAssetMutationError> {
    let mut document = document(bytes)?;
    if !document.contains_key("skills") {
        document["skills"] = Item::Table(Table::new());
    }
    let skills = document
        .get_mut("skills")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(schema_error)?;
    for key in ["paths", "ignore"] {
        if skills
            .get(key)
            .map(string_array)
            .transpose()?
            .is_some_and(|items| !items.is_empty())
        {
            return Err(unavailable_error(
                AgentAssetActionUnavailableReason::UnsupportedSchema,
            ));
        }
    }
    let already_disabled = skills
        .get("disabled")
        .map(string_array)
        .transpose()?
        .is_some_and(|list| list.iter().any(|value| value.as_str() == Some(id)));
    if already_disabled != enabled {
        return Ok(bytes.unwrap_or_default().to_vec());
    }
    if !skills.contains_key("disabled") {
        skills.insert("disabled", toml_edit::value(Array::new()));
    }
    let disabled = skills
        .get_mut("disabled")
        .and_then(Item::as_array_mut)
        .ok_or_else(schema_error)?;
    if enabled {
        for index in (0..disabled.len()).rev() {
            if disabled.get(index).and_then(toml_edit::Value::as_str) == Some(id) {
                disabled.remove(index);
            }
        }
    } else {
        disabled.push(id);
    }
    Ok(document.to_string().into_bytes())
}

fn edit_plugin(
    bytes: Option<&[u8]>,
    id: &str,
    enabled: bool,
) -> Result<Vec<u8>, AgentAssetMutationError> {
    validate_config(bytes)?;
    let mut document = document(bytes)?;
    if !document.contains_key("plugins") {
        document["plugins"] = Item::Table(Table::new());
    }
    let plugins = document
        .get_mut("plugins")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(schema_error)?;
    if plugins
        .get("paths")
        .map(string_array)
        .transpose()?
        .is_some_and(|paths| !paths.is_empty())
    {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::UnsupportedSchema,
        ));
    }
    for key in ["enabled", "disabled"] {
        if plugins
            .get(key)
            .map(string_array)
            .transpose()?
            .is_some_and(|list| {
                list.iter()
                    .filter_map(toml_edit::Value::as_str)
                    .any(|name| name != id && name.rsplit('/').next() == Some(id))
            })
        {
            // The passive manifest supplies its name, not an invented native
            // installation hash. Do not silently broaden a full-id selector.
            return Err(unavailable_error(
                AgentAssetActionUnavailableReason::ScopeAmbiguous,
            ));
        }
    }
    let on = plugins
        .get("enabled")
        .map(string_array)
        .transpose()?
        .is_some_and(|list| list.iter().any(|item| item.as_str() == Some(id)));
    let off = plugins
        .get("disabled")
        .map(string_array)
        .transpose()?
        .is_some_and(|list| list.iter().any(|item| item.as_str() == Some(id)));
    if (enabled && on && !off) || (!enabled && off && !on) {
        return Ok(bytes.unwrap_or_default().to_vec());
    }
    for (key, include) in [("enabled", enabled), ("disabled", !enabled)] {
        if !plugins.contains_key(key) {
            plugins.insert(key, toml_edit::value(Array::new()));
        }
        let list = plugins
            .get_mut(key)
            .and_then(Item::as_array_mut)
            .ok_or_else(schema_error)?;
        for index in (0..list.len()).rev() {
            if list.get(index).and_then(toml_edit::Value::as_str) == Some(id) {
                list.remove(index);
            }
        }
        if include {
            list.push(id);
        }
    }
    Ok(document.to_string().into_bytes())
}

fn require_known_project_sources(
    request: MutationPreparation<'_>,
) -> Result<(), AgentAssetMutationError> {
    let Some(workspace) = request.workspace else {
        return Ok(());
    };
    for ancestor in workspace.ancestors() {
        let candidate = ancestor.join(".grok/config.toml");
        if candidate.try_exists().map_err(|_| schema_error())?
            && request.source_at(&candidate).is_err()
        {
            return Err(unavailable_error(
                AgentAssetActionUnavailableReason::ScopeAmbiguous,
            ));
        }
        if ancestor
            .join(".git")
            .try_exists()
            .map_err(|_| schema_error())?
        {
            break;
        }
    }
    Ok(())
}

fn verify(request: MutationVerification<'_>) -> bool {
    if request
        .inventory
        .assets
        .iter()
        .find(|asset| asset.stable_id == request.asset_id)
        .is_some_and(|asset| asset.category == AgentAssetCategory::Plugin)
    {
        request.parent_state_matches()
    } else {
        request.effective_state_matches()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_toggle_preserves_siblings_and_is_byte_stable_when_already_desired() {
        let original = b"# keep\n[skills]\npaths = []\ndisabled = [\"untouched\"]\n[other]\nsecret = \"fixture\"\n";
        assert_eq!(
            edit_skill(Some(original), "fixture-skill", true).unwrap(),
            original
        );
        let disabled = edit_skill(Some(original), "fixture-skill", false).unwrap();
        assert_eq!(
            edit_skill(Some(&disabled), "fixture-skill", false).unwrap(),
            disabled
        );
        let parsed = std::str::from_utf8(&disabled)
            .unwrap()
            .parse::<Document>()
            .unwrap();
        assert_eq!(parsed["other"]["secret"].as_str(), Some("fixture"));
        assert!(parsed["skills"]["paths"].as_array().unwrap().is_empty());
        let enabled = edit_skill(Some(&disabled), "fixture-skill", true).unwrap();
        let parsed = std::str::from_utf8(&enabled)
            .unwrap()
            .parse::<Document>()
            .unwrap();
        assert_eq!(parsed["skills"]["disabled"].as_array().unwrap().len(), 1);
        assert!(enabled.starts_with(b"# keep"));
    }

    #[test]
    fn malformed_or_untyped_native_controls_are_rejected() {
        assert!(edit_skill(Some(b"[skills]\ndisabled=[false]"), "fixture", false).is_err());
        assert!(validate_config(Some(b"disabled_mcp_servers = [1]")).is_err());
        assert!(validate_config(Some(b"[mcp_servers.fixture]\nenabled='false'")).is_err());
        assert!(edit_skill(Some(b"[skills]\nignore=['custom']"), "fixture", false).is_err());
        assert!(edit_plugin(
            Some(b"[plugins]\nenabled=['user/fixture/target']"),
            "target",
            false
        )
        .is_err());
    }

    #[test]
    fn plugin_toggle_keeps_other_names_and_never_grants_trust() {
        let original = b"# preserve\n[plugins]\nenabled=['fixture','other']\ndisabled=['third']\n[trust]\nallow=false\n";
        let changed = edit_plugin(Some(original), "fixture", false).unwrap();
        let parsed = document(Some(&changed)).unwrap();
        assert_eq!(parsed["trust"]["allow"].as_bool(), Some(false));
        assert_eq!(parsed["plugins"]["enabled"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["plugins"]["disabled"].as_array().unwrap().len(), 2);
        assert_eq!(
            edit_plugin(Some(&changed), "fixture", false).unwrap(),
            changed
        );
    }
}
