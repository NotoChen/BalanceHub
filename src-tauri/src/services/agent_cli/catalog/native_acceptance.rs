//! Explicit final acceptance against discovered local CLI installations and a
//! synthetic native HOME. Execute the compiled test only in the task sandbox.
use super::tests::FixtureInspector;
use super::*;
use crate::services::agent_cli::{self, environment::config_document};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

mod roles;

pub(super) fn copy_stage_directory(source: &Path, target: &Path) {
    fs::create_dir(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        assert!(
            !kind.is_symlink(),
            "acceptance snapshots never follow links"
        );
        if kind.is_dir() {
            copy_stage_directory(&entry.path(), &target.join(entry.file_name()));
        } else {
            assert!(kind.is_file());
            fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    fixture_root: PathBuf,
    home: PathBuf,
    cli_paths: BTreeMap<AgentCliKind, String>,
}

fn is_user_native_target(snapshot: &MutationInventory, target: &AgentCatalogTarget) -> bool {
    target.scope == AgentAssetScope::User
        && projection::resolve_target(snapshot, &target.id).is_ok_and(|(context, source, _)| {
            source.scope == AgentAssetScope::User
                && if target.categories == [AgentAssetCategory::Skill] {
                    source.origin == AgentAssetInstallationOrigin::LocalFiles
                        && Path::new(&source.path) == Path::new(&context.config_root).join("skills")
                } else {
                    source.origin == AgentAssetInstallationOrigin::ConfigEntry
                }
        })
}

#[test]
#[ignore = "requires exact local CLIs, synthetic native HOME and external deny-network sandbox"]
fn production_catalog_distribution_acceptance_eight_cells() {
    assert_eq!(
        std::env::var("BALANCEHUB_CATALOG_ACCEPTANCE_SANDBOX").as_deref(),
        Ok("1")
    );
    let fixture: Fixture = serde_json::from_slice(
        &fs::read(
            std::env::var_os("BALANCEHUB_CATALOG_ACCEPTANCE_FIXTURE")
                .expect("explicit fixture manifest"),
        )
        .unwrap(),
    )
    .unwrap();
    let root = fixture.fixture_root.canonicalize().unwrap();
    let home = fixture.home.canonicalize().unwrap();
    assert!(home.starts_with(&root) && home != root);
    let mut settings = AppSettings::default();
    for (&kind, path) in &fixture.cli_paths {
        settings.set_agent_cli_path(kind, path.clone());
    }
    let inspector = Arc::new(FixtureInspector {
        home,
        settings,
        discover_installations: true,
    });
    let service = CatalogService::new(
        root.join("catalog-acceptance-library"),
        Arc::new(MutationService::default()),
    );

    let initial = inspector.inspect().unwrap();
    let mcp_targets = projection::targets(&initial.inventory)
        .into_iter()
        .filter(|target| {
            target.categories == [AgentAssetCategory::Mcp]
                && is_user_native_target(&initial, target)
        })
        .collect::<Vec<_>>();
    assert_eq!(mcp_targets.len(), 4);
    for target in &mcp_targets {
        let (_, source, adapter) = projection::resolve_target(&initial, &target.id).unwrap();
        let path = PathBuf::from(&source.path);
        assert!(path.starts_with(&inspector.home));
        let bytes = fs::read(&path).ok();
        let definition = adapter
            .decode(&serde_json::json!({"command":"/usr/bin/true"}))
            .unwrap();
        let bytes = adapter
            .patch_mcp(bytes.as_deref(), "bh-catalog-observed", &definition)
            .unwrap();
        let mut root = config_document::parse(&bytes, adapter.format).unwrap();
        root["balancehubCatalogSentinel"] = serde_json::json!({"value":"keep"});
        fs::write(
            path,
            config_document::serialize(&root, adapter.format).unwrap(),
        )
        .unwrap();
    }
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let observed = catalog
        .assets
        .iter()
        .filter(|asset| {
            asset.category == AgentAssetCategory::Mcp && asset.name == "bh-catalog-observed"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        observed.len(),
        1,
        "complete equivalent independent sources must share one identity"
    );
    let id = observed[0].id.clone();
    let associated = observed[0];
    assert_eq!(associated.bindings.len(), 4);
    assert_eq!(associated.variants.len(), 1);
    assert!(associated.application.available);
    assert!(associated.version.is_none());
    let adopted = service
        .adopt(
            AgentCatalogAdoptRequest {
                binding_id: associated.application.source_binding_id.clone().unwrap(),
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    assert_eq!(adopted.asset_id, id);

    let mut cells = Vec::new();
    for category in [AgentAssetCategory::Mcp, AgentAssetCategory::Skill] {
        let name = if category == AgentAssetCategory::Mcp {
            "bh-catalog-created-mcp"
        } else {
            "bh-catalog-created-skill"
        };
        let saved = service.save(AgentCatalogSaveRequest { hook: None, asset_id: None, expected_version: None, name: name.to_owned(), category, mcp: (category == AgentAssetCategory::Mcp).then(|| AgentCatalogMcpInput { transport: Some(AgentMcpTransport::Stdio), command: Some("/usr/bin/true".to_owned()), args: Vec::new(), url: None, cwd: None, environment: BTreeMap::from([("BALANCEHUB_FIXTURE_SECRET".to_owned(), "isolated-catalog-fixture".to_owned())]), headers: BTreeMap::new(), connection_options: BTreeMap::new() }), skill_markdown: (category == AgentAssetCategory::Skill).then(|| format!("---\nname: {name}\ndescription: Native catalog acceptance\n---\nbefore-change\n")) }).unwrap();
        let snapshot = inspector.inspect().unwrap();
        let catalog = service.catalog(&snapshot).unwrap();
        let targets = catalog
            .targets
            .iter()
            .filter(|target| {
                target.categories.contains(&category) && is_user_native_target(&snapshot, target)
            })
            .map(|target| target.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(targets.len(), 4);
        for version in 1..=2 {
            let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
            let plan = service
                .plan(
                    "catalog-native-acceptance",
                    AgentCatalogPlanRequest {
                        source: AgentCatalogPlanSource::Catalog {
                            asset_id: saved.asset_id.clone(),
                            expected_version: Some(version),
                        },
                        action: AgentCatalogAction::ApplyDefinition,
                        target_ids: targets.clone(),
                        expected_revision: catalog.revision,
                        workspace: None,
                    },
                    inspector.clone(),
                )
                .unwrap();
            assert!(plan.targets.iter().all(|target| target.available));
            let operation = service
                .start(
                    "catalog-native-acceptance",
                    &AgentCatalogApplyRequest {
                        plan_token: plan.token.unwrap(),
                        asset_id: saved.asset_id.clone(),
                        action: plan.action,
                    },
                )
                .unwrap();
            service.run_operation(&operation.id);
            let operation = service
                .operation("catalog-native-acceptance", &operation.id)
                .unwrap();
            assert!(
                operation
                    .targets
                    .iter()
                    .all(|target| target.outcome
                        == Some(AgentAssetOperationOutcome::AppliedVerified)),
                "{:?}",
                operation.targets
            );
            let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
            let asset = catalog
                .assets
                .iter()
                .find(|asset| asset.id == saved.asset_id)
                .unwrap();
            assert_eq!(asset.bindings.len(), 4);
            assert!(asset
                .bindings
                .iter()
                .all(|binding| binding.drift == AgentCatalogDrift::InSync
                    && binding.applied_version == Some(version)));
            if version == 1 {
                let mut current = service.definition(&saved.asset_id).unwrap();
                if let Some(mcp) = &mut current.mcp {
                    mcp.args = vec!["--balancehub-fixture-revision=2".to_owned()];
                    mcp.environment
                        .insert("BALANCEHUB_FIXTURE_REVISION".to_owned(), "2".to_owned());
                }
                let markdown = current.skill_markdown.map(|body| {
                    body.replace("before-change", "after-change").replace(
                        "description: Native catalog acceptance\n",
                        "description: Native catalog acceptance revision 2\n",
                    )
                });
                service
                    .save(AgentCatalogSaveRequest {
                        hook: None,
                        asset_id: Some(saved.asset_id.clone()),
                        expected_version: Some(1),
                        name: current.name,
                        category,
                        mcp: current.mcp,
                        skill_markdown: markdown,
                    })
                    .unwrap();
            } else {
                for binding in &asset.bindings {
                    let installation = catalog
                        .inventory
                        .installations
                        .iter()
                        .find(|installation| {
                            installation.agent_kind == binding.native.agent_kind
                                && installation.availability
                                    == AgentInstallationAvailability::Available
                                && binding
                                    .native
                                    .compatible_installation_ids
                                    .contains(&installation.id)
                        })
                        .expect("exact local installation available");
                    cells.push(serde_json::json!({"adapter":agent_cli::definition(binding.native.agent_kind).environment.catalog_adapter().unwrap().key,"category":category,"platform":std::env::consts::OS,"architecture":std::env::consts::ARCH,"version":installation.installed_version,"fixtureDigest":native::evidence::fixture_digest(),"serviceVerified":true,"verified":false,"destinationRoles":["userNative"],"note":"create/update/read-back passed; record independent native inspect before certifying production writes"}));
                }
            }
        }
        assert_drift_stale_and_missing(
            &service,
            inspector.clone(),
            &saved.asset_id,
            &targets[0],
            name,
            category,
        );
    }
    let final_snapshot = inspector.inspect().unwrap();
    for target in mcp_targets {
        let (_, source, adapter) = projection::resolve_target(&final_snapshot, &target.id).unwrap();
        let bytes = fs::read(&source.path).unwrap();
        let root = config_document::parse(&bytes, adapter.format).unwrap();
        assert_eq!(root["balancehubCatalogSentinel"]["value"], "keep");
    }
    assert_eq!(cells.len(), 8);
    let role_fixtures = roles::produce(&root, &fixture.cli_paths);
    assert_eq!(role_fixtures.len(), 15);
    fs::write(
        root.join("catalog-distribution-acceptance.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"revision":1,"cells":cells,"roleFixtures":role_fixtures}),
        )
        .unwrap(),
    )
    .unwrap();
}

fn assert_drift_stale_and_missing(
    service: &CatalogService,
    inspector: Arc<FixtureInspector>,
    asset_id: &str,
    target_id: &str,
    name: &str,
    category: AgentAssetCategory,
) {
    let plan = |service: &CatalogService| {
        let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
        service
            .plan(
                "catalog-native-acceptance",
                AgentCatalogPlanRequest {
                    source: AgentCatalogPlanSource::Catalog {
                        asset_id: asset_id.to_owned(),
                        expected_version: Some(2),
                    },
                    action: AgentCatalogAction::ApplyDefinition,
                    target_ids: vec![target_id.to_owned()],
                    expected_revision: catalog.revision,
                    workspace: None,
                },
                inspector.clone(),
            )
            .unwrap()
    };
    let run = |plan: AgentCatalogPlan| {
        assert!(plan.targets.iter().all(|target| target.available));
        let operation = service
            .start(
                "catalog-native-acceptance",
                &AgentCatalogApplyRequest {
                    plan_token: plan.token.unwrap(),
                    asset_id: asset_id.to_owned(),
                    action: plan.action,
                },
            )
            .unwrap();
        service.run_operation(&operation.id);
        service
            .operation("catalog-native-acceptance", &operation.id)
            .unwrap()
            .targets[0]
            .outcome
    };
    let stale = plan(service);
    let snapshot = inspector.inspect().unwrap();
    let (_, source, adapter) = projection::resolve_target(&snapshot, target_id).unwrap();
    let path = if category == AgentAssetCategory::Mcp {
        PathBuf::from(&source.path)
    } else {
        PathBuf::from(&source.path).join(name).join("SKILL.md")
    };
    let original = fs::read(&path).unwrap();
    if category == AgentAssetCategory::Mcp {
        let mut root = config_document::parse(&original, adapter.format).unwrap();
        root[adapter.table][name]["command"] = serde_json::json!("/usr/bin/false");
        fs::write(
            &path,
            config_document::serialize(&root, adapter.format).unwrap(),
        )
        .unwrap();
    } else {
        let text = String::from_utf8(original)
            .unwrap()
            .replace("after-change", "outside-change");
        fs::write(&path, text).unwrap();
    }
    let outside = fs::read(&path).unwrap();
    assert_eq!(
        run(stale),
        Some(AgentAssetOperationOutcome::UnchangedConflict)
    );
    assert_eq!(fs::read(&path).unwrap(), outside);
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let asset = catalog
        .assets
        .iter()
        .find(|asset| asset.id == asset_id)
        .unwrap();
    assert!(asset
        .bindings
        .iter()
        .any(|binding| binding.drift == AgentCatalogDrift::Modified));
    if category == AgentAssetCategory::Mcp {
        let mut root = config_document::parse(&outside, adapter.format).unwrap();
        root[adapter.table].as_object_mut().unwrap().remove(name);
        fs::write(
            &path,
            config_document::serialize(&root, adapter.format).unwrap(),
        )
        .unwrap();
    } else {
        fs::remove_file(&path).unwrap();
    }
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let asset = catalog
        .assets
        .iter()
        .find(|asset| asset.id == asset_id)
        .unwrap();
    assert!(asset
        .unresolved_targets
        .iter()
        .any(|target| target.target_id == target_id && target.drift == AgentCatalogDrift::Missing));
    assert_eq!(
        run(plan(service)),
        Some(AgentAssetOperationOutcome::AppliedVerified)
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let asset = catalog
        .assets
        .iter()
        .find(|asset| asset.id == asset_id)
        .unwrap();
    assert_eq!(asset.bindings.len(), 4);
    assert!(asset
        .bindings
        .iter()
        .all(|binding| binding.drift == AgentCatalogDrift::InSync));
}
