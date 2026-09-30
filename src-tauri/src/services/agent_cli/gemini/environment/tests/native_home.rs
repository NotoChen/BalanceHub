use super::*;

fn parse_user_rule(
    context: &AgentConfigurationContext,
    home: Option<&Path>,
    canonical: Option<&Path>,
    lexical: Option<&Path>,
    rules: &[String],
) -> ParsedAgentAsset {
    let source = test_source("extension-enablement", AgentAssetSourceKind::File);
    let snapshot = AgentAssetSnapshot::File {
        bytes: serde_json::to_vec(&serde_json::json!({"alpha": {"overrides": rules}})).unwrap(),
        revision: Default::default(),
    };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            context,
            source: &source,
            snapshot: &snapshot,
            native_home: home,
            workspace_canonical: canonical,
            workspace_lexical: lexical,
        },
        &mut output,
    );
    assert_eq!(output.declarations.len(), 1);
    output.declarations.remove(0)
}

#[test]
fn extension_user_state_uses_explicit_home_independent_of_config_root() {
    let context = test_context();
    let home = std::env::temp_dir().join("balancehub-native-user-fixture");
    assert_ne!(
        Path::new(&context.config_root).parent(),
        Some(home.as_path())
    );
    let enabled = format!("{}/*", home.display());
    let disabled = format!("!{enabled}");
    for (rules, expected) in [
        (vec![disabled.clone()], AgentAssetDeclaredState::Disabled),
        (vec![enabled.clone()], AgentAssetDeclaredState::Enabled),
        (
            vec![disabled.clone(), enabled.clone()],
            AgentAssetDeclaredState::Enabled,
        ),
        (
            vec![enabled, disabled.clone()],
            AgentAssetDeclaredState::Disabled,
        ),
        (vec!["!*".into()], AgentAssetDeclaredState::Disabled),
        (vec!["*".into()], AgentAssetDeclaredState::Enabled),
        (
            vec!["!/unrelated-location/*".into()],
            AgentAssetDeclaredState::Enabled,
        ),
    ] {
        let declaration = parse_user_rule(&context, Some(&home), None, None, &rules);
        assert_eq!(declaration.declared_state, expected, "{rules:?}");
        assert_eq!(declaration.role, AgentAssetDeclarationRole::StateOverlay);
        assert!(matches!(
            declaration.native_payload,
            AgentAssetNativePayload::None
        ));
    }
    let invalid = parse_user_rule(
        &context,
        Some(&home),
        None,
        None,
        &[disabled.clone(), String::new()],
    );
    assert!(matches!(
        invalid.native_payload,
        AgentAssetNativePayload::GeminiExtensionEnablementUnknown(
            GeminiExtensionEnablementUnknownCause::InvalidEntry
        )
    ));
    for missing_home in [None, Some(Path::new("relative-home"))] {
        let missing = parse_user_rule(
            &context,
            missing_home,
            None,
            None,
            std::slice::from_ref(&disabled),
        );
        assert_eq!(missing.declared_state, AgentAssetDeclaredState::Unknown);
        assert!(matches!(
            missing.native_payload,
            AgentAssetNativePayload::GeminiExtensionEnablementUnknown(
                GeminiExtensionEnablementUnknownCause::WorkspaceUnavailable
            )
        ));
    }
    assert!(context.workspace_id.is_none());
    assert_eq!(context.trust_context, AgentTrustState::Unknown);
}

#[test]
fn extension_user_home_does_not_replace_a_selected_workspace() {
    let context = AgentConfigurationContext {
        workspace_id: Some("selected-workspace".into()),
        ..test_context()
    };
    let home = std::env::temp_dir().join("balancehub-native-user-fixture");
    let home_disabled = format!("!{}/*", home.display());
    let workspace = Path::new("/other-workspace/project");
    let alias = Path::new("/workspace-alias/project");
    let row = parse_user_rule(
        &context,
        Some(&home),
        Some(workspace),
        Some(alias),
        std::slice::from_ref(&home_disabled),
    );
    assert_eq!(row.declared_state, AgentAssetDeclaredState::Enabled);
    let row = parse_user_rule(
        &context,
        Some(&home),
        Some(workspace),
        Some(alias),
        &[home_disabled.clone(), "!/workspace-alias/*".into()],
    );
    assert_eq!(row.declared_state, AgentAssetDeclaredState::Disabled);
    let missing_forms = parse_user_rule(&context, Some(&home), None, None, &[home_disabled]);
    assert_eq!(
        missing_forms.declared_state,
        AgentAssetDeclaredState::Unknown
    );
    assert!(matches!(
        missing_forms.native_payload,
        AgentAssetNativePayload::GeminiExtensionEnablementUnknown(
            GeminiExtensionEnablementUnknownCause::WorkspaceUnavailable
        )
    ));
}
