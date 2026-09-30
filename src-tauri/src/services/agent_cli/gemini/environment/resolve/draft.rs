//! Completed native decisions are the only input to the shared finalizer.
use super::*;

#[derive(Clone)]
pub(super) struct NativeDecision<'a> {
    pub(super) anchor: &'a ParsedAgentAsset,
    pub(super) key: String,
    pub(super) represented: Vec<&'a ParsedAgentAsset>,
    pub(super) contributors: Vec<&'a ParsedAgentAsset>,
    pub(super) resolution: AgentAssetResolutionDraft,
    pub(super) assessment: AgentAssetNativeAssessment,
    pub(super) effective: AgentAssetEffectiveStateProofDraft,
    pub(super) relationships: AgentAssetResolvedRelationships,
    pub(super) parent_state: Option<AgentAssetState>,
}

impl NativeDecision<'_> {
    pub(super) fn into_assessment(self) -> AgentAssetAssessmentResult {
        AgentAssetAssessmentResult::Assessed {
            state: self.assessment,
            projection: Box::new(AgentAssetProjectionAssessment {
                anchor_declaration_id: self.anchor.declaration_id.clone(),
                relation: self.resolution.relation,
                effective: self.effective,
                relationships: self.relationships,
            }),
        }
    }

    pub(super) fn finalize(self) -> Result<AgentAssetProjectedDraft, AgentAssetDiagnostic> {
        let invalid = || AgentAssetDiagnostic::InvalidResolution {
            projection_key: self.key.clone(),
            resolution: self.resolution.relation,
        };
        let (effective_state, details) = compose_effective_state(
            &self.assessment.intrinsic,
            &self.effective,
            self.parent_state,
        )
        .ok_or_else(invalid)?;
        finalize_projected_draft(AgentAssetResolvedDraftInput {
            anchor: self.anchor,
            projection_key: self.key.clone(),
            represented: &self.represented,
            contributors: &self.contributors,
            declared_state: self.assessment.declared_state,
            effective_state,
            trust_state: self.assessment.intrinsic.trust_state,
            resolution: self.resolution.clone(),
            details,
            relationships: self.relationships,
            state_proof: AgentAssetStateProofDraft {
                declared: self.assessment.declared,
                effective: self.effective,
            },
        })
        .map_err(|_| invalid())
    }

    pub(super) fn state(&self) -> NativeResult<AgentAssetState> {
        compose_effective_state(
            &self.assessment.intrinsic,
            &self.effective,
            self.parent_state,
        )
        .map(|(state, _)| state)
        .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)
    }

    pub(super) fn terminal(&mut self, terminal: NativeTerminal) {
        self.resolution.terminal = Some(terminal.kind);
        self.resolution.control_source = terminal.owner();
        self.effective = terminal.proof(AgentAssetEffectiveStateProofDraft::Intrinsic);
        self.assessment.control = terminal.control;
    }
}

pub(super) fn member(
    asset: &ParsedAgentAsset,
    kind: AgentAssetEvidenceKind,
) -> AgentAssetEvidenceMember {
    AgentAssetEvidenceMember {
        declaration_id: asset.declaration_id.clone(),
        expected_role: asset.role,
        kind,
    }
}

pub(super) fn ids(assets: &[&ParsedAgentAsset]) -> Vec<String> {
    let mut result = assets
        .iter()
        .map(|asset| asset.declaration_id.clone())
        .collect::<Vec<_>>();
    result.sort();
    result
}

pub(super) fn refs(assets: &[&ParsedAgentAsset]) -> Vec<AgentAssetStateEvidenceRefDraft> {
    ids(assets)
        .into_iter()
        .map(|declaration_id| AgentAssetStateEvidenceRefDraft::Declaration { declaration_id })
        .collect()
}

pub(super) fn set_declared(
    details: &AgentAssetDetails,
    state: AgentAssetDeclaredState,
) -> AgentAssetDetails {
    let mut details = details.clone();
    match &mut details {
        AgentAssetDetails::Mcp { declared_state, .. } => *declared_state = state,
        AgentAssetDetails::Skill { enabled, .. }
        | AgentAssetDetails::Plugin { enabled, .. }
        | AgentAssetDetails::Extension { enabled, .. }
        | AgentAssetDetails::Hook { enabled, .. } => *enabled = state,
        AgentAssetDetails::StatusUi {
            mode,
            command_present,
        } if state == AgentAssetDeclaredState::Unknown => {
            *mode = AgentStatusUiMode::Unknown;
            *command_present = false;
        }
        AgentAssetDetails::StatusUi { .. } => {}
    }
    details
}

pub(super) fn unknown_assessment(
    anchor: &ParsedAgentAsset,
    evidence: &[&ParsedAgentAsset],
    cause: AgentAssetDeclaredUnknownCauseDraft,
    members: Vec<AgentAssetEvidenceMember>,
) -> AgentAssetNativeAssessment {
    AgentAssetNativeAssessment {
        declared_state: AgentAssetDeclaredState::Unknown,
        declared: AgentAssetDeclaredStateProofDraft::Unknown {
            evidence: refs(evidence),
            cause,
        },
        declared_members: members,
        intrinsic: intrinsic_basis(
            set_declared(&anchor.details, AgentAssetDeclaredState::Unknown),
            anchor.trust_state,
        ),
        control: None,
    }
}

pub(super) fn definition_assessment(anchor: &ParsedAgentAsset) -> AgentAssetNativeAssessment {
    let members = vec![member(anchor, AgentAssetEvidenceKind::Definition)];
    if anchor.declared_state == AgentAssetDeclaredState::Unknown {
        return unknown_assessment(
            anchor,
            &[anchor],
            AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
            members,
        );
    }
    AgentAssetNativeAssessment {
        declared_state: anchor.declared_state,
        declared: AgentAssetDeclaredStateProofDraft::Definition {
            declaration_ids: vec![anchor.declaration_id.clone()],
            selected_id: anchor.declaration_id.clone(),
        },
        declared_members: members,
        intrinsic: intrinsic_basis(anchor.details.clone(), anchor.trust_state),
        control: None,
    }
}

pub(super) fn native_default(anchor: &ParsedAgentAsset) -> AgentAssetNativeAssessment {
    AgentAssetNativeAssessment {
        declared_state: AgentAssetDeclaredState::Enabled,
        declared: AgentAssetDeclaredStateProofDraft::NativeDefault {
            definition_id: anchor.declaration_id.clone(),
            outcome: AgentAssetDeclaredState::Enabled,
        },
        declared_members: vec![member(anchor, AgentAssetEvidenceKind::Definition)],
        intrinsic: intrinsic_basis(
            set_declared(&anchor.details, AgentAssetDeclaredState::Enabled),
            anchor.trust_state,
        ),
        control: None,
    }
}

pub(super) fn disabled_policy_assessment(
    anchor: &ParsedAgentAsset,
    policies: &[&ParsedAgentAsset],
) -> AgentAssetNativeAssessment {
    AgentAssetNativeAssessment {
        declared_state: AgentAssetDeclaredState::Disabled,
        declared: AgentAssetDeclaredStateProofDraft::Policy {
            declaration_ids: ids(policies),
            outcome: AgentAssetDeclaredState::Disabled,
        },
        declared_members: policies
            .iter()
            .map(|asset| member(asset, AgentAssetEvidenceKind::Policy))
            .collect(),
        intrinsic: intrinsic_basis(
            set_declared(&anchor.details, AgentAssetDeclaredState::Disabled),
            anchor.trust_state,
        ),
        control: None,
    }
}

pub(super) fn overlay_assessment(
    anchor: &ParsedAgentAsset,
    overlays: &[&ParsedAgentAsset],
) -> Option<AgentAssetNativeAssessment> {
    let first = *overlays.first()?;
    let peers = overlays
        .iter()
        .copied()
        .take_while(|asset| asset.logical_origin.precedence == first.logical_origin.precedence)
        .collect::<Vec<_>>();
    let invalid = |asset: &ParsedAgentAsset| {
        matches!(
            asset.native_payload,
            AgentAssetNativePayload::GeminiMcpEnablementInvalid
                | AgentAssetNativePayload::GeminiExtensionEnablementUnknown(
                    GeminiExtensionEnablementUnknownCause::InvalidEntry
                )
        )
    };
    let scope = AgentAssetStateOverlayScopeDraft::ResolutionGroup;
    let members = peers
        .iter()
        .map(|asset| {
            member(
                asset,
                if invalid(asset) {
                    AgentAssetEvidenceKind::InvalidStateControl { scope }
                } else {
                    AgentAssetEvidenceKind::Overlay { scope }
                },
            )
        })
        .collect();
    let cause = if peers.iter().any(|asset| invalid(asset)) {
        Some(AgentAssetDeclaredUnknownCauseDraft::InvalidTypedControl)
    } else if peers.iter().any(|asset| {
        matches!(
            asset.native_payload,
            AgentAssetNativePayload::GeminiExtensionEnablementUnknown(
                GeminiExtensionEnablementUnknownCause::WorkspaceUnavailable
            )
        )
    }) {
        Some(AgentAssetDeclaredUnknownCauseDraft::ContextUnavailable)
    } else if peers
        .iter()
        .any(|asset| asset.declared_state != first.declared_state)
    {
        Some(AgentAssetDeclaredUnknownCauseDraft::OverlayConflict)
    } else {
        None
    };
    Some(if let Some(cause) = cause {
        unknown_assessment(anchor, &peers, cause, members)
    } else {
        AgentAssetNativeAssessment {
            declared_state: first.declared_state,
            declared: AgentAssetDeclaredStateProofDraft::Overlay {
                declaration_ids: ids(&peers),
                scope,
                outcome: first.declared_state,
            },
            declared_members: members,
            intrinsic: intrinsic_basis(
                set_declared(&anchor.details, first.declared_state),
                anchor.trust_state,
            ),
            control: None,
        }
    })
}

pub(super) struct NativeTerminal {
    pub(super) kind: AgentAssetResolutionTerminal,
    pub(super) cause: AgentAssetTerminalCauseDraft,
    pub(super) control: Option<AgentAssetControlAssessment>,
}

impl NativeTerminal {
    pub(super) fn unknown(cause: AgentAssetTerminalCauseDraft) -> Self {
        Self {
            kind: AgentAssetResolutionTerminal::Unknown,
            cause,
            control: None,
        }
    }

    pub(super) fn declarations(
        cause: AgentAssetTerminalCauseDraft,
        assets: &[&ParsedAgentAsset],
    ) -> Self {
        Self {
            kind: if cause == AgentAssetTerminalCauseDraft::TypedPolicy {
                AgentAssetResolutionTerminal::PolicyBlocked
            } else {
                AgentAssetResolutionTerminal::Unknown
            },
            cause,
            control: Some(AgentAssetControlAssessment {
                cause,
                members: assets
                    .iter()
                    .map(|asset| {
                        member(
                            asset,
                            if cause == AgentAssetTerminalCauseDraft::TypedPolicy {
                                AgentAssetEvidenceKind::Policy
                            } else {
                                AgentAssetEvidenceKind::InvalidControl
                            },
                        )
                    })
                    .collect(),
                authorities: assets
                    .iter()
                    .map(|asset| {
                        AgentAssetControlAuthority::Declaration(asset.declaration_id.clone())
                    })
                    .collect(),
            }),
        }
    }

    fn owner(&self) -> Option<AgentAssetPolicyReferenceDraft> {
        let authorities = &self.control.as_ref()?.authorities;
        if authorities.len() != 1 {
            return None;
        }
        let AgentAssetControlAuthority::Declaration(declaration_id) = authorities.first()? else {
            return None;
        };
        Some(AgentAssetPolicyReferenceDraft::Declaration {
            declaration_id: declaration_id.clone(),
        })
    }

    pub(super) fn proof(
        &self,
        input: AgentAssetEffectiveStateProofDraft,
    ) -> AgentAssetEffectiveStateProofDraft {
        let mut evidence = self
            .control
            .as_ref()
            .map(|control| {
                control
                    .members
                    .iter()
                    .map(|member| AgentAssetStateEvidenceRefDraft::Declaration {
                        declaration_id: member.declaration_id.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        evidence.sort();
        AgentAssetEffectiveStateProofDraft::Terminal {
            terminal: self.kind,
            cause: self.cause,
            evidence,
            input: Box::new(input),
        }
    }
}
