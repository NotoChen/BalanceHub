use super::super::policy::policy_rule_matches;
use super::controls::{boolean, field_policy, policy, with_overlays, InvalidEvidence};
use super::index::{ControlList, ControlSource};
use super::*;

pub(super) fn declared(
    index: &NativeIndex<'_>,
    anchor: &ParsedAgentAsset,
    structural: bool,
    assessment: AgentAssetNativeAssessment,
) -> NativeResult<AgentAssetNativeAssessment> {
    let sources = index.controls(AgentAssetCategory::Mcp)?;
    let AgentAssetDetails::Mcp { transport, .. } = anchor.details else {
        return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
    };
    let unresolved_plugin = matches!(&anchor.native_payload, AgentAssetNativePayload::ClaudePlugin(super::super::plugin::ClaudePluginPayload::Mcp { value, .. }) if super::super::plugin::runtime_expansion(value));
    let mut assessment =
        if !structural && transport != AgentMcpTransport::Unknown && !unresolved_plugin {
            let overlays = sources
                .iter()
                .filter(|source| source.spec.native_source_key == "account")
                .flat_map(|source| source.lists.values())
                .filter(|list| list.state == ListState::Valid)
                .flat_map(|list| &list.entries)
                .copied()
                .filter(|asset| {
                    asset.native_id == anchor.native_id
                        && asset.participation == AgentAssetResolutionParticipation::Participates
                })
                .collect::<Vec<_>>();
            with_overlays(anchor, assessment, &overlays)
        } else {
            assessment
        };
    let approval_state = if anchor.source_key == "workspace-mcp"
        || (matches!(
            anchor.native_payload,
            AgentAssetNativePayload::ClaudePlugin(_)
        ) && matches!(
            anchor.logical_origin.scope,
            AgentAssetScope::Workspace | AgentAssetScope::Local
        )) {
        approval(sources, &anchor.native_id)
    } else {
        AgentMcpApprovalState::NotRequired
    };
    let details = AgentAssetDetails::Mcp {
        transport,
        declared_state: assessment.declared_state,
        approval_state,
        effective_availability: crate::models::AgentAssetEffectiveAvailability::Unknown,
    };
    assessment.intrinsic = intrinsic_basis(details, anchor.trust_state);
    Ok(assessment)
}

fn approval(sources: &[ControlSource<'_>], native_id: &str) -> AgentMcpApprovalState {
    let mut approved = BTreeSet::new();
    let mut rejected = BTreeSet::new();
    let mut approve_all = Vec::new();
    let mut unknown = false;
    for source in sources
        .iter()
        .filter(|source| SETTINGS_AUTHORITIES.contains(&source.spec.native_source_key.as_str()))
    {
        unknown |= source.state.invalid();
        for (family, target) in [
            (ListFamily::Approved, &mut approved),
            (ListFamily::Rejected, &mut rejected),
        ] {
            if let Some(list) = source.lists.get(&family) {
                unknown |= list.state == ListState::Invalid;
                for entry in &list.entries {
                    if let AgentAssetNativePayload::ClaudeControl(
                        ClaudeControlPayload::NameEntry {
                            name: Some(name), ..
                        },
                    ) = &entry.native_payload
                    {
                        target.insert(name.as_str());
                    }
                }
            }
        }
        if let Some((asset, value)) = source.booleans.get(&BooleanField::ApproveAll) {
            unknown |= *value == BooleanValue::Invalid;
            if let BooleanValue::Bool(value) = value {
                approve_all.push((asset.logical_origin.precedence, *value));
            }
        }
    }
    if unknown {
        return AgentMcpApprovalState::Unknown;
    }
    if rejected.contains(native_id) {
        return AgentMcpApprovalState::Rejected;
    }
    if approved.contains(native_id) {
        return AgentMcpApprovalState::Approved;
    }
    if let Some(precedence) = approve_all.iter().map(|(precedence, _)| *precedence).max() {
        let states = approve_all
            .iter()
            .filter(|(candidate, _)| *candidate == precedence)
            .map(|(_, state)| *state)
            .collect::<BTreeSet<_>>();
        if states.len() > 1 {
            return AgentMcpApprovalState::Unknown;
        }
        if states.contains(&true) {
            return AgentMcpApprovalState::Approved;
        }
    }
    AgentMcpApprovalState::Pending
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum McpTerminalRank {
    SecurityUnknown,
    ManagedMcpExclusive,
    MatchingDeny,
    ManagedOnly,
    AllowedList,
}
struct McpTerminalDecision {
    rank: McpTerminalRank,
    terminal: NativeTerminal,
}
fn select_mcp_terminal_decision(
    candidates: impl IntoIterator<Item = McpTerminalDecision>,
) -> Option<NativeTerminal> {
    candidates
        .into_iter()
        .min_by_key(|candidate| candidate.rank)
        .map(|candidate| candidate.terminal)
}

enum PolicyEvaluation<'a> {
    Denied(&'a ParsedAgentAsset),
    Allowed,
    BlockedByAllowList(&'a ControlList<'a>),
    Invalid,
    NotApplicable,
}
fn evaluate_policy<'a>(
    source: &'a ControlSource<'a>,
    anchor: &ParsedAgentAsset,
    transport: AgentMcpTransport,
) -> PolicyEvaluation<'a> {
    if let Some(denied) = source.lists.get(&ListFamily::Denied) {
        if denied.state == ListState::Invalid {
            return PolicyEvaluation::Invalid;
        }
        if let Some(rule) = denied
            .entries
            .iter()
            .copied()
            .find(|entry| matches_rule(entry, anchor, transport))
        {
            return PolicyEvaluation::Denied(rule);
        }
    }
    let Some(allowed) = source.lists.get(&ListFamily::Allowed) else {
        return PolicyEvaluation::NotApplicable;
    };
    match allowed.state {
        ListState::Absent => PolicyEvaluation::NotApplicable,
        ListState::NativeFailClosed | ListState::Invalid => {
            PolicyEvaluation::BlockedByAllowList(allowed)
        }
        ListState::Valid => {
            if allowed
                .entries
                .iter()
                .any(|entry| matches_rule(entry, anchor, transport))
            {
                PolicyEvaluation::Allowed
            } else {
                PolicyEvaluation::BlockedByAllowList(allowed)
            }
        }
    }
}
fn matches_rule(
    entry: &ParsedAgentAsset,
    anchor: &ParsedAgentAsset,
    transport: AgentMcpTransport,
) -> bool {
    matches!(&entry.native_payload, AgentAssetNativePayload::ClaudeControl(ClaudeControlPayload::RuleEntry { matcher: Some(matcher), .. })
        if policy_rule_matches(matcher, &anchor.native_id, transport, &anchor.native_payload))
}
fn rule_order(entry: &ParsedAgentAsset) -> (&str, &str, usize) {
    match &entry.native_payload {
        AgentAssetNativePayload::ClaudeControl(ClaudeControlPayload::RuleEntry {
            family,
            ordinal,
            matcher: Some(matcher),
        }) => (family.field(), matcher.field(), *ordinal),
        _ => unreachable!("indexed matching rule"),
    }
}

pub(super) fn terminal(
    index: &NativeIndex<'_>,
    anchor: &ParsedAgentAsset,
    assessment: &AgentAssetNativeAssessment,
) -> NativeResult<Option<NativeTerminal>> {
    let sources = index.controls(AgentAssetCategory::Mcp)?;
    let mut invalid = InvalidEvidence::default();
    for source in sources {
        if source.spec.native_source_key != "account" && source.state.invalid() {
            invalid.field(source.root);
        }
        for (family, list) in &source.lists {
            if matches!(
                family,
                ListFamily::Denied | ListFamily::Approved | ListFamily::Rejected
            ) && list.state == ListState::Invalid
            {
                invalid.entity(list.root, list.members());
            }
        }
        if let Some((asset, NamespaceState::InvalidContainer)) = source.namespace {
            invalid.field(asset);
        }
    }
    let _ = boolean(sources, BooleanField::ApproveAll, &mut invalid);
    let managed_only = boolean(sources, BooleanField::ManagedMcpOnly, &mut invalid);
    let mut candidates = Vec::new();
    if let Some(terminal) = invalid.terminal() {
        candidates.push(McpTerminalDecision {
            rank: McpTerminalRank::SecurityUnknown,
            terminal,
        });
    } else if matches!(
        assessment.declared,
        AgentAssetDeclaredStateProofDraft::Unknown {
            cause: AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown
                | AgentAssetDeclaredUnknownCauseDraft::OverlayConflict
                | AgentAssetDeclaredUnknownCauseDraft::InvalidNativeMerge,
            ..
        }
    ) {
        candidates.push(McpTerminalDecision {
            rank: McpTerminalRank::SecurityUnknown,
            terminal: NativeTerminal::unknown(AgentAssetTerminalCauseDraft::DeclaredUnknown),
        });
    }
    for source in sources {
        if let Some((asset, NamespaceState::Object(names))) = source.namespace {
            if anchor.source_key != "managed-mcp" && !names.contains(&anchor.native_id) {
                candidates.push(McpTerminalDecision {
                    rank: McpTerminalRank::ManagedMcpExclusive,
                    terminal: policy(
                        &[asset],
                        AgentAssetControlAuthority::SourceAggregate(
                            source.spec.native_source_key.clone(),
                        ),
                    ),
                });
            }
        }
    }
    let transport = match anchor.details {
        AgentAssetDetails::Mcp { transport, .. } => transport,
        _ => AgentMcpTransport::Unknown,
    };
    let mut denied_matches = Vec::new();
    let mut allow_blocks = Vec::new();
    for source in sources
        .iter()
        .filter(|source| SETTINGS_AUTHORITIES.contains(&source.spec.native_source_key.as_str()))
    {
        match evaluate_policy(source, anchor, transport) {
            PolicyEvaluation::Denied(rule) => denied_matches.push((source.spec.precedence, rule)),
            PolicyEvaluation::BlockedByAllowList(list) => allow_blocks.push((
                source.spec.precedence,
                source.spec.native_source_key.as_str(),
                list,
            )),
            PolicyEvaluation::Invalid
            | PolicyEvaluation::Allowed
            | PolicyEvaluation::NotApplicable => {}
        }
    }
    if let Some((_, rule)) = denied_matches.into_iter().max_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.source_key.cmp(&right.1.source_key))
            .then_with(|| rule_order(left.1).cmp(&rule_order(right.1)))
    }) {
        candidates.push(McpTerminalDecision {
            rank: McpTerminalRank::MatchingDeny,
            terminal: field_policy(rule),
        });
    }
    if let Some((field, true)) = managed_only {
        if !matches!(
            anchor.source_key.as_str(),
            "managed-mcp" | "managed-settings"
        ) {
            candidates.push(McpTerminalDecision {
                rank: McpTerminalRank::ManagedOnly,
                terminal: field_policy(field),
            });
        }
    }
    if let Some((_, source_key, list)) = allow_blocks
        .into_iter()
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)))
    {
        candidates.push(McpTerminalDecision {
            rank: McpTerminalRank::AllowedList,
            terminal: policy(
                &list.members(),
                AgentAssetControlAuthority::SourceAggregate(source_key.to_owned()),
            ),
        });
    }
    Ok(select_mcp_terminal_decision(candidates))
}
