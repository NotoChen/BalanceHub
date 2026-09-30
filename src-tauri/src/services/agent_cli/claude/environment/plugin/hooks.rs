//! Plugin Hook definitions retain their exact parent and physical source.
//! The default hooks/hooks.json and manifest additions accumulate natively.
mod decoded;
pub(crate) use decoded::{HookDocument, HookRule};

use super::super::parse::hooks as schema;
use super::*;
use crate::models::*;
use crate::services::agent_cli::{
    contracts::{
        AgentAssetNativePayload, AgentAssetParseRequest, AgentOutputStop, AgentParseOutput,
    },
    environment::{parsed_asset, ParsedAssetInput},
};
use std::{collections::BTreeMap, ops::ControlFlow};

#[derive(Clone, PartialEq)]
pub(crate) enum HookComponent {
    File(PathBuf),
    Inline(HookDocument),
    Invalid,
}

pub(super) fn components(value: Option<&Value>) -> Vec<HookComponent> {
    value
        .into_iter()
        .flat_map(|value| {
            value
                .as_array()
                .map_or_else(|| vec![value], |values| values.iter().collect())
        })
        .map(|value| match value {
            Value::String(path) => relative_path(path)
                .map(HookComponent::File)
                .unwrap_or(HookComponent::Invalid),
            Value::Object(events) => HookComponent::Inline(decoded::inline_document(events)),
            _ => HookComponent::Invalid,
        })
        .collect()
}

pub(in crate::services::agent_cli::claude) fn native_id(
    parent: &str,
    ordinal: usize,
    event: &str,
    group: usize,
    handler: usize,
) -> String {
    format!("plugin:{parent}:hook:{ordinal}:{event}:group-{group}:hook-{handler}")
}

pub(in crate::services::agent_cli::claude::environment) fn source_id(
    parent: &str,
    ordinal: usize,
) -> String {
    format!("plugin:{parent}:hook-source:{ordinal}")
}

/// A malformed guard makes the native plugin loader refuse the plugin; valid
/// siblings from another Hook file must not acquire an enabled state from it.
pub(in crate::services::agent_cli::claude::environment) fn guard_safe(root: &Value) -> bool {
    let Some(root) = root.as_object() else {
        return false;
    };
    if root.contains_key("PreToolUse") || root.contains_key("PermissionRequest") {
        return false;
    }
    match root.get("hooks") {
        None => true,
        Some(Value::Object(events)) => !events
            .iter()
            .any(|(event, groups)| schema::guard_event_invalid(event, groups)),
        Some(_) => false,
    }
}

pub(super) fn emit(
    request: AgentAssetParseRequest<'_>,
    binding: &SourceBinding,
    namespace: &str,
    ordinal: usize,
    root: Option<&Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    emit_document(
        request,
        binding,
        namespace,
        ordinal,
        root.map(decoded::decode_document),
        output,
    )
}

pub(super) fn emit_document(
    request: AgentAssetParseRequest<'_>,
    binding: &SourceBinding,
    namespace: &str,
    ordinal: usize,
    document: Option<HookDocument>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let id = source_id(&binding.key.id, ordinal);
    let mut observation = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: &id,
            resolution_group_key: &id,
            category: AgentAssetCategory::Hook,
            native_id: &id,
            label: "Plugin Hook 来源",
            logical_origin: binding.origin(),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: AgentTrustState::Unknown,
            role: AgentAssetDeclarationRole::StateOverlay,
            participation: AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Hook {
                managed: binding.origin().scope == AgentAssetScope::Managed,
                enabled: AgentAssetDeclaredState::Unknown,
                rule_count: None,
            },
            facts: BTreeMap::new(),
        },
    );
    observation.native_payload =
        AgentAssetNativePayload::ClaudePlugin(ClaudePluginPayload::HookSource {
            key: binding.key.clone(),
            ordinal,
            document: document.clone(),
            missing: matches!(request.snapshot, AgentAssetSnapshot::Missing { .. }),
        });
    if matches!(request.snapshot, AgentAssetSnapshot::Missing { .. }) {
        observation.presence = AgentAssetPresence::Missing;
    }
    output.emit_declaration(observation)?;
    let Some(document) = document else {
        if !matches!(request.snapshot, AgentAssetSnapshot::Missing { .. }) {
            unobserved(output, "hooks.sourceUnreadable");
        }
        return ControlFlow::Continue(());
    };
    for diagnostic in &document.diagnostics {
        unobserved(output, diagnostic);
    }
    for definition in document.rules {
        let id = native_id(
            &binding.key.id,
            ordinal,
            &definition.event,
            definition.group_index,
            definition.handler_index,
        );
        let mut asset = parsed_asset(
            request,
            ParsedAssetInput {
                declaration_key: &id,
                resolution_group_key: &id,
                category: AgentAssetCategory::Hook,
                native_id: &id,
                label: &format!("Plugin Hook：{}", definition.event),
                logical_origin: binding.origin(),
                declared_state: AgentAssetDeclaredState::Enabled,
                trust_state: request.context.trust_context,
                role: AgentAssetDeclarationRole::Definition,
                participation: AgentAssetResolutionParticipation::Participates,
                provided_by: Some(parent_ref(&binding.key)),
                action_owner: Some(parent_ref(&binding.key)),
                explicitly_affected: Vec::new(),
                details: AgentAssetDetails::Hook {
                    managed: binding.origin().scope == AgentAssetScope::Managed,
                    enabled: AgentAssetDeclaredState::Enabled,
                    rule_count: Some(1),
                },
                facts: BTreeMap::new(),
            },
        );
        asset.native_payload = AgentAssetNativePayload::ClaudePlugin(ClaudePluginPayload::Hook {
            key: binding.key.clone(),
            namespace: namespace.to_owned(),
            ordinal,
            definition,
        });
        output.emit_declaration(asset)?;
    }
    ControlFlow::Continue(())
}
