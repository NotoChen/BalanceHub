//! Configured Hook definitions from the same bounded inventory scan.
//!
//! Native adapters own schema decoding. This collector only consumes their
//! typed Definition details and source diagnostics; executable availability,
//! trust and runtime resolution never change the configured rule count.

use crate::models::{
    AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDetails, AgentAssetDiagnostic,
    AgentAssetLimitKind, AgentAssetPresence, AgentCliKind, AgentConfigurationContext,
    AgentEnvironmentInventory, AgentHookRuleCount,
};
use crate::services::agent_cli::contracts::{AgentAssetSourceSpec, ParsedAgentAsset};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct HookRuleCounts {
    /// Every adapter parses the entire Hook definition set of one source.
    /// Re-reading that physical configuration in another context must not
    /// multiply its count. Divergent observations make the count incomplete.
    sources: BTreeMap<(AgentCliKind, String), Option<u32>>,
    policy_source_ids: BTreeSet<String>,
}

impl HookRuleCounts {
    pub(super) fn observe(
        &mut self,
        context: &AgentConfigurationContext,
        source: &AgentAssetSourceSpec,
        declarations: &[ParsedAgentAsset],
        parse_complete: bool,
    ) {
        if !source.hook_definition_source {
            self.policy_source_ids
                .insert(super::identity::source_stable_id(context, &source.path));
            return;
        }
        let mut identities = BTreeSet::new();
        let count = parse_complete.then_some(0_u32).and_then(|initial| {
            declarations
                .iter()
                .filter(|declaration| {
                    declaration.category == AgentAssetCategory::Hook
                        && declaration.role == AgentAssetDeclarationRole::Definition
                })
                .try_fold(initial, |total, declaration| {
                    if declaration.presence != AgentAssetPresence::Present
                        || !identities.insert(&declaration.declaration_key)
                    {
                        return None;
                    }
                    match &declaration.details {
                        AgentAssetDetails::Hook { rule_count, .. } => {
                            total.checked_add((*rule_count)?)
                        }
                        _ => None,
                    }
                })
        });
        let path = source
            .verified_physical_path
            .as_deref()
            .unwrap_or(&source.path);
        self.sources
            .entry((context.agent_kind, super::identity::lexical_identity(path)))
            .and_modify(|previous| {
                if *previous != count {
                    *previous = None;
                }
            })
            .or_insert(count);
    }

    pub(super) fn finish(
        self,
        inventory: &AgentEnvironmentInventory,
        kinds: impl Iterator<Item = AgentCliKind>,
    ) -> Vec<AgentHookRuleCount> {
        kinds
            .map(|kind| {
                let contexts = inventory
                    .contexts
                    .iter()
                    .filter(|context| context.agent_kind == kind)
                    .map(|context| context.id.as_str())
                    .collect::<BTreeSet<_>>();
                let incomplete = contexts.is_empty()
                    || inventory.diagnostics.iter().any(|diagnostic| {
                        count_gap(diagnostic, kind, false)
                            || diagnostic_source_id(diagnostic).is_some_and(|id| {
                                inventory.sources.iter().any(|source| {
                                    source.id == id
                                        && !self.policy_source_ids.contains(&source.id)
                                        && contexts.contains(source.context_id.as_str())
                                        && count_gap(
                                            diagnostic,
                                            kind,
                                            source.categories.contains(&AgentAssetCategory::Hook),
                                        )
                                })
                            })
                    })
                    || inventory.sources.iter().any(|source| {
                        contexts.contains(source.context_id.as_str())
                            && !self.policy_source_ids.contains(&source.id)
                            && source.diagnostics.iter().any(|diagnostic| {
                                count_gap(
                                    diagnostic,
                                    kind,
                                    source.categories.contains(&AgentAssetCategory::Hook),
                                )
                            })
                    })
                    || inventory.declarations.iter().any(|declaration| {
                        contexts.contains(declaration.context_id.as_str())
                            && !self.policy_source_ids.contains(&declaration.source_id)
                            && declaration.diagnostics.iter().any(|diagnostic| {
                                count_gap(
                                    diagnostic,
                                    kind,
                                    declaration.native_kind == AgentAssetCategory::Hook,
                                ) || (declaration.native_kind == AgentAssetCategory::Hook
                                    && matches!(
                                        diagnostic,
                                        AgentAssetDiagnostic::InvalidProjection { .. }
                                            | AgentAssetDiagnostic::InvalidResolution { .. }
                                    ))
                            })
                    });
                let rule_count = (!incomplete)
                    .then(|| {
                        self.sources
                            .iter()
                            .filter(|((agent, _), _)| *agent == kind)
                            .try_fold(0_u32, |total, (_, count)| total.checked_add((*count)?))
                    })
                    .flatten();
                AgentHookRuleCount {
                    agent_kind: kind,
                    rule_count,
                }
            })
            .collect()
    }
}

fn count_gap(diagnostic: &AgentAssetDiagnostic, kind: AgentCliKind, hook_source: bool) -> bool {
    use AgentAssetDiagnostic as D;
    match diagnostic {
        // These limits truncate executable discovery, not the independently
        // discovered configuration sources. Keep their diagnostics without
        // erasing configured Hook counts for every Agent.
        D::Truncated {
            limit:
                AgentAssetLimitKind::CandidatePathsPerAgent
                | AgentAssetLimitKind::InstallationsPerAgent
                | AgentAssetLimitKind::CliOutput
                | AgentAssetLimitKind::CliConcurrency,
            ..
        } => false,
        D::BudgetExceeded { .. } | D::Truncated { .. } => true,
        D::DiscoveryIncomplete {
            agent_kind,
            category: AgentAssetCategory::Hook,
            ..
        } => *agent_kind == kind,
        D::InvalidNativeId {
            category: AgentAssetCategory::Hook,
        } => hook_source,
        // A settings file may contain malformed non-Hook fields. Native
        // parsers report relevant decoding gaps as typed Hook discovery
        // diagnostics; this collector never interprets field-path strings.
        D::ReadFailed { .. }
        | D::SymlinkRejected { .. }
        | D::SourceOutsideAllowedRoot { .. }
        | D::SourceTypeMismatch { .. } => hook_source,
        _ => false,
    }
}

fn diagnostic_source_id(diagnostic: &AgentAssetDiagnostic) -> Option<&str> {
    match diagnostic {
        AgentAssetDiagnostic::ReadFailed { source_id, .. }
        | AgentAssetDiagnostic::SymlinkRejected { source_id }
        | AgentAssetDiagnostic::SourceOutsideAllowedRoot { source_id }
        | AgentAssetDiagnostic::SourceTypeMismatch { source_id, .. } => Some(source_id),
        _ => None,
    }
}
