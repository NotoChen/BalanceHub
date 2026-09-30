//! Typed state composition shared by native construction and common checking.
//! Native precedence, approval, trust and control applicability stay outside.

use crate::models::{
    AgentAssetActionKind, AgentAssetDeclaredState, AgentAssetDetails,
    AgentAssetEffectiveAvailability, AgentAssetInstallState, AgentAssetResolutionTerminal,
    AgentAssetState, AgentMcpApprovalState, AgentStatusUiMode, AgentTrustState,
};
use crate::services::agent_cli::contracts::{
    AgentAssetEffectiveStateProofDraft, AgentAssetIntrinsicBasis, AgentAssetTerminalCauseDraft,
};

use super::adapter_support::state_from_declared;

/// Every native and catalog surface offers only a meaningful toggle direction.
pub(in crate::services::agent_cli) fn toggle_action_is_relevant(
    state: AgentAssetState,
    action: AgentAssetActionKind,
) -> bool {
    match action {
        AgentAssetActionKind::Enable => state == AgentAssetState::Disabled,
        AgentAssetActionKind::Disable => state == AgentAssetState::Enabled,
        _ => true,
    }
}

pub(in crate::services::agent_cli) fn details_declared_state(
    details: &AgentAssetDetails,
) -> AgentAssetDeclaredState {
    match details {
        AgentAssetDetails::Skill { enabled, .. }
        | AgentAssetDetails::Plugin { enabled, .. }
        | AgentAssetDetails::Extension { enabled, .. }
        | AgentAssetDetails::Hook { enabled, .. } => *enabled,
        AgentAssetDetails::Mcp { declared_state, .. } => *declared_state,
        AgentAssetDetails::StatusUi { mode, .. } => match mode {
            AgentStatusUiMode::BuiltIn | AgentStatusUiMode::Command => {
                AgentAssetDeclaredState::Enabled
            }
            AgentStatusUiMode::Disabled => AgentAssetDeclaredState::Disabled,
            AgentStatusUiMode::Unknown => AgentAssetDeclaredState::Unknown,
        },
    }
}

pub(in crate::services::agent_cli) fn expected_mcp_availability(
    declared_state: AgentAssetDeclaredState,
    approval_state: AgentMcpApprovalState,
    trust_state: AgentTrustState,
) -> AgentAssetEffectiveAvailability {
    match declared_state {
        AgentAssetDeclaredState::Disabled => AgentAssetEffectiveAvailability::Disabled,
        AgentAssetDeclaredState::Rejected => AgentAssetEffectiveAvailability::PolicyBlocked,
        AgentAssetDeclaredState::Pending => AgentAssetEffectiveAvailability::ApprovalRequired,
        AgentAssetDeclaredState::Unknown => AgentAssetEffectiveAvailability::Unknown,
        AgentAssetDeclaredState::Enabled => match approval_state {
            AgentMcpApprovalState::Rejected => AgentAssetEffectiveAvailability::PolicyBlocked,
            AgentMcpApprovalState::Pending => AgentAssetEffectiveAvailability::ApprovalRequired,
            AgentMcpApprovalState::Unknown => AgentAssetEffectiveAvailability::Unknown,
            AgentMcpApprovalState::Approved | AgentMcpApprovalState::NotRequired => {
                if matches!(
                    trust_state,
                    AgentTrustState::Required | AgentTrustState::Untrusted
                ) {
                    AgentAssetEffectiveAvailability::TrustRequired
                } else {
                    AgentAssetEffectiveAvailability::Available
                }
            }
        },
    }
}

pub(in crate::services::agent_cli) fn state_from_availability(
    availability: AgentAssetEffectiveAvailability,
) -> AgentAssetState {
    match availability {
        AgentAssetEffectiveAvailability::Available => AgentAssetState::Enabled,
        AgentAssetEffectiveAvailability::Disabled => AgentAssetState::Disabled,
        AgentAssetEffectiveAvailability::PolicyBlocked => AgentAssetState::Blocked,
        AgentAssetEffectiveAvailability::Invalid => AgentAssetState::Invalid,
        AgentAssetEffectiveAvailability::ApprovalRequired
        | AgentAssetEffectiveAvailability::TrustRequired
        | AgentAssetEffectiveAvailability::Unknown => AgentAssetState::Unknown,
    }
}

pub(in crate::services::agent_cli) fn intrinsic_basis(
    mut details: AgentAssetDetails,
    trust_state: AgentTrustState,
) -> AgentAssetIntrinsicBasis {
    if let AgentAssetDetails::Mcp {
        declared_state,
        approval_state,
        effective_availability,
        ..
    } = &mut details
    {
        *effective_availability =
            expected_mcp_availability(*declared_state, *approval_state, trust_state);
    }
    AgentAssetIntrinsicBasis {
        details,
        trust_state,
    }
}

pub(super) fn intrinsic_basis_is_consistent(basis: &AgentAssetIntrinsicBasis) -> bool {
    match &basis.details {
        AgentAssetDetails::Mcp {
            declared_state,
            approval_state,
            effective_availability,
            ..
        } => {
            *effective_availability
                == expected_mcp_availability(*declared_state, *approval_state, basis.trust_state)
        }
        AgentAssetDetails::StatusUi {
            mode,
            command_present,
        } => *command_present == (*mode == AgentStatusUiMode::Command),
        _ => true,
    }
}

/// This is only typed composition. Callers separately prove identity, the
/// parent edge and each control cause before accepting the result.
pub(in crate::services::agent_cli) fn compose_effective_state(
    basis: &AgentAssetIntrinsicBasis,
    proof: &AgentAssetEffectiveStateProofDraft,
    parent_state: Option<AgentAssetState>,
) -> Option<(AgentAssetState, AgentAssetDetails)> {
    let mut details = basis.details.clone();
    let intrinsic_state = intrinsic_state(&details, basis.trust_state);
    let (state, availability) = match proof {
        AgentAssetEffectiveStateProofDraft::Intrinsic => (intrinsic_state, None),
        AgentAssetEffectiveStateProofDraft::ParentGate { input, .. }
            if matches!(
                input.as_ref(),
                AgentAssetEffectiveStateProofDraft::Intrinsic
            ) =>
        {
            match parent_state? {
                AgentAssetState::Enabled => (intrinsic_state, None),
                AgentAssetState::Disabled | AgentAssetState::NotInstalled => (
                    AgentAssetState::Disabled,
                    Some(AgentAssetEffectiveAvailability::Disabled),
                ),
                AgentAssetState::Unknown | AgentAssetState::Blocked => (
                    AgentAssetState::Unknown,
                    Some(AgentAssetEffectiveAvailability::Unknown),
                ),
                _ => return None,
            }
        }
        AgentAssetEffectiveStateProofDraft::Shadowed { input, .. }
            if matches!(
                input.as_ref(),
                AgentAssetEffectiveStateProofDraft::Intrinsic
            ) =>
        {
            (AgentAssetState::Shadowed, None)
        }
        AgentAssetEffectiveStateProofDraft::Terminal {
            terminal,
            cause,
            input,
            ..
        } => {
            match input.as_ref() {
                AgentAssetEffectiveStateProofDraft::Intrinsic => {}
                AgentAssetEffectiveStateProofDraft::ParentGate { input, .. }
                    if matches!(
                        input.as_ref(),
                        AgentAssetEffectiveStateProofDraft::Intrinsic
                    ) && matches!(
                        parent_state,
                        Some(
                            AgentAssetState::Enabled
                                | AgentAssetState::Disabled
                                | AgentAssetState::NotInstalled
                                | AgentAssetState::Unknown
                                | AgentAssetState::Blocked
                        )
                    ) => {}
                _ => return None,
            }
            match terminal {
                AgentAssetResolutionTerminal::PolicyBlocked => (
                    AgentAssetState::Blocked,
                    Some(AgentAssetEffectiveAvailability::PolicyBlocked),
                ),
                AgentAssetResolutionTerminal::Unknown => (
                    AgentAssetState::Unknown,
                    Some(if *cause == AgentAssetTerminalCauseDraft::TrustSuppressed {
                        AgentAssetEffectiveAvailability::TrustRequired
                    } else {
                        AgentAssetEffectiveAvailability::Unknown
                    }),
                ),
            }
        }
        _ => return None,
    };
    if let (
        AgentAssetDetails::Mcp {
            effective_availability,
            ..
        },
        Some(availability),
    ) = (&mut details, availability)
    {
        *effective_availability = availability;
    }
    Some((state, details))
}

pub(super) fn intrinsic_state(
    details: &AgentAssetDetails,
    trust_state: AgentTrustState,
) -> AgentAssetState {
    match details {
        AgentAssetDetails::Hook {
            enabled: AgentAssetDeclaredState::Enabled,
            ..
        } if matches!(
            trust_state,
            AgentTrustState::Required | AgentTrustState::Untrusted
        ) =>
        {
            AgentAssetState::Unknown
        }
        AgentAssetDetails::Mcp {
            effective_availability,
            ..
        } => state_from_availability(*effective_availability),
        AgentAssetDetails::Plugin {
            enabled: AgentAssetDeclaredState::Enabled,
            install_state,
            ..
        }
        | AgentAssetDetails::Extension {
            enabled: AgentAssetDeclaredState::Enabled,
            install_state,
            ..
        } => match install_state {
            AgentAssetInstallState::Installed => AgentAssetState::Enabled,
            AgentAssetInstallState::NotInstalled => AgentAssetState::NotInstalled,
            AgentAssetInstallState::Unknown => AgentAssetState::Unknown,
        },
        _ => state_from_declared(details_declared_state(details)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgentAssetCategory, AgentAssetNativeRef, AgentSkillInvocationPolicy};

    #[test]
    fn hook_trust_controls_effective_state_without_rewriting_native_enablement() {
        for trust_state in [
            AgentTrustState::Trusted,
            AgentTrustState::Unknown,
            AgentTrustState::Required,
            AgentTrustState::Untrusted,
        ] {
            for enabled in [
                AgentAssetDeclaredState::Enabled,
                AgentAssetDeclaredState::Disabled,
            ] {
                let details = AgentAssetDetails::Hook {
                    managed: false,
                    enabled,
                    rule_count: Some(1),
                };
                let expected = if enabled == AgentAssetDeclaredState::Disabled {
                    AgentAssetState::Disabled
                } else if matches!(
                    trust_state,
                    AgentTrustState::Required | AgentTrustState::Untrusted
                ) {
                    AgentAssetState::Unknown
                } else {
                    AgentAssetState::Enabled
                };
                assert_eq!(
                    compose_effective_state(
                        &intrinsic_basis(details.clone(), trust_state),
                        &AgentAssetEffectiveStateProofDraft::Intrinsic,
                        None,
                    ),
                    Some((expected, details)),
                );
            }
        }
    }

    #[test]
    fn unavailable_installation_never_claims_effective_enablement_or_rewrites_configuration() {
        for extension in [false, true] {
            for (install_state, expected) in [
                (AgentAssetInstallState::Installed, AgentAssetState::Enabled),
                (
                    AgentAssetInstallState::NotInstalled,
                    AgentAssetState::NotInstalled,
                ),
                (AgentAssetInstallState::Unknown, AgentAssetState::Unknown),
            ] {
                for enabled in [
                    AgentAssetDeclaredState::Enabled,
                    AgentAssetDeclaredState::Disabled,
                ] {
                    let details = if extension {
                        AgentAssetDetails::Extension {
                            install_state,
                            enabled,
                            trusted: AgentTrustState::Unknown,
                        }
                    } else {
                        AgentAssetDetails::Plugin {
                            install_state,
                            enabled,
                            trusted: AgentTrustState::Unknown,
                        }
                    };
                    let basis = intrinsic_basis(details.clone(), AgentTrustState::Unknown);
                    let expected = if enabled == AgentAssetDeclaredState::Disabled {
                        AgentAssetState::Disabled
                    } else {
                        expected
                    };
                    assert_eq!(
                        compose_effective_state(
                            &basis,
                            &AgentAssetEffectiveStateProofDraft::Intrinsic,
                            None
                        ),
                        Some((expected, details))
                    );
                }
            }
        }
    }

    #[test]
    fn a_missing_parent_disables_its_child_without_rewriting_the_child_switch() {
        let details = AgentAssetDetails::Skill {
            enabled: AgentAssetDeclaredState::Enabled,
            invocation_policy: AgentSkillInvocationPolicy::ModelInvocable,
        };
        let proof = AgentAssetEffectiveStateProofDraft::ParentGate {
            parent: AgentAssetNativeRef {
                category: AgentAssetCategory::Plugin,
                native_id: "parent".to_owned(),
                qualifier: None,
            },
            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
        };
        assert_eq!(
            compose_effective_state(
                &intrinsic_basis(details.clone(), AgentTrustState::Unknown),
                &proof,
                Some(AgentAssetState::NotInstalled)
            ),
            Some((AgentAssetState::Disabled, details))
        );
    }
}
