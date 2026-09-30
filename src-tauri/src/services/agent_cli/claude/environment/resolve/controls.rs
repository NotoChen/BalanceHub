use super::index::ControlSource;
use super::*;

#[derive(Default)]
pub(super) struct InvalidEvidence<'a> {
    members: BTreeMap<&'a str, &'a ParsedAgentAsset>,
    authorities: BTreeSet<AgentAssetControlAuthority>,
}
impl<'a> InvalidEvidence<'a> {
    pub(super) fn entity(
        &mut self,
        authority: &'a ParsedAgentAsset,
        members: impl IntoIterator<Item = &'a ParsedAgentAsset>,
    ) {
        self.authorities
            .insert(AgentAssetControlAuthority::Declaration(
                authority.declaration_id.clone(),
            ));
        for asset in members {
            self.members.insert(&asset.declaration_id, asset);
        }
    }
    pub(super) fn field(&mut self, asset: &'a ParsedAgentAsset) {
        self.entity(asset, [asset]);
    }
    pub(super) fn terminal(self) -> Option<NativeTerminal> {
        if self.members.is_empty() {
            return None;
        }
        Some(NativeTerminal::controlled(AgentAssetControlAssessment {
            cause: AgentAssetTerminalCauseDraft::InvalidControl,
            members: self
                .members
                .values()
                .map(|asset| member(asset, AgentAssetEvidenceKind::InvalidControl))
                .collect(),
            authorities: self.authorities,
        }))
    }
}

pub(super) fn boolean<'a>(
    sources: &[ControlSource<'a>],
    field: BooleanField,
    invalid: &mut InvalidEvidence<'a>,
) -> Option<(&'a ParsedAgentAsset, bool)> {
    let mut values = Vec::new();
    for source in sources {
        if let Some((asset, value)) = source.booleans.get(&field) {
            if *value == BooleanValue::Invalid {
                invalid.field(asset);
            }
            if let Some(value) = value.effective() {
                values.push((*asset, value));
            }
        }
    }
    values.sort_by(|left, right| {
        left.0
            .logical_origin
            .precedence
            .cmp(&right.0.logical_origin.precedence)
            .then_with(|| left.0.source_key.cmp(&right.0.source_key))
    });
    let selected = values.last().copied()?;
    let peers = values
        .iter()
        .filter(|(asset, _)| {
            asset.logical_origin.precedence == selected.0.logical_origin.precedence
        })
        .collect::<Vec<_>>();
    if peers.iter().any(|(_, value)| *value != selected.1) {
        for (asset, _) in peers {
            invalid.field(asset);
        }
    }
    Some(selected)
}

pub(super) fn policy(
    members: &[&ParsedAgentAsset],
    authority: AgentAssetControlAuthority,
) -> NativeTerminal {
    NativeTerminal::controlled(AgentAssetControlAssessment {
        cause: AgentAssetTerminalCauseDraft::TypedPolicy,
        members: members
            .iter()
            .map(|asset| member(asset, AgentAssetEvidenceKind::Policy))
            .collect(),
        authorities: BTreeSet::from([authority]),
    })
}
pub(super) fn field_policy(asset: &ParsedAgentAsset) -> NativeTerminal {
    policy(
        &[asset],
        AgentAssetControlAuthority::Declaration(asset.declaration_id.clone()),
    )
}

pub(super) fn hook_terminal(
    index: &NativeIndex<'_>,
    anchor: &ParsedAgentAsset,
) -> NativeResult<Option<NativeTerminal>> {
    let sources = index.controls(anchor.category)?;
    let mut invalid = InvalidEvidence::default();
    for source in sources {
        if source.state.invalid() {
            invalid.field(source.root);
        }
    }
    let disabled =
        boolean(sources, BooleanField::DisableAllHooks, &mut invalid).filter(|(_, value)| *value);
    let managed = if anchor.category == AgentAssetCategory::Hook {
        boolean(sources, BooleanField::ManagedHooksOnly, &mut invalid).filter(|(_, value)| *value)
    } else {
        None
    };
    if let Some(terminal) = invalid.terminal() {
        return Ok(Some(terminal));
    }
    let is_managed = matches!(
        anchor.details,
        AgentAssetDetails::Hook { managed: true, .. }
    );
    Ok(disabled
        .or(managed.filter(|_| !is_managed))
        .map(|(asset, _)| field_policy(asset)))
}

pub(super) fn plugin_terminal(index: &NativeIndex<'_>) -> NativeResult<Option<NativeTerminal>> {
    let sources = index.controls(AgentAssetCategory::Plugin)?;
    let mut invalid = InvalidEvidence::default();
    for source in sources {
        if source.state.invalid() {
            invalid.field(source.root);
        }
        if let Some(plugins) = &source.plugins {
            if plugins.state == ListState::Invalid {
                invalid.entity(plugins.root, plugins.members());
            }
        }
    }
    Ok(invalid.terminal())
}

pub(super) fn plugin_declared(
    index: &NativeIndex<'_>,
    anchor: &ParsedAgentAsset,
    structural: bool,
    assessment: AgentAssetNativeAssessment,
) -> NativeResult<AgentAssetNativeAssessment> {
    let sources = index.controls(AgentAssetCategory::Plugin)?;
    if structural {
        return Ok(assessment);
    }
    let overlays = sources
        .iter()
        .filter_map(|source| source.plugins.as_ref())
        .flat_map(|plugins| &plugins.entries)
        .copied()
        .filter(|asset| {
            asset.native_id == anchor.native_id
                && asset.participation == AgentAssetResolutionParticipation::Participates
        })
        .collect::<Vec<_>>();
    Ok(with_overlays(anchor, assessment, &overlays))
}

pub(super) fn with_overlays(
    anchor: &ParsedAgentAsset,
    assessment: AgentAssetNativeAssessment,
    overlays: &[&ParsedAgentAsset],
) -> AgentAssetNativeAssessment {
    let Some(precedence) = overlays
        .iter()
        .map(|asset| asset.logical_origin.precedence)
        .max()
    else {
        return assessment;
    };
    let mut peers = overlays
        .iter()
        .copied()
        .filter(|asset| asset.logical_origin.precedence == precedence)
        .collect::<Vec<_>>();
    peers.sort_by(|left, right| left.declaration_id.cmp(&right.declaration_id));
    let overlay_state = |asset: &ParsedAgentAsset| match &asset.native_payload {
        AgentAssetNativePayload::ClaudeControl(ClaudeControlPayload::PluginEntry { enabled }) => {
            *enabled
        }
        AgentAssetNativePayload::ClaudeControl(ClaudeControlPayload::NameEntry {
            family: ListFamily::PersonalEnabled,
            name: Some(_),
            ..
        }) => Some(true),
        AgentAssetNativePayload::ClaudeControl(ClaudeControlPayload::NameEntry {
            family: ListFamily::PersonalDisabled,
            name: Some(_),
            ..
        }) => Some(false),
        _ => None,
    };
    let invalid = peers.iter().any(|asset| overlay_state(asset).is_none());
    let states = peers
        .iter()
        .filter_map(|asset| overlay_state(asset))
        .collect::<BTreeSet<_>>();
    let cause = if invalid {
        Some(AgentAssetDeclaredUnknownCauseDraft::InvalidTypedControl)
    } else if states.len() > 1 {
        Some(AgentAssetDeclaredUnknownCauseDraft::OverlayConflict)
    } else {
        None
    };
    let state = if cause.is_some() {
        AgentAssetDeclaredState::Unknown
    } else if states.first() == Some(&true) {
        AgentAssetDeclaredState::Enabled
    } else {
        AgentAssetDeclaredState::Disabled
    };
    let members = peers
        .iter()
        .map(|asset| {
            member(
                asset,
                if overlay_state(asset).is_none() {
                    AgentAssetEvidenceKind::InvalidStateControl {
                        scope: AgentAssetStateOverlayScopeDraft::ExactNativeId,
                    }
                } else {
                    AgentAssetEvidenceKind::Overlay {
                        scope: AgentAssetStateOverlayScopeDraft::ExactNativeId,
                    }
                },
            )
        })
        .collect();
    let declared = if let Some(cause) = cause {
        AgentAssetDeclaredStateProofDraft::Unknown {
            evidence: refs(&peers),
            cause,
        }
    } else {
        AgentAssetDeclaredStateProofDraft::Overlay {
            declaration_ids: ids(&peers),
            scope: AgentAssetStateOverlayScopeDraft::ExactNativeId,
            outcome: state,
        }
    };
    let mut details = assessment.intrinsic.details;
    set_declared(&mut details, state);
    AgentAssetNativeAssessment {
        declared_state: state,
        declared,
        declared_members: members,
        intrinsic: intrinsic_basis(details, anchor.trust_state),
        control: None,
    }
}
