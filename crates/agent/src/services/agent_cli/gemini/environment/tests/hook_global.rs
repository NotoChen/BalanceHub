use super::*;
use serde_json::{json, Value};

fn global_source(
    key: &str,
    scope: AgentAssetScope,
    precedence: u32,
    enabled: Option<Value>,
) -> (AgentAssetSourceSpec, AgentAssetSnapshot) {
    let mut source = d2_source(
        key,
        vec![AgentAssetCategory::Hook],
        precedence,
        &format!("/tmp/gemini/global-fixture/{key}.json"),
    );
    source.scope = scope;
    source.allowed_logical_origins[0].scope = scope;
    let mut document = json!({});
    if let Some(enabled) = enabled {
        document["hooksConfig"] = json!({"enabled": enabled});
    }
    if key == "settings" {
        document["hooks"] = json!({
            "SessionStart": [{"hooks": [{"type": "command", "command": "fixture-hook"}]}]
        });
    }
    (
        source,
        AgentAssetSnapshot::File {
            bytes: serde_json::to_vec(&document).unwrap(),
            revision: Default::default(),
        },
    )
}

#[test]
fn native_global_hook_switch_uses_highest_present_settings_value() {
    for (defaults, user, workspace, managed, expected, owner) in [
        (None, None, None, None, AgentAssetState::Enabled, None),
        (
            Some(json!(false)),
            Some(json!(true)),
            None,
            None,
            AgentAssetState::Enabled,
            None,
        ),
        (
            None,
            Some(json!(false)),
            Some(json!(true)),
            None,
            AgentAssetState::Enabled,
            None,
        ),
        (
            None,
            Some(json!(true)),
            Some(json!(false)),
            None,
            AgentAssetState::Blocked,
            Some("workspace-settings"),
        ),
        (
            None,
            Some(json!(false)),
            Some(json!(true)),
            Some(json!(false)),
            AgentAssetState::Blocked,
            Some("system-settings"),
        ),
        (
            None,
            Some(json!(false)),
            None,
            Some(json!("invalid")),
            AgentAssetState::Unknown,
            Some("system-settings"),
        ),
        (
            None,
            Some(json!("invalid")),
            None,
            Some(json!(true)),
            AgentAssetState::Enabled,
            None,
        ),
    ] {
        let trace = run_d2_sources(
            test_context(),
            vec![
                global_source("system-defaults", AgentAssetScope::System, 5, defaults),
                global_source("settings", AgentAssetScope::User, 10, user),
                global_source(
                    "workspace-settings",
                    AgentAssetScope::Workspace,
                    20,
                    workspace,
                ),
                global_source("system-settings", AgentAssetScope::Managed, 30, managed),
            ],
        );
        assert_no_projection_diagnostics(&trace);
        let row = record(&trace, AgentAssetCategory::Hook, "SessionStart:0:0");
        assert_eq!(row.declared_state, AgentAssetState::Enabled);
        assert_eq!(row.effective_state, expected);
        if let Some(owner) = owner {
            let Some(AgentAssetPolicyReference::Declaration { declaration_id }) =
                row.resolution.control_source.as_ref()
            else {
                panic!("global Hook control must identify its source declaration");
            };
            assert!(trace.declarations.iter().any(|declaration| {
                declaration.declaration_id == *declaration_id
                    && declaration.source_key == owner
                    && declaration.native_id == "hooksConfig.enabled"
            }));
        } else {
            assert!(row.resolution.control_source.is_none());
        }
    }
}

#[test]
fn untrusted_workspace_global_hook_policy_does_not_override_user_settings() {
    let mut context = test_context();
    context.workspace_id = Some("/tmp/gemini/global-workspace".to_owned());
    context.trust_context = AgentTrustState::Untrusted;
    let trace = run_d2_sources(
        context,
        vec![
            global_source("settings", AgentAssetScope::User, 10, Some(json!(true))),
            global_source(
                "workspace-settings",
                AgentAssetScope::Workspace,
                20,
                Some(json!(false)),
            ),
        ],
    );
    assert_no_projection_diagnostics(&trace);
    let row = record(&trace, AgentAssetCategory::Hook, "SessionStart:0:0");
    assert_eq!(row.declared_state, AgentAssetState::Enabled);
    assert_eq!(row.trust_state, AgentTrustState::Untrusted);
    assert_eq!(row.effective_state, AgentAssetState::Unknown);
    assert!(matches!(
        row.details,
        AgentAssetDetails::Hook {
            enabled: AgentAssetDeclaredState::Enabled,
            ..
        }
    ));
    assert_eq!(row.resolution.terminal, None);
    assert!(row.resolution.control_source.is_none());
    let draft = trace
        .drafts
        .iter()
        .find(|draft| {
            draft.native_kind == AgentAssetCategory::Hook && draft.native_id == "SessionStart:0:0"
        })
        .unwrap();
    assert_eq!(
        draft.state_proof.effective,
        AgentAssetEffectiveStateProofDraft::Intrinsic
    );
    assert!(trace.declarations.iter().any(|declaration| {
        declaration.source_key == "workspace-settings"
            && declaration.native_id == "hooksConfig.enabled"
            && declaration.role == AgentAssetDeclarationRole::PolicyOverlay
            && declaration.participation
                == AgentAssetResolutionParticipation::Suppressed {
                    reason: AgentAssetSuppressionReason::UntrustedWorkspace,
                }
    }));
}
