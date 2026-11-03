//! Reuse live, unchanged adapter results; mutations never enter this cache.
use super::{CatalogService, PublishedCatalog};
use crate::{
    models::*,
    services::agent_cli::environment::{self, InventoryBuild},
};
use std::{collections::BTreeSet, path::Path};

impl CatalogService {
    pub(crate) fn refresh_inventory(
        &self,
        previous: Option<&PublishedCatalog>,
        settings: &AppSettings,
        workspace: Option<&Path>,
        canceled: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<InventoryBuild, String> {
        let Some(previous) = previous.filter(|previous| {
            // Persisted rows carry no live access evidence. A changed cold
            // snapshot needs a fresh inventory before global reconciliation.
            previous
                .snapshot
                .inventory
                .sources
                .iter()
                .all(|source| !source.allowed_root.is_empty())
                && !previous
                    .snapshot
                    .inventory
                    .diagnostics
                    .iter()
                    .any(|diagnostic| {
                        matches!(diagnostic, AgentAssetDiagnostic::BudgetExceeded { .. })
                    })
        }) else {
            return environment::display_inventory(settings, workspace, None, canceled);
        };
        let changed = previous.inputs.changed_agents(settings);
        if changed.is_empty() {
            return Ok(previous.inventory_build());
        }
        let updated =
            environment::display_inventory(settings, workspace, Some(&changed), canceled)?;
        Ok(merge(previous.inventory_build(), updated, &changed))
    }
}

fn merge(
    mut base: InventoryBuild,
    update: InventoryBuild,
    changed: &BTreeSet<AgentCliKind>,
) -> InventoryBuild {
    let replaced_sources = base
        .inventory
        .sources
        .iter()
        .filter(|source| {
            base.inventory.contexts.iter().any(|context| {
                context.id == source.context_id && changed.contains(&context.agent_kind)
            })
        })
        .map(|source| source.id.as_str())
        .collect::<BTreeSet<_>>();
    base.access_evidence
        .retain(|evidence| !replaced_sources.contains(evidence.source_id.as_str()));
    merge_inventory(&mut base.inventory, update.inventory, changed);
    base.access_evidence.extend(update.access_evidence);
    base
}

pub(super) fn merge_inventory(
    inventory: &mut AgentEnvironmentInventory,
    update: AgentEnvironmentInventory,
    changed: &BTreeSet<AgentCliKind>,
) {
    let removed_contexts = inventory
        .contexts
        .iter()
        .filter(|context| changed.contains(&context.agent_kind))
        .map(|context| context.id.clone())
        .collect::<BTreeSet<_>>();
    inventory
        .sources
        .retain(|source| !removed_contexts.contains(&source.context_id));
    inventory
        .declarations
        .retain(|declaration| !removed_contexts.contains(&declaration.context_id));
    inventory
        .contexts
        .retain(|context| !changed.contains(&context.agent_kind));
    inventory
        .installations
        .retain(|item| !changed.contains(&item.agent_kind));
    inventory
        .assets
        .retain(|item| !changed.contains(&item.agent_kind));
    inventory
        .capabilities
        .retain(|item| !changed.contains(&item.agent_kind));
    inventory
        .hook_rule_counts
        .retain(|item| !changed.contains(&item.agent_kind));
    inventory
        .mechanisms
        .retain(|item| !changed.contains(&item.agent_kind));
    inventory.sources.extend(update.sources);
    inventory.declarations.extend(update.declarations);
    inventory.contexts.extend(update.contexts);
    inventory.installations.extend(update.installations);
    inventory.assets.extend(update.assets);
    inventory.capabilities.extend(update.capabilities);
    inventory.hook_rule_counts.extend(update.hook_rule_counts);
    inventory.mechanisms.extend(update.mechanisms);
    inventory.diagnostics = update.diagnostics;
    inventory.scanned_at = update.scanned_at;
    inventory.limits = update.limits;
    inventory
        .sources
        .sort_by(|left, right| left.id.cmp(&right.id));
    inventory
        .contexts
        .sort_by(|left, right| left.id.cmp(&right.id));
    inventory
        .declarations
        .sort_by(|left, right| left.id.cmp(&right.id));
    inventory
        .assets
        .sort_by(|left, right| left.stable_id.cmp(&right.stable_id));
}
