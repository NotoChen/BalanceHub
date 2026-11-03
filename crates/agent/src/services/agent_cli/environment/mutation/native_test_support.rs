//! Test inventory uses the production builder and retains that scan's anchors.
//! The default MutationInspector preparer always dispatches to the native adapter.

use super::super::{
    inventory::{
        build_inventory_with, InstallationDiscoveryPort, InventoryInput, InventoryPipelineDeps,
    },
    run::SystemMonotonicClock,
    snapshot::{RealSnapshotPort, SnapshotPort},
};
use super::{MutationInspector, MutationInventory};
use crate::{models::*, services::agent_cli::AgentCliDefinition};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) struct NativeFixtureInspector {
    pub(super) home: PathBuf,
    pub(super) workspace: Option<PathBuf>,
    pub(super) settings: AppSettings,
    pub(super) definitions: Vec<AgentCliDefinition>,
    pub(super) installations: Arc<dyn InstallationDiscoveryPort>,
}

impl MutationInspector for NativeFixtureInspector {
    fn inspect(&self) -> Result<MutationInventory, AgentAssetMutationError> {
        let snapshots = RealSnapshotPort::default();
        let inventory = build_inventory_with(
            InventoryInput {
                home: &self.home,
                workspace: self.workspace.as_deref(),
                settings: Some(&self.settings),
            },
            InventoryPipelineDeps {
                definitions: &self.definitions,
                limits: AgentAssetLimits::defaults(),
                clock: Arc::new(SystemMonotonicClock),
                installations: self.installations.as_ref(),
                snapshots: &snapshots,
                checkpoint_probe: None,
            },
        )
        .map_err(|_| {
            AgentAssetMutationError::new(AgentAssetMutationErrorKind::PreparationFailed)
        })?;
        // Never reopen a source and assign its new bytes an old revision.
        let source_anchors = inventory
            .sources
            .iter()
            .filter_map(|source| {
                snapshots
                    .access_anchor(&source.revision)
                    .map(|anchor| (source.id.clone(), anchor))
            })
            .collect();
        Ok(MutationInventory {
            inventory,
            source_anchors,
        })
    }

    fn home(&self) -> &Path {
        &self.home
    }
    fn workspace(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }
    fn settings(&self) -> &AppSettings {
        &self.settings
    }
}

/// Catalog integration fixtures keep the production inventory and its original anchors.
pub(crate) fn catalog_fixture_inventory(
    home: &Path,
    workspace: Option<&Path>,
    settings: Option<&AppSettings>,
    definitions: &[AgentCliDefinition],
) -> Result<MutationInventory, String> {
    let snapshots = RealSnapshotPort::default();
    let inventory = build_inventory_with(
        InventoryInput {
            home,
            workspace,
            settings,
        },
        InventoryPipelineDeps {
            definitions,
            limits: AgentAssetLimits::defaults(),
            clock: Arc::new(SystemMonotonicClock),
            installations: &super::super::inventory::RealInstallationDiscoveryPort,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )?;
    let source_anchors = inventory
        .sources
        .iter()
        .filter_map(|source| {
            snapshots
                .access_anchor(&source.revision)
                .map(|anchor| (source.id.clone(), anchor))
        })
        .collect();
    Ok(MutationInventory {
        inventory,
        source_anchors,
    })
}

/// Exercise native catalog batches with explicit fixture installations, without
/// probing or launching any CLI from the machine running ordinary unit tests.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn catalog_seeded_inventory(
    home: &Path,
    definitions: &[AgentCliDefinition],
    installations: &[AgentInstallation],
) -> Result<MutationInventory, AgentAssetMutationError> {
    struct SeededInstallations(Vec<AgentInstallation>);
    impl InstallationDiscoveryPort for SeededInstallations {
        fn discover(
            &self,
            request: super::super::inventory::InstallationDiscoveryRequest<'_>,
            _: &mut super::super::run::AgentInventoryRun,
        ) -> Vec<AgentInstallation> {
            self.0
                .iter()
                .filter(|installation| installation.agent_kind == request.definition.kind)
                .cloned()
                .collect()
        }
    }
    NativeFixtureInspector {
        home: home.to_path_buf(),
        workspace: None,
        settings: AppSettings::default(),
        definitions: definitions.to_vec(),
        installations: Arc::new(SeededInstallations(installations.to_vec())),
    }
    .inspect()
}
