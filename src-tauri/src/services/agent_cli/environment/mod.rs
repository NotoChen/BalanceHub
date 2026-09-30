//! Bounded, read-only inventory of documented Agent user/workspace resources.
//!
//! This module deliberately does not execute any discovered hook, plugin or status command.

pub(crate) mod access_registry;
mod adapter_support;
pub(crate) mod config_document;
mod diagnostics;
mod hook_counts;
mod identity;
mod inventory;
pub(crate) mod mechanism;
pub(crate) mod mutation;
mod output;
mod path_access;
pub(crate) mod preview;
mod projection;
mod provenance;
pub(super) mod run;
mod snapshot;
mod state_projection;
pub(crate) mod verified_path;
mod versioning;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(in crate::services::agent_cli) use projection::{
    fixture_assessor, fixture_draft, project_records, AgentAssetProjectionInput,
};

pub(super) use adapter_support::{
    aggregate_relationships, availability_from_declared, bounded_object_entries,
    declaration_matches_source, declaration_trust_for_participation, default_context,
    emit_unknown_fields, finalize_projected_draft, parse_json, parsed_asset, physical_origin,
    qualified_projection_key, select_draft_structure, source, source_with_logical_origins,
    ParsedAssetInput, SourceInput,
};
pub(crate) use identity::source_stable_id;
pub(crate) use identity::{lexical_identity, stable_id};
pub(crate) use inventory::{
    configuration_contexts, configuration_source_allowed, display_inventory, installation_contexts,
    inventory_for_agents_cancellable, inventory_with_access, normalize_optional_workspace,
    ConfigurationContexts, InventoryBuild,
};
pub(crate) use path_access::{
    acknowledge_risks, open_asset, open_source, open_verified_source, read_asset, read_source,
};
pub(super) use state_projection::{
    compose_effective_state, details_declared_state, intrinsic_basis, toggle_action_is_relevant,
};
pub(crate) use versioning::{parse_semantic_version, releases, version_channel, version_state};

#[cfg(test)]
pub(crate) fn build_inventory_for_test(
    home: &std::path::Path,
    workspace: Option<&std::path::Path>,
    definitions: &[crate::services::agent_cli::AgentCliDefinition],
) -> Result<crate::models::AgentEnvironmentInventory, String> {
    use std::sync::Arc;

    inventory::build_inventory_with(
        inventory::InventoryInput {
            home,
            workspace,
            settings: None,
        },
        inventory::InventoryPipelineDeps {
            definitions,
            limits: crate::models::AgentAssetLimits::DEFAULT,
            clock: Arc::new(run::SystemMonotonicClock),
            installations: &inventory::RealInstallationDiscoveryPort,
            snapshots: &snapshot::RealSnapshotPort::default(),
            checkpoint_probe: None,
        },
    )
}

/// Initial descriptors are read-only. The native mechanism gate alone may
/// enable mutations after the complete projection has been validated.
pub(super) fn read_only_action_set() -> Vec<crate::models::AgentAssetAction> {
    use crate::models::{
        AgentAssetAction, AgentAssetActionKind, AgentAssetActionUnavailableReason,
    };

    [
        AgentAssetActionKind::Inspect,
        AgentAssetActionKind::Preview,
        AgentAssetActionKind::Open,
        AgentAssetActionKind::Reveal,
        AgentAssetActionKind::Enable,
        AgentAssetActionKind::Disable,
        AgentAssetActionKind::Remove,
    ]
    .into_iter()
    .map(|action| {
        let mutation = matches!(
            action,
            AgentAssetActionKind::Enable
                | AgentAssetActionKind::Disable
                | AgentAssetActionKind::Remove
        );
        AgentAssetAction {
            action,
            available: !mutation,
            reason: mutation.then_some(AgentAssetActionUnavailableReason::MutationDisabled),
            mechanism_id: None,
            confirmation_required: mutation
                || matches!(
                    action,
                    AgentAssetActionKind::Open | AgentAssetActionKind::Reveal
                ),
            reload_effect: None,
            trust_effect: None,
            selected_installation_id: None,
            risks: Vec::new(),
        }
    })
    .collect()
}
