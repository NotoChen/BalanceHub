use super::super::mutation::test_support::{inventory, mechanism};
use super::*;

fn evaluate(inventory: &AgentEnvironmentInventory) -> AgentMechanismEvaluation<'_> {
    evaluate_mechanism(
        &inventory.mechanisms,
        &inventory.installations[0],
        &inventory.contexts[0],
        &inventory.assets[0],
        AgentAssetActionKind::Disable,
        AgentNativeTarget {
            platform: inventory.environment.host_platform,
            architecture: inventory.environment.host_architecture,
        },
    )
}

fn unavailable(inventory: &AgentEnvironmentInventory) -> AgentAssetActionUnavailableReason {
    match evaluate(inventory) {
        AgentMechanismEvaluation::Unavailable { reason, .. } => reason,
        AgentMechanismEvaluation::Matched(_) => panic!("unqualified mechanism was enabled"),
    }
}

#[test]
fn native_gate_requires_matching_context_and_schema() {
    use AgentAssetActionUnavailableReason::*;
    type GateRejectionCase = (
        fn(&mut AgentEnvironmentInventory),
        AgentAssetActionUnavailableReason,
    );
    assert!(matches!(
        evaluate(&inventory()),
        AgentMechanismEvaluation::Matched(_)
    ));
    let cases: &[GateRejectionCase] = &[
        (
            |state| state.assets[0].compatible_installation_ids.clear(),
            NoCompatibleInstallation,
        ),
        (
            |state| {
                state.installations[0].availability = AgentInstallationAvailability::Unavailable
            },
            InstallationUnavailable,
        ),
        (
            |state| state.installations[0].executable_revision = None,
            InstallationUnavailable,
        ),
        (
            |state| state.assets[0].scope = AgentAssetScope::Managed,
            UnsupportedScope,
        ),
        (
            |state| state.environment.host_platform = AgentHostPlatform::Linux,
            UnsupportedPlatform,
        ),
        (
            |state| state.contexts[0].parser_version = 2,
            UnsupportedSchema,
        ),
    ];
    for (mutate, expected) in cases {
        let mut state = inventory();
        mutate(&mut state);
        assert_eq!(unavailable(&state), *expected);
    }
}

#[test]
fn native_gate_and_public_actions_require_installed_assets() {
    for category in [AgentAssetCategory::Plugin, AgentAssetCategory::Extension] {
        for enabled in [
            AgentAssetDeclaredState::Enabled,
            AgentAssetDeclaredState::Disabled,
        ] {
            for (install_state, reason) in [
                (
                    AgentAssetInstallState::NotInstalled,
                    AgentAssetActionUnavailableReason::AssetNotInstalled,
                ),
                (
                    AgentAssetInstallState::Unknown,
                    AgentAssetActionUnavailableReason::AssetInstallationUnknown,
                ),
            ] {
                let mut state = inventory();
                state.assets[0].category = category;
                state.assets[0].details = if category == AgentAssetCategory::Plugin {
                    AgentAssetDetails::Plugin {
                        enabled,
                        install_state,
                        trusted: AgentTrustState::Trusted,
                    }
                } else {
                    AgentAssetDetails::Extension {
                        enabled,
                        install_state,
                        trusted: AgentTrustState::Trusted,
                    }
                };
                for mechanism in &mut state.mechanisms {
                    mechanism.category = category;
                }
                assert_eq!(unavailable(&state), reason);
                materialize_actions(&mut state, &[]);
                let actions = state.assets[0]
                    .actions
                    .iter()
                    .filter(|action| {
                        matches!(
                            action.action,
                            AgentAssetActionKind::Enable | AgentAssetActionKind::Disable
                        )
                    })
                    .collect::<Vec<_>>();
                assert_eq!(actions.len(), 1);
                assert_eq!(actions[0].action, AgentAssetActionKind::Disable);
                assert!(actions
                    .iter()
                    .all(|action| !action.available && action.reason == Some(reason)));
            }
        }
    }
}

#[test]
fn overlapping_mechanisms_are_rejected() {
    let mut state = inventory();
    let mut second = mechanism(AgentAssetActionKind::Disable);
    second.id.0 = "overlapping fixture mechanism".into();
    state.mechanisms.push(second);
    assert_eq!(
        unavailable(&state),
        AgentAssetActionUnavailableReason::AmbiguousMechanism
    );
}

#[test]
fn native_actions_preserve_projector_policy_and_source_blockers() {
    for reason in [
        AgentAssetActionUnavailableReason::PolicyBlocked,
        AgentAssetActionUnavailableReason::SourceUnavailable,
        AgentAssetActionUnavailableReason::ChildOwnedByParent,
        AgentAssetActionUnavailableReason::TrustRequired,
        AgentAssetActionUnavailableReason::Shadowed,
        AgentAssetActionUnavailableReason::ScopeAmbiguous,
    ] {
        let mut state = inventory();
        let action = state.assets[0]
            .actions
            .iter_mut()
            .find(|action| action.action == AgentAssetActionKind::Disable)
            .unwrap();
        action.available = false;
        action.reason = Some(reason);
        materialize_actions(&mut state, &[]);
        let action = state.assets[0]
            .actions
            .iter()
            .find(|action| action.action == AgentAssetActionKind::Disable)
            .unwrap();
        assert!(!action.available);
        assert_eq!(action.reason, Some(reason));
    }
}

#[test]
fn fallback_uses_injected_adapter_and_not_a_global_agent_definition() {
    use super::super::mutation::{MutationPreparation, PreparedMutation};
    fn no_records(_: AgentCliKind) -> Vec<AgentAssetMechanismRecord> {
        vec![]
    }
    fn no_prepare(_: MutationPreparation<'_>) -> Result<PreparedMutation, AgentAssetMutationError> {
        Err(AgentAssetMutationError::new(
            AgentAssetMutationErrorKind::PreparationFailed,
        ))
    }
    fn native_reason(
        _: AgentAssetCategory,
        _: AgentAssetActionKind,
    ) -> AgentAssetActionUnavailableReason {
        AgentAssetActionUnavailableReason::InvocationPolicyOnly
    }
    let mut registered = crate::services::agent_cli::codex::definition(AgentCliKind::Codex);
    registered.environment =
        registered
            .environment
            .with_asset_mutation(no_records, no_prepare, native_reason);
    let mut state = inventory();
    state.mechanisms.clear();
    materialize_actions(&mut state, &[registered]);
    assert!(state.assets[0]
        .actions
        .iter()
        .all(|action| !action.available
            && action.reason == Some(AgentAssetActionUnavailableReason::InvocationPolicyOnly)));
}
