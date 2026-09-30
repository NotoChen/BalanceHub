use super::*;
use crate::services::agent_cli::{
    self,
    contracts::{AgentContextDiscoveryRequest, AgentDiagnosticOutput},
    environment::mutation::native_test_support::catalog_fixture_inventory,
    AgentCliDefinition,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(unix)]
mod global_identity;
#[cfg(unix)]
mod jsonc_mcp;
mod provenance;
#[cfg(unix)]
mod readonly_skill_links;

pub(super) struct FixtureInspector {
    pub home: PathBuf,
    pub settings: AppSettings,
    pub discover_installations: bool,
}
impl MutationInspector for FixtureInspector {
    fn inspect(&self) -> Result<MutationInventory, AgentAssetMutationError> {
        let definitions = fixture_definitions();
        catalog_fixture_inventory(
            &self.home,
            None,
            self.discover_installations.then_some(&self.settings),
            &definitions,
        )
        .map_err(|_| AgentAssetMutationError::new(AgentAssetMutationErrorKind::PreparationFailed))
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
pub(super) fn fixture_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Vec<AgentConfigurationContext> {
    let directory = match request.agent_kind {
        AgentCliKind::ClaudeCode => ".claude",
        AgentCliKind::Codex => ".codex",
        AgentCliKind::Gemini => ".gemini",
        AgentCliKind::Grok => ".grok",
    };
    let root = request.home.join(directory);
    let mut contexts = agent_cli::definition(request.agent_kind)
        .environment
        .discover_contexts(request, output);
    for context in &mut contexts {
        context.config_root = root.to_string_lossy().into_owned();
    }
    contexts
}
pub(super) fn fixture_definitions() -> Vec<AgentCliDefinition> {
    agent_cli::definitions()
        .iter()
        .map(|definition| AgentCliDefinition {
            environment: definition.environment.with_test_contexts(fixture_contexts),
            ..*definition
        })
        .collect()
}
pub(super) fn fixture() -> (tempfile::TempDir, Arc<FixtureInspector>, CatalogService) {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let home = root.join("home");
    fs::create_dir(&home).unwrap();
    for directory in [".claude", ".codex", ".gemini", ".grok"] {
        fs::create_dir(home.join(directory)).unwrap();
    }
    let inspector = Arc::new(FixtureInspector {
        home,
        settings: AppSettings::default(),
        discover_installations: false,
    });
    let service = CatalogService::new(root.join("library"), Arc::new(MutationService::default()));

    (temporary, inspector, service)
}
pub(super) fn mcp_request(name: &str) -> AgentCatalogSaveRequest {
    AgentCatalogSaveRequest {
        hook: None,
        asset_id: None,
        expected_version: None,
        name: name.to_owned(),
        category: AgentAssetCategory::Mcp,
        mcp: Some(AgentCatalogMcpInput {
            transport: Some(AgentMcpTransport::Stdio),
            command: Some("npx".to_owned()),
            args: vec![
                "-y".to_owned(),
                "@example/server".to_owned(),
                "--port".to_owned(),
                "3000".to_owned(),
                "--token".to_owned(),
                "fixture-private-value".to_owned(),
            ],
            url: None,
            cwd: None,
            environment: BTreeMap::from([("API_KEY".to_owned(), "fixture-private-env".to_owned())]),
            headers: BTreeMap::new(),
            connection_options: BTreeMap::new(),
        }),
        skill_markdown: None,
    }
}

#[test]
fn complete_skill_package_changes_when_scripts_change_and_rejects_symlinks() {
    let (temporary, _, _) = fixture();
    let root = temporary.path().canonicalize().unwrap();
    let package = root.join("skill");
    fs::create_dir(&package).unwrap();
    fs::create_dir(package.join("scripts")).unwrap();
    fs::write(
        package.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nRead sources\n",
    )
    .unwrap();
    fs::write(package.join("scripts/run.py"), "print('one')\n").unwrap();
    let one =
        definition::DefinitionPayload::Skill(package::read_package(&package, &root).unwrap().files)
            .fingerprint();
    fs::write(package.join("scripts/run.py"), "print('two')\n").unwrap();
    let two =
        definition::DefinitionPayload::Skill(package::read_package(&package, &root).unwrap().files)
            .fingerprint();
    assert_ne!(one, two);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("outside"), package.join("link")).unwrap();
        assert!(package::read_package(&package, &root).is_err());
    }
}

#[test]
fn json_toml_mapping_keeps_credentials_and_native_options_out_of_public_identity() {
    let left = native::CODEX.decode(&serde_json::json!({"command":"npx","args":["-y","server"],"env":{"KEY":"private-a"},"enabled":false})).unwrap();
    let right = native::GEMINI
        .decode(
            &serde_json::json!({"command":"npx","args":["-y","server"],"env":{"KEY":"private-a"}}),
        )
        .unwrap();
    assert_eq!(
        definition::DefinitionPayload::Mcp(left.clone()).fingerprint(),
        definition::DefinitionPayload::Mcp(right).fingerprint()
    );
    let other = native::CODEX
        .decode(
            &serde_json::json!({"command":"npx","args":["-y","server"],"env":{"KEY":"private-b"}}),
        )
        .unwrap();
    assert_ne!(
        definition::DefinitionPayload::Mcp(left).fingerprint(),
        definition::DefinitionPayload::Mcp(other).fingerprint()
    );
}

#[test]
fn atomic_private_library_failure_preserves_previous_version_and_limited_reads() {
    let (temporary, _, service) = fixture();
    let saved = service.save(mcp_request("docs")).unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let path = root.join("library/library.json");
    let previous = fs::read(&path).unwrap();
    let mut request = mcp_request("docs");
    request.asset_id = Some(saved.asset_id);
    request.expected_version = Some(1);
    request.mcp.as_mut().unwrap().args = vec!["x".repeat(package::MAX_FILE_BYTES + 1)];
    assert!(service.save(request).is_err());
    assert_eq!(fs::read(&path).unwrap(), previous);
    let larger = root.join("large");
    fs::write(&larger, vec![b'x'; package::MAX_FILE_BYTES + 1]).unwrap();
    assert!(package::capture_file(&root, &larger, package::MAX_FILE_BYTES).is_err());
    let accepted = package::capture_file(&root, &larger, package::MAX_FILE_BYTES * 2).unwrap();
    assert!(accepted.revalidate().is_ok());
    assert!(package::capture_file(&root, &larger, 65 * 1024 * 1024).is_err());
}

#[test]
fn real_inventory_equivalent_assets_keep_identity_after_native_content_edit() {
    let (_temporary, inspector, service) = fixture();
    fs::write(
        inspector.home.join(".codex/config.toml"),
        "[mcp_servers.shared]\ncommand = 'runner'\n",
    )
    .unwrap();
    fs::write(
        inspector.home.join(".gemini/settings.json"),
        r#"{"mcpServers":{"shared":{"command":"runner"}}}"#,
    )
    .unwrap();
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    let rows = catalog
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Mcp && item.name == "shared")
        .collect::<Vec<_>>();
    let bindings = rows
        .iter()
        .flat_map(|item| item.bindings.iter().map(|binding| binding.id.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        bindings.len(),
        2,
        "actual production inventory must discover both native entries"
    );
    let id = rows[0].id.clone();
    assert_eq!(
        rows.len(),
        1,
        "complete equal definitions join automatically"
    );
    assert!(rows[0].application.available);
    assert!(rows[0].application.source_binding_id.is_some());
    fs::write(
        inspector.home.join(".gemini/settings.json"),
        r#"{"mcpServers":{"shared":{"command":"different-runner"}}}"#,
    )
    .unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let row = catalog.assets.iter().find(|item| item.id == id).unwrap();
    assert_eq!(row.bindings.len(), 2);
    assert_eq!(row.variants.len(), 2);
    assert!(!row.application.available);
    assert!(row.application.source_binding_id.is_none());
    assert_eq!(
        catalog
            .assets
            .iter()
            .filter(|item| item.category == AgentAssetCategory::Mcp && item.name == "shared")
            .count(),
        1
    );
}

#[test]
fn invalid_unknown_ipc_fields_are_rejected_before_dispatch() {
    assert!(serde_json::from_value::<AgentCatalogApplyRequest>(
        serde_json::json!({"planToken":"x","assetId":"x","action":"enable","path":"/tmp/untrusted"})
    )
    .is_err());
}

#[test]
fn shared_skill_validation_requires_valid_scalar_yaml_metadata() {
    for text in [
        "---\nname: [invalid]\ndescription: Example\n---\nBody",
        "---\nname: valid\ndescription: \"unterminated\n---\nBody",
        "---\nname: valid\ndescription: null\n---\nBody",
        "---\nname: valid\ndescription: Example\n---not-a-delimiter\nBody",
    ] {
        assert!(definition::validate_skill(text.as_bytes()).is_err());
    }
    assert!(definition::validate_skill(
        b"---\nname: valid\ndescription: |\n  Multiple lines\n  remain valid\n---\nBody"
    )
    .is_ok());
}

#[test]
fn one_physical_skill_package_keeps_one_global_id_across_native_names() {
    let (_temporary, inspector, service) = fixture();
    let package = inspector.home.join(".agents/skills/shared-review");
    fs::create_dir_all(&package).unwrap();
    fs::write(
        package.join("SKILL.md"),
        "---\nname: Shared Review\ndescription: Review sources\n---\nRead before editing\n",
    )
    .unwrap();
    let snapshot = inspector.inspect().unwrap();
    let skills = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| {
            asset.category == AgentAssetCategory::Skill && asset.relationships.provided_by.is_none()
        })
        .collect::<Vec<_>>();
    assert!(
        skills.len() >= 2,
        "production native discovery must see the shared package"
    );
    assert!(
        skills
            .iter()
            .map(|asset| &asset.native_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            >= 2,
        "fixture must exercise native naming differences"
    );
    let catalog = service.catalog(&snapshot).unwrap();
    let globals = catalog
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(globals.len(), 1);
    assert_eq!(globals[0].bindings.len(), skills.len());
}

#[test]
#[cfg(unix)]
fn production_catalog_creates_updates_tracks_drift_and_rejects_stale_plan() {
    let (_temporary, inspector, service) = fixture();
    let saved = service.save(mcp_request("shared-create")).unwrap();
    let initial = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let targets = initial
        .targets
        .iter()
        .filter(|target| target.categories.contains(&AgentAssetCategory::Mcp))
        .map(|target| target.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 4);
    let plan = service
        .plan(
            "window",
            AgentCatalogPlanRequest {
                source: AgentCatalogPlanSource::Catalog {
                    asset_id: saved.asset_id.clone(),
                    expected_version: Some(1),
                },
                action: AgentCatalogAction::ApplyDefinition,
                target_ids: targets.clone(),
                expected_revision: initial.revision,
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    assert!(plan.targets.iter().all(|target| target.available));
    let operation = service
        .start(
            "window",
            &AgentCatalogApplyRequest {
                plan_token: plan.token.clone().unwrap(),
                asset_id: saved.asset_id.clone(),
                action: plan.action,
            },
        )
        .unwrap();
    assert!(service
        .start(
            "window",
            &AgentCatalogApplyRequest {
                plan_token: plan.token.unwrap(),
                asset_id: saved.asset_id.clone(),
                action: plan.action
            }
        )
        .is_err());
    service.run_operation(&operation.id);
    let operation = service.operation("window", &operation.id).unwrap();
    assert_eq!(operation.targets.len(), 4);
    assert!(
        operation
            .targets
            .iter()
            .all(|target| target.outcome == Some(AgentAssetOperationOutcome::AppliedVerified)),
        "{:?}",
        operation.targets
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let row = catalog
        .assets
        .iter()
        .find(|item| item.id == saved.asset_id)
        .unwrap();
    assert_eq!(row.bindings.len(), 4);
    assert!(row
        .bindings
        .iter()
        .all(|binding| binding.drift == AgentCatalogDrift::InSync));
    let mut update = mcp_request("shared-create");
    update.asset_id = Some(saved.asset_id.clone());
    update.expected_version = Some(1);
    update
        .mcp
        .as_mut()
        .unwrap()
        .args
        .push("--verbose".to_owned());
    service.save(update).unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    assert!(catalog
        .assets
        .iter()
        .find(|item| item.id == saved.asset_id)
        .unwrap()
        .bindings
        .iter()
        .all(|binding| binding.drift == AgentCatalogDrift::UpdateAvailable));
    let target = catalog
        .targets
        .iter()
        .find(|target| {
            target.agent_kind == AgentCliKind::Grok
                && target.categories.contains(&AgentAssetCategory::Mcp)
        })
        .unwrap();
    let plan = service
        .plan(
            "window",
            AgentCatalogPlanRequest {
                source: AgentCatalogPlanSource::Catalog {
                    asset_id: saved.asset_id.clone(),
                    expected_version: Some(2),
                },
                action: AgentCatalogAction::ApplyDefinition,
                target_ids: vec![target.id.clone()],
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    let path = inspector.home.join(".grok/config.toml");
    let changed = fs::read_to_string(&path)
        .unwrap()
        .replace("npx", "external-runner");
    fs::write(&path, &changed).unwrap();
    let operation = service
        .start(
            "window",
            &AgentCatalogApplyRequest {
                plan_token: plan.token.unwrap(),
                asset_id: saved.asset_id.clone(),
                action: plan.action,
            },
        )
        .unwrap();
    service.run_operation(&operation.id);
    assert_eq!(
        service.operation("window", &operation.id).unwrap().targets[0].outcome,
        Some(AgentAssetOperationOutcome::UnchangedConflict)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), changed);
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    assert!(catalog
        .assets
        .iter()
        .find(|item| item.id == saved.asset_id)
        .unwrap()
        .bindings
        .iter()
        .any(|binding| binding.drift == AgentCatalogDrift::Modified));
}
