use super::{
    AgentAssetAccessRegistry, AgentAssetAccessRequest, AgentAssetAccessTargetKind,
    AgentSourceAccessEvidence,
};
use crate::{
    models::{
        AgentAssetAccess, AgentAssetCategory, AgentAssetLimits, AgentAssetScope,
        AgentAssetSourceKind, AgentCliKind, AgentEnvironmentInventory,
    },
    services::agent_cli::{
        contracts::{AgentAssetLogicalOrigin, AgentAssetSourceSpec},
        definition,
        environment::{
            inventory::{
                build_inventory_with, InventoryInput, InventoryPipelineDeps,
                RealInstallationDiscoveryPort,
            },
            preview::AgentSourcePreviewPolicy,
            run::SystemMonotonicClock,
            snapshot::{RealSnapshotPort, SnapshotPort},
        },
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub(in crate::services::agent_cli::environment) struct AccessFixture {
    pub(in crate::services::agent_cli::environment) root: PathBuf,
    pub(in crate::services::agent_cli::environment) file: PathBuf,
}

impl AccessFixture {
    pub(in crate::services::agent_cli::environment) fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "balancehub-verified-access-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join(".codex")).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let file = root.join(".codex/config.toml");
        fs::write(&file, "[mcp_servers.fixture]\ncommand='node'\nargs=['server.js','--port','3000']\nenabled=true\n").unwrap();
        Self { root, file }
    }

    pub(in crate::services::agent_cli::environment) fn source_spec(&self) -> AgentAssetSourceSpec {
        AgentAssetSourceSpec {
            hook_definition_source: true,
            verified_physical_path: None,
            provider: crate::models::AgentAssetProviderOrigin::Unknown,
            origin: crate::models::AgentAssetInstallationOrigin::Unknown,
            path_policy: Default::default(),
            native_source_key: "config".to_string(),
            label: "测试配置".to_string(),
            scope: AgentAssetScope::User,
            path: self.file.clone(),
            allowed_root: self.root.join(".codex"),
            precedence: 10,
            writable: false,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Mcp],
            allowed_logical_origins: vec![AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            }],
        }
    }

    pub(in crate::services::agent_cli::environment) fn build(
        &self,
    ) -> (AgentEnvironmentInventory, Vec<AgentSourceAccessEvidence>) {
        let snapshots = RealSnapshotPort::default();
        let inventory = build_inventory_with(
            InventoryInput {
                home: &self.root,
                workspace: None,
                settings: None,
            },
            InventoryPipelineDeps {
                definitions: std::slice::from_ref(definition(AgentCliKind::Codex)),
                limits: AgentAssetLimits::DEFAULT,
                clock: Arc::new(SystemMonotonicClock),
                installations: &RealInstallationDiscoveryPort,
                snapshots: &snapshots,
                checkpoint_probe: None,
            },
        )
        .unwrap();
        let evidence = inventory
            .sources
            .iter()
            .filter_map(|source| {
                let anchor = snapshots.access_anchor(&source.revision)?;
                let policy = if Path::new(&source.path) == self.file {
                    definition(AgentCliKind::Codex)
                        .environment()
                        .source_preview(&self.source_spec())
                } else {
                    AgentSourcePreviewPolicy::metadata_only()
                };
                Some(AgentSourceAccessEvidence {
                    source_id: source.id.clone(),
                    anchor,
                    policy,
                })
            })
            .collect();
        (inventory, evidence)
    }

    pub(in crate::services::agent_cli::environment) fn publish(
        &self,
        registry: &AgentAssetAccessRegistry,
        actor: &str,
    ) -> AgentEnvironmentInventory {
        let (mut inventory, evidence) = self.build();
        registry.publish(actor, &mut inventory, evidence).unwrap();
        inventory
    }
}

impl Drop for AccessFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(in crate::services::agent_cli::environment) fn access_id(access: &AgentAssetAccess) -> &str {
    match access {
        AgentAssetAccess::Ready { access_id } => access_id,
        AgentAssetAccess::Unavailable { reason } => {
            panic!("fixture access must be ready: {reason:?}")
        }
    }
}

pub(in crate::services::agent_cli::environment) fn asset_request<'a>(
    inventory: &'a AgentEnvironmentInventory,
    actor: &'a str,
) -> AgentAssetAccessRequest<'a> {
    let asset = inventory
        .assets
        .iter()
        .find(|asset| asset.native_id == "fixture")
        .unwrap();
    AgentAssetAccessRequest {
        actor,
        environment_id: &inventory.environment.id,
        workspace: inventory.workspace.as_deref().map(Path::new),
        target_id: &asset.stable_id,
        access_id: access_id(&asset.access),
        target_kind: AgentAssetAccessTargetKind::Asset,
    }
}

pub(in crate::services::agent_cli::environment) fn source_request<'a>(
    inventory: &'a AgentEnvironmentInventory,
    actor: &'a str,
) -> AgentAssetAccessRequest<'a> {
    let asset = inventory
        .assets
        .iter()
        .find(|asset| asset.native_id == "fixture")
        .unwrap();
    let source = inventory
        .sources
        .iter()
        .find(|source| source.id == asset.inspection_source_id)
        .unwrap();
    AgentAssetAccessRequest {
        actor,
        environment_id: &inventory.environment.id,
        workspace: inventory.workspace.as_deref().map(Path::new),
        target_id: &source.id,
        access_id: access_id(&source.access),
        target_kind: AgentAssetAccessTargetKind::Source,
    }
}
