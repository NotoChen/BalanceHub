use super::{
    tests::{fixture, fixture_definitions},
    *,
};
use crate::services::agent_cli::environment::mutation::{
    native_test_support::catalog_seeded_inventory, prepare_request,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

struct NativeInspector {
    home: PathBuf,
    installation: AgentInstallation,
    settings: AppSettings,
}
impl MutationInspector for NativeInspector {
    fn inspect(&self) -> Result<MutationInventory, AgentAssetMutationError> {
        catalog_seeded_inventory(
            &self.home,
            &fixture_definitions(),
            std::slice::from_ref(&self.installation),
        )
    }
    fn home(&self) -> &Path {
        &self.home
    }
    fn workspace(&self) -> Option<&Path> {
        None
    }
    fn settings(&self) -> &AppSettings {
        &self.settings
    }
}

fn native_inspector(base: &super::tests::FixtureInspector) -> Arc<NativeInspector> {
    let executable = base.home.join("fixture-codex-never-executed");
    fs::write(
        &executable,
        "Fixture for atomic native config writes only\n",
    )
    .unwrap();
    let environment = base.inspect().unwrap().inventory.environment;
    Arc::new(NativeInspector {
        home: base.home.clone(),
        settings: AppSettings::default(),
        installation: AgentInstallation {
            id: "catalog-native-codex".into(),
            environment_id: environment.id,
            agent_kind: AgentCliKind::Codex,
            label: "Catalog native Codex fixture".into(),
            availability: AgentInstallationAvailability::Available,
            executable_path: Some(executable.to_string_lossy().into_owned()),
            executable_identity: Some(AgentExecutableIdentity {
                owner: "fixture".into(),
                canonical_path: executable.to_string_lossy().into_owned(),
                installation_source: AgentDiscoverySource::Configured,
            }),
            executable_revision: Some("catalog-native-fixture".into()),
            installed_version: Some("0.154.0".into()),
            discovery_source: AgentDiscoverySource::Configured,
            distribution: AgentCliDistribution::Npm,
            channel: AgentInstallationChannel::Stable,
            installed_version_source: AgentVersionSource::LocalExecutable,
            diagnostics: Vec::new(),
        },
    })
}

#[test]
fn production_catalog_composes_two_of_ten_native_mcp_entries_without_losing_neighbors() {
    let (_temporary, base, service) = fixture();
    let inspector = native_inspector(&base);
    let path = inspector.home.join(".codex/config.toml");
    let mut config = String::from("# Root comment must survive\nfixture_unknown = 'keep-root'\n");
    for index in 0..10 {
        config.push_str(&format!("\n# MCP {index}\n[mcp_servers.bh-batch-{index}]\ncommand = 'fixture-never-run'\nargs = ['{index}']\nenabled = true\n"));
    }
    fs::write(&path, &config).unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let rows = catalog
        .assets
        .iter()
        .filter(|asset| {
            asset.category == AgentAssetCategory::Mcp && asset.name.starts_with("bh-batch-")
        })
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 10);
    assert!(rows.iter().all(|asset| asset.bindings.len() == 1));
    let selected = ["bh-batch-0", "bh-batch-1"]
        .map(|name| rows.iter().find(|asset| asset.name == name).unwrap());
    let id = selected[0].id.clone();
    let targets = selected
        .iter()
        .map(|asset| asset.bindings[0].id.clone())
        .collect::<Vec<_>>();
    assert_ne!(targets[0], targets[1]);
    assert_eq!(
        selected[0].bindings[0].native.inspection_source_id,
        selected[1].bindings[0].native.inspection_source_id
    );
    let relation = service
        .preview_relation(
            "window",
            AgentCatalogRelationPreviewRequest {
                intent: AgentCatalogRelationIntent::Merge {
                    destination_asset_id: id.clone(),
                    source_asset_id: selected[1].id.clone(),
                },
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    assert!(relation.available, "{:?}", relation.reason);
    service
        .commit_relation(
            "window",
            AgentCatalogRelationCommitRequest {
                plan_token: relation.token.unwrap(),
                relation_key: relation.relation_key,
                action: relation.action,
            },
        )
        .unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let plan = service
        .plan(
            "window",
            AgentCatalogPlanRequest {
                source: AgentCatalogPlanSource::Catalog {
                    asset_id: id.clone(),
                    expected_version: None,
                },
                action: AgentCatalogAction::Disable,
                target_ids: targets.clone(),
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    assert_eq!(plan.targets.len(), 2);
    assert!(
        plan.targets.iter().all(|target| target.available),
        "{:?}",
        plan.targets
    );
    let operation = service
        .start(
            "window",
            &AgentCatalogApplyRequest {
                plan_token: plan.token.unwrap(),
                asset_id: id,
                action: plan.action,
            },
        )
        .unwrap();
    service.run_operation(&operation.id);
    let done = service.operation("window", &operation.id).unwrap();
    assert!(
        done.targets
            .iter()
            .all(|target| target.outcome == Some(AgentAssetOperationOutcome::AppliedVerified)),
        "{:?}",
        done.targets
    );
    let snapshot = inspector.inspect().unwrap();
    let native = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| asset.native_id.starts_with("bh-batch-"))
        .collect::<Vec<_>>();
    assert_eq!(native.len(), 10);
    for asset in native {
        let expected = if ["bh-batch-0", "bh-batch-1"].contains(&asset.native_id.as_str()) {
            AgentAssetState::Disabled
        } else {
            AgentAssetState::Enabled
        };
        assert_eq!(asset.effective_state, expected);
        assert_eq!(asset.declared_state, expected);
    }
    let updated = fs::read_to_string(&path).unwrap();
    assert!(
        updated.contains("# Root comment must survive")
            && updated.contains("fixture_unknown = 'keep-root'")
    );
    for index in 0..10 {
        assert!(updated.contains(&format!("# MCP {index}")));
    }
}

#[test]
fn pinned_native_replan_rejects_new_valid_revision_and_executable_replacement() {
    let (_temporary, base, service) = fixture();
    let inspector = native_inspector(&base);
    let path = inspector.home.join(".codex/config.toml");
    fs::write(
        &path,
        "[mcp_servers.pinned]\ncommand = 'fixture-runner'\nenabled = true\n",
    )
    .unwrap();
    let snapshot = inspector.inspect().unwrap();
    let asset = snapshot
        .inventory
        .assets
        .iter()
        .find(|asset| asset.native_id == "pinned")
        .unwrap();
    let mut request = AgentAssetPlanRequest {
        asset_id: asset.stable_id.clone(),
        action: AgentAssetActionKind::Disable,
        workspace: None,
        expected_revision: asset.revision.identity.clone(),
        installation_id: asset.selected_action_installation_id.clone(),
    };
    let prepared = prepare_request(inspector.as_ref(), &snapshot, &request)
        .unwrap()
        .0;
    let external = "[mcp_servers.pinned]\ncommand = 'external-runner'\nenabled = true\n";
    fs::write(&path, external).unwrap();
    let snapshot = inspector.inspect().unwrap();
    request.expected_revision = snapshot
        .inventory
        .assets
        .iter()
        .find(|asset| asset.native_id == "pinned")
        .unwrap()
        .revision
        .identity
        .clone();
    assert!(service
        .native
        .plan_pinned("window", request, inspector.clone(), &prepared.signature)
        .is_err());
    fs::write(
        inspector.installation.executable_path.as_ref().unwrap(),
        "replacement installation\n",
    )
    .unwrap();
    assert!(prepared.revalidate().is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), external);
}
