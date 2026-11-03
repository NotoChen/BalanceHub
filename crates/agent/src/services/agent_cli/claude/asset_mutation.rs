use crate::{
    models::*,
    services::agent_cli::environment::{
        config_document::{parse, ConfigDocumentFormat},
        mutation::{
            catalog::{
                native_records, native_removal_record, safe_native_id, schema_error,
                unavailable_error, NativeMechanismSpec,
            },
            ExactCliCommand, MutationExecution, MutationPreparation, MutationVerification,
            PreparedMutation,
        },
    },
};
use serde_json::Value;
use std::{ffi::OsString, path::Path};

pub(super) fn mechanisms(kind: AgentCliKind) -> Vec<AgentAssetMechanismRecord> {
    let mut result = native_records(NativeMechanismSpec {
        key: "claude-plugin-persistent-cli",
        agent: kind,
        category: AgentAssetCategory::Plugin,
        schema: super::environment::PARSER_VERSION,
        source_schema: Some("settings.json enabledPlugins[qualified plugin id]: boolean"),
        argv: &[
            "plugin",
            "enable|disable",
            "<qualified-plugin-id>",
            "--scope",
            "user|project|local",
        ],
        scopes: &[
            AgentAssetScope::User,
            AgentAssetScope::Workspace,
            AgentAssetScope::Local,
        ],
        reload: "按明确范围更新 Plugin 状态；现有会话需重新加载 Plugin 或新建会话",
    });
    result.push(native_removal_record(NativeMechanismSpec {
        key: "claude-plugin-native-cli",
        agent: kind,
        category: AgentAssetCategory::Plugin,
        schema: super::environment::PARSER_VERSION,
        source_schema: Some(
            "Claude Code 原生 Plugin 卸载；按已注册的 qualified plugin id 和 scope 定位",
        ),
        argv: &[
            "plugin",
            "uninstall",
            "<qualified-plugin-id>",
            "--scope",
            "user|project|local",
        ],
        scopes: &[
            AgentAssetScope::User,
            AgentAssetScope::Workspace,
            AgentAssetScope::Local,
        ],
        reload: "卸载 Plugin；其提供的 Skill、MCP、Hook 将随插件重新盘点",
    }));
    result
}

pub(super) fn unavailable_reason(
    category: AgentAssetCategory,
    _: AgentAssetActionKind,
) -> AgentAssetActionUnavailableReason {
    match category {
        AgentAssetCategory::Mcp => AgentAssetActionUnavailableReason::NativeInteractiveOnly,
        AgentAssetCategory::Skill => AgentAssetActionUnavailableReason::InvocationPolicyOnly,
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
    if request.asset.category != AgentAssetCategory::Plugin {
        return Err(unavailable_error(unavailable_reason(
            request.asset.category,
            request.action,
        )));
    }
    let enabled = request.desired_enabled()?;
    let id = safe_native_id(&request.asset.native_id)?;
    // The installed registry owns this identity. A manifest/display name is
    // insufficient when two marketplaces offer a plugin with the same name.
    if !id.split_once('@').is_some_and(|(name, marketplace)| {
        !name.is_empty() && !marketplace.is_empty() && !marketplace.contains('@')
    }) {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::ScopeAmbiguous,
        ));
    }
    let root = Path::new(&request.context.config_root);
    let (scope, path) = match request.asset.scope {
        AgentAssetScope::User => ("user", root.join("settings.json")),
        AgentAssetScope::Workspace => (
            "project",
            request
                .workspace
                .ok_or_else(schema_error)?
                .join(".claude/settings.json"),
        ),
        AgentAssetScope::Local => (
            "local",
            request
                .workspace
                .ok_or_else(schema_error)?
                .join(".claude/settings.local.json"),
        ),
        _ => {
            return Err(unavailable_error(
                AgentAssetActionUnavailableReason::UnsupportedScope,
            ))
        }
    };
    if request.workspace.is_some() && request.context.trust_context != AgentTrustState::Trusted {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::TrustRequired,
        ));
    }
    let source = request.source_at(&path)?;
    let mut files = request.context_files()?;
    if !files.iter().any(|file| file.source_id == source.id) {
        files.push(request.file(source)?);
    }
    for file in &files {
        if file.path().file_name().is_some_and(|name| {
            matches!(
                name.to_str(),
                Some("settings.json" | "settings.local.json" | "managed-settings.json")
            )
        }) {
            validate_settings(file.bytes())?;
        }
    }
    let command = ExactCliCommand::new(
        request.installation,
        vec![
            "plugin".into(),
            if enabled { "enable" } else { "disable" }.into(),
            id.into(),
            "--scope".into(),
            scope.into(),
        ],
        request.cwd(),
        vec![
            (OsString::from("HOME"), request.home.as_os_str().to_owned()),
            (
                OsString::from("CLAUDE_CONFIG_DIR"),
                root.as_os_str().to_owned(),
            ),
        ],
    )?;
    PreparedMutation::new(
        request,
        MutationExecution::ExactCli(Box::new(command)),
        files,
        vec![source.id.clone()],
        vec![request.boolean_change(&path, "指定范围的 Plugin 启用状态")],
        request.asset.relationships.affected_asset_ids.clone(),
        verify,
    )
}

fn prepare_removal(
    request: MutationPreparation<'_>,
) -> Result<PreparedMutation, AgentAssetMutationError> {
    if request.asset.category != AgentAssetCategory::Plugin {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::NoOfficialMechanism,
        ));
    }
    let id = safe_native_id(&request.asset.native_id)?;
    if !id.split_once('@').is_some_and(|(name, marketplace)| {
        !name.is_empty() && !marketplace.is_empty() && !marketplace.contains('@')
    }) {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::ScopeAmbiguous,
        ));
    }
    let root = Path::new(&request.context.config_root);
    let (scope, path) = match request.asset.scope {
        AgentAssetScope::User => ("user", root.join("settings.json")),
        AgentAssetScope::Workspace => (
            "project",
            request
                .workspace
                .ok_or_else(schema_error)?
                .join(".claude/settings.json"),
        ),
        AgentAssetScope::Local => (
            "local",
            request
                .workspace
                .ok_or_else(schema_error)?
                .join(".claude/settings.local.json"),
        ),
        _ => {
            return Err(unavailable_error(
                AgentAssetActionUnavailableReason::UnsupportedScope,
            ))
        }
    };
    let source = request.source_at(&path)?;
    let mut files = request.context_files()?;
    if !files.iter().any(|file| file.source_id == source.id) {
        files.push(request.file(source)?);
    }
    for file in &files {
        if file.path().file_name().is_some_and(|name| {
            matches!(
                name.to_str(),
                Some("settings.json" | "settings.local.json" | "managed-settings.json")
            )
        }) {
            validate_settings(file.bytes())?;
        }
    }
    let command = ExactCliCommand::new(
        request.installation,
        vec![
            "plugin".into(),
            "uninstall".into(),
            id.into(),
            "--scope".into(),
            scope.into(),
            "--json".into(),
        ],
        request.cwd(),
        vec![
            (OsString::from("HOME"), request.home.as_os_str().to_owned()),
            (
                OsString::from("CLAUDE_CONFIG_DIR"),
                root.as_os_str().to_owned(),
            ),
        ],
    )?;
    PreparedMutation::new(
        request,
        MutationExecution::ExactCli(Box::new(command)),
        files,
        vec![source.id.clone()],
        vec![AgentAssetPlanChange {
            label: "通过 Claude Code 原生 CLI 卸载 Plugin".into(),
            path: Some(path.to_string_lossy().into_owned()),
            before: Some(id.to_owned()),
            after: Some("Plugin 注册与本地包由 Claude Code 清理".into()),
        }],
        request.asset.relationships.affected_asset_ids.clone(),
        verify,
    )
}

fn validate_settings(bytes: Option<&[u8]>) -> Result<(), AgentAssetMutationError> {
    let Some(bytes) = bytes else {
        return Ok(());
    };
    let value = parse(bytes, ConfigDocumentFormat::Json).ok_or_else(schema_error)?;
    if let Some(entries) = value.get("enabledPlugins") {
        let entries = entries.as_object().ok_or_else(schema_error)?;
        if entries
            .values()
            .any(|entry| !matches!(entry, Value::Bool(_)))
        {
            return Err(schema_error());
        }
    }
    Ok(())
}

fn verify(request: MutationVerification<'_>) -> bool {
    if request.action == AgentAssetActionKind::Remove {
        return request.asset_removed();
    }
    request.parent_state_matches()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_preflight_rejects_ambiguous_or_malformed_json_without_exposing_values() {
        for bytes in [
            br#"{"enabledPlugins":{"fixture@local":true,"fixture@local":false}}"#.as_slice(),
            br#"{"enabledPlugins":{"fixture@local":"false"}}"#,
            br#"{"enabledPlugins":[]}"#,
            b"{",
        ] {
            assert!(validate_settings(Some(bytes)).is_err());
        }
        assert!(validate_settings(Some(
            br#"{"enabledPlugins":{"fixture@local":true},"unknown":{"token":"fixture"}}"#
        ))
        .is_ok());
    }
}
