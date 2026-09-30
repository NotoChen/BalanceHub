use crate::{
    models::*,
    services::agent_cli::environment::mutation::{
        catalog::{
            native_records, native_removal_record, schema_error, unavailable_error,
            NativeMechanismSpec,
        },
        ExactCliCommand, MutationExecution, MutationPreparation, MutationVerification,
        PreparedMutation,
    },
};
use std::{ffi::OsString, path::Path};
use toml_edit::{value, Array, Document, InlineTable, Item, Table, TableLike, Value};

pub(super) fn mechanisms(kind: AgentCliKind) -> Vec<AgentAssetMechanismRecord> {
    let mut records = native_records(NativeMechanismSpec {
        key: "codex-mcp-enabled-toml",
        agent: kind,
        category: AgentAssetCategory::Mcp,
        schema: super::environment::PARSER_VERSION,
        source_schema: Some("TOML mcp_servers[exact native id].enabled: boolean"),
        argv: &[],
        scopes: &[AgentAssetScope::User, AgentAssetScope::Workspace],
        reload: "新建 Codex 会话后生效",
    });
    records.extend(native_records(NativeMechanismSpec {
        key: "codex-skill-enabled-toml",
        agent: kind,
        category: AgentAssetCategory::Skill,
        schema: super::environment::PARSER_VERSION,
        source_schema: Some("User TOML skills.config[] path/name selector and enabled boolean"),
        argv: &[],
        scopes: &[AgentAssetScope::User, AgentAssetScope::Workspace],
        reload: "重新加载技能列表或新建 Codex 会话后生效",
    }));
    records.extend(native_records(NativeMechanismSpec {
        key: "codex-plugin-enabled-toml",
        agent: kind,
        category: AgentAssetCategory::Plugin,
        schema: super::environment::PARSER_VERSION,
        source_schema: Some("User TOML plugins[qualified plugin key].enabled: boolean"),
        argv: &[],
        scopes: &[AgentAssetScope::User],
        reload: "新建 Codex 会话后生效；保留插件信任与各子资产策略",
    }));
    records.push(native_removal_record(NativeMechanismSpec {
        key: "codex-plugin-native-cli",
        agent: kind,
        category: AgentAssetCategory::Plugin,
        schema: super::environment::PARSER_VERSION,
        source_schema: Some("Codex 原生插件包卸载；按 PLUGIN@MARKETPLACE 精确定位并移除本地缓存"),
        argv: &["plugin", "remove", "<plugin@marketplace>"],
        scopes: &[AgentAssetScope::User],
        reload: "卸载插件并清理其本地缓存；插件提供的 Skill、MCP、Hook 将随插件重新盘点",
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
    if request.action == AgentAssetActionKind::Remove {
        return prepare_removal(request);
    }
    let enabled = request.desired_enabled()?;
    if request.asset.scope == AgentAssetScope::Workspace
        && request.context.trust_context != AgentTrustState::Trusted
    {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::TrustRequired,
        ));
    }
    let config_path = Path::new(&request.context.config_root).join("config.toml");
    let source = if request.asset.category == AgentAssetCategory::Mcp {
        request.source(&request.asset.inspection_source_id)?
    } else {
        request.source_at(&config_path)?
    };
    if !source.writable
        || !matches!(
            source.scope,
            AgentAssetScope::User | AgentAssetScope::Workspace
        )
    {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::UnsupportedScope,
        ));
    }
    let file = request.file(source)?;
    let raw = std::str::from_utf8(file.bytes().unwrap_or_default()).map_err(|_| schema_error())?;
    let replacement = match request.asset.category {
        AgentAssetCategory::Mcp => edit_mcp(raw, &request.asset.native_id, enabled)?,
        AgentAssetCategory::Skill => {
            let skill_path = request
                .asset
                .path
                .as_deref()
                .map(Path::new)
                .ok_or_else(schema_error)?;
            if skill_path.file_name().is_none_or(|name| name != "SKILL.md") {
                return Err(unavailable_error(
                    AgentAssetActionUnavailableReason::ScopeAmbiguous,
                ));
            }
            edit_skill(raw, skill_path, &request.asset.native_id, enabled)?
        }
        AgentAssetCategory::Plugin => edit_plugin(raw, &request.asset.native_id, enabled)?,
        category => {
            return Err(unavailable_error(unavailable_reason(
                category,
                request.action,
            )))
        }
    };
    let mut files = request.context_files()?;
    if !files
        .iter()
        .any(|existing| existing.source_id == file.source_id)
    {
        files.push(file);
    }
    PreparedMutation::new(
        request,
        MutationExecution::AtomicFile {
            source_id: source.id.clone(),
            replacement: replacement.into_bytes(),
        },
        files,
        vec![source.id.clone()],
        vec![request.boolean_change(Path::new(&source.path), "原生启用状态")],
        request.asset.relationships.affected_asset_ids.clone(),
        verify,
    )
}

fn prepare_removal(
    request: MutationPreparation<'_>,
) -> Result<PreparedMutation, AgentAssetMutationError> {
    if request.asset.category != AgentAssetCategory::Plugin
        || request.asset.scope != AgentAssetScope::User
    {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::UnsupportedScope,
        ));
    }
    let id = crate::services::agent_cli::environment::mutation::catalog::safe_native_id(
        &request.asset.native_id,
    )?;
    if !id.split_once('@').is_some_and(|(name, marketplace)| {
        !name.is_empty() && !marketplace.is_empty() && !marketplace.contains('@')
    }) {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::ScopeAmbiguous,
        ));
    }
    let root = Path::new(&request.context.config_root);
    let source = request.source_at(&root.join("config.toml"))?;
    let mut files = request.context_files()?;
    let file = request.file(source)?;
    if !files.iter().any(|existing| existing.source_id == source.id) {
        files.push(file.clone());
    }
    let raw =
        std::str::from_utf8(file.bytes().ok_or_else(schema_error)?).map_err(|_| schema_error())?;
    let document = document(raw)?;
    let plugins = document
        .get("plugins")
        .and_then(Item::as_table_like)
        .ok_or_else(schema_error)?;
    if plugins.get(id).is_none() {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::AssetNotInstalled,
        ));
    }
    let command = ExactCliCommand::new(
        request.installation,
        vec!["plugin".into(), "remove".into(), id.into()],
        request.cwd(),
        vec![
            (OsString::from("HOME"), request.home.as_os_str().to_owned()),
            (OsString::from("CODEX_HOME"), root.as_os_str().to_owned()),
        ],
    )?;
    PreparedMutation::new(
        request,
        MutationExecution::ExactCli(Box::new(command)),
        files,
        vec![source.id.clone()],
        vec![AgentAssetPlanChange {
            label: "通过 Codex 原生 CLI 卸载插件".into(),
            path: Some(source.path.clone()),
            before: Some(id.to_owned()),
            after: Some("插件注册与本地缓存由 Codex 清理".into()),
        }],
        request.asset.relationships.affected_asset_ids.clone(),
        verify,
    )
}

fn document(raw: &str) -> Result<Document, AgentAssetMutationError> {
    raw.parse().map_err(|_| schema_error())
}

fn edit_mcp(raw: &str, id: &str, enabled: bool) -> Result<String, AgentAssetMutationError> {
    let mut document = document(raw)?;
    let entry = document
        .get_mut("mcp_servers")
        .and_then(Item::as_table_like_mut)
        .and_then(|servers| servers.get_mut(id))
        .and_then(Item::as_table_like_mut)
        .ok_or_else(schema_error)?;
    if entry
        .get("enabled")
        .is_some_and(|item| item.as_bool().is_none())
    {
        return Err(schema_error());
    }
    if entry.get("enabled").and_then(Item::as_bool) == Some(enabled) {
        return Ok(raw.to_owned());
    }
    entry.insert("enabled", value(enabled));
    Ok(document.to_string())
}

fn edit_plugin(raw: &str, key: &str, enabled: bool) -> Result<String, AgentAssetMutationError> {
    // Codex identifies installed plugins by their qualified config key. Cache
    // directory names alone never grant permission to create a setting.
    if !key.split_once('@').is_some_and(|(name, marketplace)| {
        !name.is_empty() && !marketplace.is_empty() && !marketplace.contains('@')
    }) || key.chars().any(char::is_control)
    {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::ScopeAmbiguous,
        ));
    }
    let mut document = document(raw)?;
    if !document.contains_key("plugins") {
        document["plugins"] = Item::Table(Table::new());
    }
    let plugins = document
        .get_mut("plugins")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(schema_error)?;
    if !plugins.contains_key(key) {
        plugins.insert(key, Item::Table(Table::new()));
    }
    let entry = plugins
        .get_mut(key)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(schema_error)?;
    if entry
        .get("enabled")
        .is_some_and(|item| item.as_bool().is_none())
    {
        return Err(schema_error());
    }
    if entry.get("enabled").and_then(Item::as_bool) == Some(enabled) {
        return Ok(raw.to_owned());
    }
    entry.insert("enabled", value(enabled));
    Ok(document.to_string())
}

fn edit_skill(
    raw: &str,
    path: &Path,
    name: &str,
    enabled: bool,
) -> Result<String, AgentAssetMutationError> {
    if !path.is_absolute() {
        return Err(schema_error());
    }
    let path = path.to_str().ok_or_else(schema_error)?;
    let mut document = document(raw)?;
    if !document.contains_key("skills") {
        document["skills"] = Item::Table(Table::new());
    }
    let skills = document
        .get_mut("skills")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(schema_error)?;
    if !skills.contains_key("config") {
        skills.insert("config", Item::Value(Value::Array(Array::new())));
    }
    let entries = skills.get_mut("config").ok_or_else(schema_error)?;
    let plan = match &*entries {
        Item::ArrayOfTables(tables) => plan_skill_rule(
            tables.iter().map(|entry| Ok(entry as &dyn TableLike)),
            path,
            name,
            enabled,
        )?,
        Item::Value(Value::Array(values)) => plan_skill_rule(
            values.iter().map(|entry| {
                entry
                    .as_inline_table()
                    .map(|entry| entry as &dyn TableLike)
                    .ok_or_else(schema_error)
            }),
            path,
            name,
            enabled,
        )?,
        _ => return Err(schema_error()),
    };
    if plan.unchanged {
        return Ok(raw.to_owned());
    }
    match entries {
        Item::ArrayOfTables(entries) => {
            let mut entry = plan
                .selected
                .and_then(|index| entries.get(index))
                .cloned()
                .unwrap_or_else(|| {
                    let mut entry = Table::new();
                    entry.insert("path", value(path));
                    entry
                });
            for index in plan.matching_paths.into_iter().rev() {
                entries.remove(index);
            }
            entry.insert("enabled", value(enabled));
            entries.push(entry);
        }
        Item::Value(Value::Array(entries)) => {
            let mut entry = plan
                .selected
                .and_then(|index| entries.get(index))
                .and_then(Value::as_inline_table)
                .cloned()
                .unwrap_or_else(|| {
                    let mut entry = InlineTable::new();
                    entry.insert("path", Value::from(path));
                    entry
                });
            for index in plan.matching_paths.into_iter().rev() {
                entries.remove(index);
            }
            entry.insert("enabled", Value::from(enabled));
            entries.push(entry);
        }
        _ => return Err(schema_error()),
    }
    Ok(document.to_string())
}

struct SkillRulePlan {
    selected: Option<usize>,
    matching_paths: Vec<usize>,
    unchanged: bool,
}

fn plan_skill_rule<'a>(
    entries: impl Iterator<Item = Result<&'a dyn TableLike, AgentAssetMutationError>>,
    path: &str,
    name: &str,
    enabled: bool,
) -> Result<SkillRulePlan, AgentAssetMutationError> {
    let mut selected = None;
    let mut last_matching_state = None;
    let mut matching_paths = Vec::new();
    let mut count = 0usize;
    for (index, entry) in entries.enumerate() {
        count += 1;
        let entry = entry?;
        if entry.get("enabled").and_then(Item::as_bool).is_none()
            || entry
                .get("path")
                .is_some_and(|item| item.as_str().is_none())
            || entry
                .get("name")
                .is_some_and(|item| item.as_str().is_none())
        {
            return Err(schema_error());
        }
        let entry_path = entry.get("path").and_then(Item::as_str);
        let entry_name = entry.get("name").and_then(Item::as_str);
        // Codex ignores entries with both/neither selectors. Preserve them,
        // and preserve name selectors which may govern other skill files.
        if entry_name.is_none() && entry_path == Some(path) {
            selected = Some(index);
            matching_paths.push(index);
            last_matching_state = entry.get("enabled").and_then(Item::as_bool);
        } else if entry_path.is_none()
            && entry_name.is_some_and(|entry_name| entry_name.trim() == name)
        {
            last_matching_state = entry.get("enabled").and_then(Item::as_bool);
        }
    }
    let unchanged = matching_paths.len() == 1
        && last_matching_state == Some(enabled)
        && selected == count.checked_sub(1);
    Ok(SkillRulePlan {
        selected,
        matching_paths,
        unchanged,
    })
}

fn verify(request: MutationVerification<'_>) -> bool {
    if request.action == AgentAssetActionKind::Remove {
        return request.asset_removed();
    }
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
    fn quoted_mcp_id_and_secrets_are_byte_preserved_outside_the_boolean() {
        let raw = "# keep\n[mcp_servers.\"a.b\"]\ncommand = 'runner'\nenabled = true\nsecret = 'SENSITIVE_FIXTURE'\n[mcp_servers.other]\ncommand = 'other'\n";
        let changed = edit_mcp(raw, "a.b", false).unwrap();
        assert_eq!(changed, raw.replace("enabled = true", "enabled = false"));
        assert_eq!(edit_mcp(&changed, "a.b", false).unwrap(), changed);
        assert!(edit_mcp(raw, "absent", false).is_err());
    }
    #[test]
    fn plugin_and_skill_edits_preserve_unrelated_native_nodes() {
        let raw = "extra = 'keep'\n[plugins.\"fixture@local\"]\nenabled = true\n";
        assert!(edit_plugin(raw, "fixture@local", false)
            .unwrap()
            .contains("extra = 'keep'"));
        assert!(edit_plugin(raw, "unqualified-cache-folder", false).is_err());
        let path = std::env::temp_dir().join("balancehub-fixture/skills/one/SKILL.md");
        let changed = edit_skill("extra = 'keep'\n", &path, "one", false).unwrap();
        assert!(changed.contains("enabled = false"));
        assert!(changed.contains("extra = 'keep'"));
        assert_eq!(edit_skill(&changed, &path, "one", false).unwrap(), changed);
    }
    #[test]
    fn later_path_rule_overrides_broad_name_without_changing_other_skills() {
        let path = std::env::temp_dir().join("balancehub-fixture/SKILL.md");
        let quoted_path = Value::from(path.to_str().unwrap()).to_string();
        let raw = format!("[[skills.config]]\npath = {quoted_path}\nenabled = true\n[[skills.config]]\nname = 'one'\nenabled = false\n[[skills.config]]\npath = {quoted_path}\nenabled = false\n");
        let changed = edit_skill(&raw, &path, "one", true).unwrap();
        let document = document(&changed).unwrap();
        let entries = document["skills"]["config"].as_array_of_tables().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries.get(0).unwrap()["name"].as_str(), Some("one"));
        assert_eq!(entries.get(0).unwrap()["enabled"].as_bool(), Some(false));
        assert_eq!(entries.get(1).unwrap()["path"].as_str(), path.to_str());
        assert_eq!(entries.get(1).unwrap()["enabled"].as_bool(), Some(true));
        assert_eq!(edit_skill(&changed, &path, "one", true).unwrap(), changed);
    }

    #[test]
    fn inline_skill_rule_arrays_preserve_broad_selectors_unknown_values_and_order() {
        let path = std::env::temp_dir().join("balancehub-fixture/SKILL.md");
        let quoted_path = Value::from(path.to_str().unwrap()).to_string();
        for raw in [
            "[skills]\nconfig = []\n",
            "skills = {config = [], keep = 'yes'}\n",
        ] {
            let changed = edit_skill(raw, &path, "one", false).unwrap();
            let parsed = document(&changed).unwrap();
            let entries = parsed["skills"]["config"].as_array().unwrap();
            assert_eq!(entries.len(), 1);
            let entry = entries.get(0).unwrap().as_inline_table().unwrap();
            assert_eq!(entry.get("path").and_then(Value::as_str), path.to_str());
            assert_eq!(entry.get("enabled").and_then(Value::as_bool), Some(false));
            assert_eq!(edit_skill(&changed, &path, "one", false).unwrap(), changed);
        }
        let raw = format!("# preserve\n[skills]\nconfig = [{{path={quoted_path},enabled=false}}, {{name='one',enabled=false,unknown='broad'}}, {{path={quoted_path},enabled=false,unknown='target'}}, {{path='ignored',name='ignored',enabled=true,unknown='ignored'}}]\nkeep = 'yes'\n");
        let changed = edit_skill(&raw, &path, "one", true).unwrap();
        let parsed = document(&changed).unwrap();
        let entries = parsed["skills"]["config"].as_array().unwrap();
        assert_eq!(entries.len(), 3);
        let entry = |index| entries.get(index).unwrap().as_inline_table().unwrap();
        assert_eq!(entry(0).get("name").and_then(Value::as_str), Some("one"));
        assert_eq!(
            entry(0).get("enabled").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            entry(0).get("unknown").and_then(Value::as_str),
            Some("broad")
        );
        assert_eq!(
            entry(1).get("unknown").and_then(Value::as_str),
            Some("ignored")
        );
        assert_eq!(entry(2).get("path").and_then(Value::as_str), path.to_str());
        assert_eq!(entry(2).get("enabled").and_then(Value::as_bool), Some(true));
        assert_eq!(
            entry(2).get("unknown").and_then(Value::as_str),
            Some("target")
        );
        assert_eq!(parsed["skills"]["keep"].as_str(), Some("yes"));
        assert!(changed.starts_with("# preserve"));
        assert_eq!(edit_skill(&changed, &path, "one", true).unwrap(), changed);
        for raw in [
            "[skills]\nconfig=[1]\n",
            "[skills]\nconfig=[{path='x',enabled='false'}]\n",
            "[skills]\nconfig={}\n",
        ] {
            assert!(edit_skill(raw, &path, "one", false).is_err());
        }
    }
}
