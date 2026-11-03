use super::super::plugin::{registry_entries, ClaudePluginPayload};
use super::*;

pub(super) fn parse_registry(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    for entry in registry_entries(request.context, request.workspace_lexical, root, output) {
        let mut facts = BTreeMap::new();
        if let Some(version) = &entry.version {
            facts.insert("version".to_owned(), version.clone());
        }
        let mut declaration = parsed_asset(
            request,
            ParsedAssetInput {
                declaration_key: &format!("registry:{}", entry.key.occurrence),
                resolution_group_key: &entry.key.id,
                category: AgentAssetCategory::Plugin,
                native_id: &entry.key.id,
                label: &entry.key.id,
                logical_origin: entry.origin,
                declared_state: AgentAssetDeclaredState::Unknown,
                trust_state: AgentTrustState::Unknown,
                role: crate::models::AgentAssetDeclarationRole::Definition,
                participation: if matches!(
                    entry.origin.scope,
                    AgentAssetScope::Workspace | AgentAssetScope::Local
                ) && request.context.trust_context != AgentTrustState::Trusted
                {
                    crate::models::AgentAssetResolutionParticipation::Suppressed {
                        reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
                    }
                } else {
                    crate::models::AgentAssetResolutionParticipation::Participates
                },
                provided_by: None,
                action_owner: None,
                explicitly_affected: Vec::new(),
                details: AgentAssetDetails::Plugin {
                    install_state: crate::models::AgentAssetInstallState::Installed,
                    enabled: AgentAssetDeclaredState::Unknown,
                    trusted: AgentTrustState::Unknown,
                },
                facts,
            },
        );
        declaration.native_payload =
            AgentAssetNativePayload::ClaudePlugin(ClaudePluginPayload::Registry(entry));
        output.emit_declaration(declaration)?;
    }
    ControlFlow::Continue(())
}
