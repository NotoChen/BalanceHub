use crate::services::agent_cli::environment::config_document::{parse, ConfigDocumentFormat};
use crate::{
    models::*,
    services::agent_cli::environment::mutation::{
        catalog::{
            native_records, native_removal_record, safe_native_id, schema_error, unavailable_error,
            NativeMechanismSpec,
        },
        ExactCliCommand, GuardedFile, MutationExecution, MutationPreparation, MutationVerification,
        PreparedMutation,
    },
};
use serde_json::Value;
use std::{collections::BTreeSet, ffi::OsString, path::Path};

pub(super) fn mechanisms(kind: AgentCliKind) -> Vec<AgentAssetMechanismRecord> {
    let mut result = native_records(NativeMechanismSpec {
        key: "gemini-mcp-persistent-cli",
        agent: kind,
        category: AgentAssetCategory::Mcp,
        schema: super::environment::PARSER_VERSION,
        source_schema: Some("mcp-server-enablement.json: normalized id -> {enabled:boolean}"),
        argv: &["mcp", "enable|disable", "<normalized-id>"],
        scopes: &[AgentAssetScope::User, AgentAssetScope::Workspace],
        reload: "影响该 Gemini 配置目录的所有工作区；已有会话需 /mcp reload 或新建会话",
    });
    result.extend(native_records(NativeMechanismSpec {
        key: "gemini-extension-persistent-cli",
        agent: kind,
        category: AgentAssetCategory::Extension,
        schema: super::environment::PARSER_VERSION,
        source_schema: Some(
            "extensions/extension-enablement.json; enabling also clears same-name MCP disablement",
        ),
        argv: &[
            "extensions",
            "enable|disable",
            "<name>",
            "--scope",
            "user|workspace",
        ],
        scopes: &[AgentAssetScope::User, AgentAssetScope::Workspace],
        reload: "按计划指定范围更新扩展；启用会同时解除扩展声明的同名 MCP 在全部工作区的个人禁用，已有会话需重新加载",
    }));
    result.extend(native_records(NativeMechanismSpec {
        key: "gemini-skill-persistent-cli", agent: kind, category: AgentAssetCategory::Skill, schema: super::environment::PARSER_VERSION, source_schema: Some("settings.json skills.disabled case-insensitive matching; native enable removes exact Workspace and User entries"),
        argv: &["skills", "enable|disable", "<name>"], scopes: &[AgentAssetScope::User, AgentAssetScope::Workspace],
        reload: "已有会话需重新加载技能或新建会话；启用会解除用户及当前工作区的个人禁用",
    }));
    result.push(native_removal_record(NativeMechanismSpec {
        key: "gemini-extension-native-cli",
        agent: kind,
        category: AgentAssetCategory::Extension,
        schema: super::environment::PARSER_VERSION,
        source_schema: Some("Gemini CLI 原生扩展卸载；按扩展名称移除已安装扩展"),
        argv: &["extensions", "uninstall", "<name>"],
        scopes: &[AgentAssetScope::User],
        reload: "卸载扩展并清理其本地目录；扩展提供的 Skill、MCP、Hook 将随插件重新盘点",
    }));
    result
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
    let id = safe_native_id(&request.asset.native_id)?;
    let root = Path::new(&request.context.config_root);
    if root.file_name().is_none_or(|name| name != ".gemini") {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::ScopeAmbiguous,
        ));
    }
    if request.workspace.is_some() && request.context.trust_context != AgentTrustState::Trusted {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::TrustRequired,
        ));
    }
    let mut files = request.context_files()?;
    let mut writes = Vec::new();
    let mut changes = Vec::new();
    let mut affected = vec![];
    let verb = if enabled { "enable" } else { "disable" };
    let mut argv = match request.asset.category {
        AgentAssetCategory::Mcp => {
            let source = request.source_at(&root.join("mcp-server-enablement.json"))?;
            add_file(request, source, &mut files)?;
            let file = files
                .iter()
                .find(|file| file.source_id == source.id)
                .ok_or_else(schema_error)?;
            validate_mcp_enablement(file.bytes())?;
            writes.push(source.id.clone());
            changes.push(
                request.boolean_change(Path::new(&source.path), "全工作区共享的 MCP 启用状态"),
            );
            let normalized = id.trim().to_lowercase();
            affected.extend(
                request
                    .inventory
                    .assets
                    .iter()
                    .filter(|asset| {
                        asset.agent_kind == request.asset.agent_kind
                            && asset.context_id == request.context.id
                            && asset.category == AgentAssetCategory::Mcp
                            && asset.native_id.trim().to_lowercase() == normalized
                    })
                    .map(|asset| asset.stable_id.clone()),
            );
            vec!["mcp".into(), verb.into(), normalized]
        }
        AgentAssetCategory::Extension => {
            let source = request.source_at(&root.join("extensions/extension-enablement.json"))?;
            add_file(request, source, &mut files)?;
            validate_extension_enablement(
                files
                    .iter()
                    .find(|file| file.source_id == source.id)
                    .and_then(GuardedFile::bytes),
            )?;
            writes.push(source.id.clone());
            changes.push(request.boolean_change(
                Path::new(&source.path),
                if request.workspace.is_some() {
                    "当前工作区的扩展启用状态"
                } else {
                    "用户范围的扩展启用状态"
                },
            ));
            affected.extend(
                request
                    .asset
                    .relationships
                    .affected_asset_ids
                    .iter()
                    .cloned(),
            );
            if enabled {
                let names = extension_mcp_names(request.inventory, request.asset)?;
                affected.extend(
                    matching_mcp_assets(request.inventory, request.asset, &names)
                        .map(|asset| asset.stable_id.clone()),
                );
                // A settings definition may replace the extension definition.
                // Native autoEnableServers still changes its global name switch.
                // Guard its definitions and policy inputs even without providedBy.
                for source in request.inventory.sources.iter().filter(|source| {
                    source.context_id == request.context.id
                        && source.source_kind == AgentAssetSourceKind::File
                        && source.categories.contains(&AgentAssetCategory::Mcp)
                }) {
                    add_file(request, source, &mut files)?;
                }
                let mcp_source = request.source_at(&root.join("mcp-server-enablement.json"))?;
                add_file(request, mcp_source, &mut files)?;
                validate_mcp_enablement(
                    files
                        .iter()
                        .find(|file| file.source_id == mcp_source.id)
                        .and_then(GuardedFile::bytes),
                )?;
                writes.push(mcp_source.id.clone());
                changes.push(AgentAssetPlanChange {
                    label: "同时解除扩展声明的同名 MCP 在全部工作区的个人禁用".into(),
                    path: Some(mcp_source.path.clone()),
                    before: None,
                    after: Some("保留管理策略与信任限制".into()),
                });
            }
            vec![
                "extensions".into(),
                verb.into(),
                id.into(),
                "--scope".into(),
                if request.workspace.is_some() {
                    "workspace"
                } else {
                    "user"
                }
                .into(),
            ]
        }
        AgentAssetCategory::Skill => {
            let user = request.source_at(&root.join("settings.json"))?;
            let user_file = add_file(request, user, &mut files)?;
            validate_skill_enablement(user_file.bytes(), id, enabled)?;
            if enabled || request.asset.scope == AgentAssetScope::User {
                writes.push(user.id.clone());
                changes
                    .push(request.boolean_change(Path::new(&user.path), "用户范围的技能禁用名单"));
            }
            if let Some(workspace) = request.workspace {
                let project = request.source_at(&workspace.join(".gemini/settings.json"))?;
                let project_file = add_file(request, project, &mut files)?;
                validate_skill_enablement(project_file.bytes(), id, enabled)?;
                if enabled || request.asset.scope == AgentAssetScope::Workspace {
                    writes.push(project.id.clone());
                    changes.push(
                        request
                            .boolean_change(Path::new(&project.path), "当前工作区的技能禁用名单"),
                    );
                }
            }
            affected.extend(
                request
                    .inventory
                    .assets
                    .iter()
                    .filter(|asset| {
                        asset.context_id == request.context.id
                            && asset.category == AgentAssetCategory::Skill
                            && asset.native_id.to_lowercase() == id.to_lowercase()
                    })
                    .map(|asset| asset.stable_id.clone()),
            );
            let mut args = vec!["skills".into(), verb.into(), id.into()];
            if !enabled {
                args.push("--scope".into());
                args.push(
                    if request.asset.scope == AgentAssetScope::Workspace {
                        "workspace"
                    } else {
                        "user"
                    }
                    .into(),
                );
            }
            args
        }
        category => {
            return Err(unavailable_error(unavailable_reason(
                category,
                request.action,
            )))
        }
    };
    if argv.iter().any(|arg| arg == "--session") {
        return Err(schema_error());
    }
    let command = ExactCliCommand::new(
        request.installation,
        std::mem::take(&mut argv),
        request.cwd(),
        vec![
            (OsString::from("HOME"), request.home.as_os_str().to_owned()),
            (
                OsString::from("GEMINI_CLI_HOME"),
                root.parent()
                    .ok_or_else(schema_error)?
                    .as_os_str()
                    .to_owned(),
            ),
        ],
    )?;
    PreparedMutation::new(
        request,
        MutationExecution::ExactCli(Box::new(command)),
        files,
        writes,
        changes,
        affected,
        verify,
    )
}

fn prepare_removal(
    request: MutationPreparation<'_>,
) -> Result<PreparedMutation, AgentAssetMutationError> {
    if request.asset.category != AgentAssetCategory::Extension
        || request.asset.scope != AgentAssetScope::User
    {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::UnsupportedScope,
        ));
    }
    let id = safe_native_id(&request.asset.native_id)?;
    let root = Path::new(&request.context.config_root);
    if root.file_name().is_none_or(|name| name != ".gemini") {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::ScopeAmbiguous,
        ));
    }
    let source = request.source_at(&root.join("extensions/extension-enablement.json"))?;
    let mut files = request.context_files()?;
    add_file(request, source, &mut files)?;
    validate_extension_enablement(
        files
            .iter()
            .find(|file| file.source_id == source.id)
            .and_then(GuardedFile::bytes),
    )?;
    let command = ExactCliCommand::new(
        request.installation,
        vec!["extensions".into(), "uninstall".into(), id.into()],
        request.cwd(),
        vec![
            (OsString::from("HOME"), request.home.as_os_str().to_owned()),
            (
                OsString::from("GEMINI_CLI_HOME"),
                root.parent()
                    .ok_or_else(schema_error)?
                    .as_os_str()
                    .to_owned(),
            ),
        ],
    )?;
    PreparedMutation::new(
        request,
        MutationExecution::ExactCli(Box::new(command)),
        files,
        vec![source.id.clone()],
        vec![AgentAssetPlanChange {
            label: "通过 Gemini CLI 原生命令卸载扩展".into(),
            path: Some(root.join("extensions").to_string_lossy().into_owned()),
            before: Some(id.to_owned()),
            after: Some("扩展目录与原生注册状态由 Gemini CLI 清理".into()),
        }],
        request.asset.relationships.affected_asset_ids.clone(),
        verify,
    )
}

fn add_file<'a>(
    request: MutationPreparation<'_>,
    source: &AgentAssetSource,
    files: &'a mut Vec<GuardedFile>,
) -> Result<&'a GuardedFile, AgentAssetMutationError> {
    if !files.iter().any(|file| file.source_id == source.id) {
        files.push(request.file(source)?);
    }
    files
        .iter()
        .find(|file| file.source_id == source.id)
        .ok_or_else(schema_error)
}

fn object(bytes: Option<&[u8]>) -> Result<serde_json::Map<String, Value>, AgentAssetMutationError> {
    match bytes {
        None => Ok(serde_json::Map::new()),
        Some(bytes) => parse(bytes, ConfigDocumentFormat::Json)
            .and_then(|value| value.as_object().cloned())
            .ok_or_else(schema_error),
    }
}

fn validate_mcp_enablement(bytes: Option<&[u8]>) -> Result<(), AgentAssetMutationError> {
    for (id, value) in object(bytes)? {
        let entry = value.as_object().ok_or_else(schema_error)?;
        if id.is_empty()
            || id != id.trim().to_lowercase()
            || entry.len() != 1
            || entry.get("enabled").and_then(Value::as_bool).is_none()
        {
            return Err(schema_error());
        }
    }
    Ok(())
}

fn validate_extension_enablement(bytes: Option<&[u8]>) -> Result<(), AgentAssetMutationError> {
    for (_, value) in object(bytes)? {
        let entry = value.as_object().ok_or_else(schema_error)?;
        // Native zod parsing strips unknown fields. Reject them before invoking
        // a command that would otherwise rewrite and silently discard them.
        if entry.len() != 1
            || !entry
                .get("overrides")
                .and_then(Value::as_array)
                .is_some_and(|items| items.iter().all(Value::is_string))
        {
            return Err(schema_error());
        }
    }
    Ok(())
}

fn validate_skills(bytes: Option<&[u8]>) -> Result<(), AgentAssetMutationError> {
    let root = object(bytes)?;
    if let Some(skills) = root.get("skills") {
        let skills = skills.as_object().ok_or_else(schema_error)?;
        if let Some(disabled) = skills.get("disabled") {
            if !disabled
                .as_array()
                .is_some_and(|list| list.iter().all(Value::is_string))
            {
                return Err(schema_error());
            }
        }
    }
    Ok(())
}

fn validate_skill_enablement(
    bytes: Option<&[u8]>,
    id: &str,
    enabled: bool,
) -> Result<(), AgentAssetMutationError> {
    validate_skills(bytes)?;
    if enabled
        && object(bytes)?
            .get("skills")
            .and_then(|value| value.get("disabled"))
            .and_then(Value::as_array)
            .is_some_and(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|name| name != id && name.to_lowercase() == id.to_lowercase())
            })
    {
        // State matching folds case, while this native command removes exact
        // strings. Do not report a reliable enable for a surviving alias.
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::ScopeAmbiguous,
        ));
    }
    Ok(())
}

fn verify(request: MutationVerification<'_>) -> bool {
    if request.action == AgentAssetActionKind::Remove {
        return request.asset_removed();
    }
    let Some(parent) = request
        .inventory
        .assets
        .iter()
        .find(|asset| asset.stable_id == request.asset_id)
    else {
        return false;
    };
    if parent.category != AgentAssetCategory::Extension {
        return request.effective_state_matches();
    }
    if !request.effective_state_matches() {
        return false;
    }
    if request.action != AgentAssetActionKind::Enable {
        return request.parent_state_matches();
    }
    let Ok(names) = extension_mcp_names(request.inventory, parent) else {
        return false;
    };
    let mut children = vec![parent.stable_id.clone()];
    for id in request
        .affected_asset_ids
        .iter()
        .filter(|id| *id != &parent.stable_id)
    {
        let Some(asset) = request
            .inventory
            .assets
            .iter()
            .find(|asset| &asset.stable_id == id)
        else {
            return false;
        };
        if request.is_provided_child(asset) {
            children.push(id.clone());
        } else if asset.context_id != parent.context_id
            || asset.category != AgentAssetCategory::Mcp
            || !names.contains(&normalized_mcp_name(&asset.native_id))
        {
            return false;
        }
    }
    // Only actual descendants use parent gating. A settings winner retains its
    // own trust, policy and resolution while its native name switch is verified.
    MutationVerification {
        inventory: request.inventory,
        asset_id: request.asset_id,
        action: request.action,
        affected_asset_ids: &children,
    }
    .parent_state_matches()
        && matching_mcp_assets(request.inventory, parent, &names)
            .all(|asset| request.affected_asset_ids.contains(&asset.stable_id))
        && mcp_names_enabled(request.inventory, parent, &names)
}

fn normalized_mcp_name(name: &str) -> String {
    name.trim().to_lowercase()
}

fn matching_mcp_assets<'a>(
    inventory: &'a AgentEnvironmentInventory,
    parent: &'a AgentAssetRecord,
    names: &'a BTreeSet<String>,
) -> impl Iterator<Item = &'a AgentAssetRecord> {
    inventory.assets.iter().filter(move |asset| {
        asset.agent_kind == parent.agent_kind
            && asset.context_id == parent.context_id
            && asset.category == AgentAssetCategory::Mcp
            && names.contains(&normalized_mcp_name(&asset.native_id))
    })
}

fn extension_mcp_names(
    inventory: &AgentEnvironmentInventory,
    parent: &AgentAssetRecord,
) -> Result<BTreeSet<String>, AgentAssetMutationError> {
    let definitions = inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.context_id == parent.context_id
                && declaration.native_kind == AgentAssetCategory::Extension
                && declaration.native_id == parent.native_id
                && declaration.role == AgentAssetDeclarationRole::Definition
                && parent.represented_declaration_ids.contains(&declaration.id)
        })
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return Err(schema_error());
    };
    let expected = definition
        .explicitly_affected
        .iter()
        .filter(|reference| reference.category == AgentAssetCategory::Mcp)
        .map(|reference| normalized_mcp_name(&reference.native_id))
        .collect::<BTreeSet<_>>();
    let parent_ref = AgentAssetNativeRef {
        category: AgentAssetCategory::Extension,
        native_id: parent.native_id.clone(),
        qualifier: Some(format!("extension:{}", parent.native_id)),
    };
    // Include represented definitions from replaced rows, not just winners or
    // contributor IDs. They are the names passed by the native extension CLI.
    let represented = inventory
        .assets
        .iter()
        .filter(|asset| {
            asset.context_id == parent.context_id && asset.category == AgentAssetCategory::Mcp
        })
        .flat_map(|asset| asset.represented_declaration_ids.iter())
        .collect::<BTreeSet<_>>();
    let observed = inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.context_id == parent.context_id
                && declaration.source_id == definition.source_id
                && declaration.native_kind == AgentAssetCategory::Mcp
                && declaration.role == AgentAssetDeclarationRole::Definition
                && declaration.provided_by.as_ref() == Some(&parent_ref)
                && represented.contains(&declaration.id)
        })
        .map(|declaration| normalized_mcp_name(&declaration.native_id))
        .collect::<BTreeSet<_>>();
    if expected != observed || expected.contains("") {
        return Err(schema_error());
    }
    Ok(expected)
}

fn mcp_names_enabled(
    inventory: &AgentEnvironmentInventory,
    parent: &AgentAssetRecord,
    names: &BTreeSet<String>,
) -> bool {
    let Some(context) = inventory
        .contexts
        .iter()
        .find(|context| context.id == parent.context_id)
    else {
        return false;
    };
    let path = Path::new(&context.config_root).join("mcp-server-enablement.json");
    let Some(source) = inventory
        .sources
        .iter()
        .find(|source| source.context_id == context.id && Path::new(&source.path) == path)
    else {
        return false;
    };
    if source.source_kind != AgentAssetSourceKind::File
        || source.revision.is_symlink
        || !source.diagnostics.is_empty()
    {
        return false;
    }
    names.iter().all(|name| {
        let overlays = inventory
            .declarations
            .iter()
            .filter(|declaration| {
                declaration.context_id == context.id
                    && declaration.source_id == source.id
                    && declaration.native_kind == AgentAssetCategory::Mcp
                    && normalized_mcp_name(&declaration.native_id) == *name
            })
            .collect::<Vec<_>>();
        // The validated native file defaults to enabled when no entry exists.
        overlays.is_empty()
            || (overlays.len() == 1
                && overlays[0].role == AgentAssetDeclarationRole::StateOverlay
                && overlays[0].declared_state == AgentAssetDeclaredState::Enabled
                && overlays[0].diagnostics.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_global_enablement_and_lossy_extension_schema_are_rejected() {
        assert!(validate_mcp_enablement(Some(b"{")).is_err());
        assert!(validate_mcp_enablement(Some(br#"{"server":{"enabled":"false"}}"#)).is_err());
        assert!(validate_mcp_enablement(Some(br#"{"server":{"enabled":false}}"#)).is_ok());
        assert!(validate_extension_enablement(Some(
            br#"{"ext":{"overrides":[],"secret":"fixture"}}"#
        ))
        .is_err());
        assert!(validate_extension_enablement(Some(br#"{"ext":{"overrides":["!*"]}}"#)).is_ok());
        assert!(
            validate_mcp_enablement(Some(br#"{"server":{"enabled":false,"enabled":true}}"#))
                .is_err()
        );
    }
    #[test]
    fn skill_lists_are_exact_strings_and_preserve_unrelated_settings() {
        assert!(validate_skills(Some(
            br#"{"skills":{"disabled":["Case-Sensitive"]},"unknown":{"secret":"fixture"}}"#
        ))
        .is_ok());
        assert!(validate_skills(Some(br#"{"skills":{"disabled":[true]}}"#)).is_err());
        assert!(validate_skill_enablement(
            Some(br#"{"skills":{"disabled":["Fixture"]}}"#),
            "fixture",
            true
        )
        .is_err());
        assert!(validate_skill_enablement(
            Some(br#"{"skills":{"disabled":["fixture"]}}"#),
            "fixture",
            true
        )
        .is_ok());
    }
}
