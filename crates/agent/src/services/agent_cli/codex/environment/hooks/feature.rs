//! One complete, typed observation of the native `features.hooks` merge input.
use super::*;
use crate::services::agent_cli::contracts::AgentAssetSourceSpec;

pub(in crate::services::agent_cli::codex::environment) const POLICY_KEY: &str =
    "codex-hook-feature-policy";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookFeatureLayer {
    Absent,
    /// An empty/other-feature table preserves an existing Hook leaf, but
    /// replaces a lower non-table container with a valid absent Hook leaf.
    TableWithoutHook,
    Enabled(bool),
    InvalidHook,
    InvalidTable,
    /// A source read/parse failure is not an observed empty feature table.
    Unavailable,
}

pub(in crate::services::agent_cli::codex::environment) fn is_config_source(key: &str) -> bool {
    matches!(key, "config" | "system-config" | "workspace-config")
}

pub(in crate::services::agent_cli::codex::environment) fn participation(
    context: &AgentConfigurationContext,
    source: &AgentAssetSourceSpec,
) -> AgentAssetResolutionParticipation {
    if source.native_source_key == "workspace-config"
        && context.trust_context != AgentTrustState::Trusted
    {
        AgentAssetResolutionParticipation::Suppressed {
            reason: AgentAssetSuppressionReason::UntrustedWorkspace,
        }
    } else {
        AgentAssetResolutionParticipation::Participates
    }
}

fn decode(request: AgentAssetParseRequest<'_>, root: Option<&Value>) -> HookFeatureLayer {
    if matches!(request.snapshot, AgentAssetSnapshot::Missing { .. }) {
        return HookFeatureLayer::Absent;
    }
    let Some(root) = root.and_then(Value::as_object) else {
        return HookFeatureLayer::Unavailable;
    };
    let Some(features) = root.get("features") else {
        return HookFeatureLayer::Absent;
    };
    let Some(features) = features.as_object() else {
        return HookFeatureLayer::InvalidTable;
    };
    match features.get("hooks") {
        None => HookFeatureLayer::TableWithoutHook,
        Some(Value::Bool(enabled)) => HookFeatureLayer::Enabled(*enabled),
        Some(_) => HookFeatureLayer::InvalidHook,
    }
}

pub(super) fn parse(
    request: AgentAssetParseRequest<'_>,
    root: Option<&Value>,
    output: &mut dyn AgentParseOutput,
) -> bool {
    if !is_config_source(&request.source.native_source_key) {
        return true;
    }
    let layer = decode(request, root);
    let mut observation = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: POLICY_KEY,
            resolution_group_key: POLICY_KEY,
            native_id: POLICY_KEY,
            category: AgentAssetCategory::Hook,
            label: "Codex Hook 全局开关",
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: AgentTrustState::Unknown,
            role: AgentAssetDeclarationRole::PolicyOverlay,
            participation: participation(request.context, request.source),
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Hook {
                managed: matches!(
                    request.source.scope,
                    AgentAssetScope::System | AgentAssetScope::Managed
                ),
                enabled: AgentAssetDeclaredState::Unknown,
                rule_count: Some(0),
            },
            facts: BTreeMap::new(),
        },
    );
    observation.native_payload =
        AgentAssetNativePayload::CodexHook(CodexHookPayload::FeaturePolicy { layer });
    output.emit_declaration(observation).is_continue()
}
