use super::super::{
    inventory::{native_environment, InstallationDiscoveryPort, InstallationDiscoveryRequest},
    run::AgentInventoryRun,
};
use super::{
    native_test_support::NativeFixtureInspector, prepared::prepare_request, MutationExecution,
    MutationInspector, MutationService,
};
use crate::{models::*, services::agent_cli::definition};
use std::{fs, path::Path, sync::Arc};

const SKILL: &str = "review-helper";
const MANIFEST: &str =
    "---\nname: review-helper\ndescription: Isolated plan guard fixture\n---\nDo not execute commands.\n";
const INITIAL_CONFIG: &str = "[skills]\ndisabled = ['review-helper']\n";
const CHANGED_CONFIG: &str = "[skills]\ndisabled = ['review-helper', 'unrelated-skill']\n";

struct GrokFixtureInstallation(AgentInstallation);

impl InstallationDiscoveryPort for GrokFixtureInstallation {
    fn discover(
        &self,
        request: InstallationDiscoveryRequest<'_>,
        _: &mut AgentInventoryRun,
    ) -> Vec<AgentInstallation> {
        assert_eq!(request.definition.kind, AgentCliKind::Grok);
        vec![self.0.clone()]
    }
}

fn inspector(home: &Path) -> NativeFixtureInspector {
    let home = home.canonicalize().unwrap();
    let executable = home.join("fixture-grok-not-executable");
    // Discovery never launches a process; the builtin Skill preparer is atomic.
    // The selected installation cannot accidentally resolve to a real CLI.
    fs::write(&executable, "Non-executable native plan fixture\n").unwrap();
    let installation = AgentInstallation {
        id: "fixture-grok".into(),
        environment_id: native_environment().id,
        agent_kind: AgentCliKind::Grok,
        label: "Grok plan fixture".into(),
        availability: AgentInstallationAvailability::Available,
        executable_path: Some(executable.to_string_lossy().into_owned()),
        executable_identity: Some(AgentExecutableIdentity {
            owner: "fixture".into(),
            canonical_path: executable.to_string_lossy().into_owned(),
            installation_source: AgentDiscoverySource::Configured,
        }),
        executable_revision: Some("fixture-grok-executable".into()),
        installed_version: Some("1.0.24".into()),
        discovery_source: AgentDiscoverySource::Configured,
        distribution: AgentCliDistribution::VendorNative,
        channel: AgentInstallationChannel::Stable,
        installed_version_source: AgentVersionSource::LocalExecutable,
        diagnostics: vec![],
    };
    NativeFixtureInspector {
        home,
        workspace: None,
        settings: AppSettings::default(),
        definitions: vec![*definition(AgentCliKind::Grok)],
        installations: Arc::new(GrokFixtureInstallation(installation)),
    }
}

fn skill(inventory: &AgentEnvironmentInventory) -> &AgentAssetRecord {
    let skills = inventory
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(skills.len(), 1, "{skills:?}");
    let asset = skills[0];
    assert_eq!(asset.native_id, SKILL);
    assert_eq!(asset.effective_state, AgentAssetState::Disabled);
    assert!(asset
        .actions
        .iter()
        .any(|action| action.action == AgentAssetActionKind::Enable && action.available));
    let definitions = inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.context_id == asset.context_id
                && declaration.native_kind == AgentAssetCategory::Skill
                && declaration.native_id == SKILL
                && declaration.role == AgentAssetDeclarationRole::Definition
        })
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 1);
    let definition = definitions[0];
    assert_eq!(definition.source_id, asset.inspection_source_id);
    assert_eq!(
        definition.evidence.revision.identity,
        asset.revision.identity
    );
    assert!(asset.source_ids.contains(&definition.source_id));
    assert!(asset.represented_declaration_ids.contains(&definition.id));
    assert!(asset.resolution.contributor_ids.contains(&definition.id));
    asset
}

fn control_source<'a>(
    inventory: &'a AgentEnvironmentInventory,
    asset: &AgentAssetRecord,
    path: &Path,
    expected_disabled: &[&str],
) -> &'a AgentAssetSource {
    let sources = inventory
        .sources
        .iter()
        .filter(|source| source.context_id == asset.context_id && Path::new(&source.path) == path)
        .collect::<Vec<_>>();
    assert_eq!(sources.len(), 1);
    let source = sources[0];
    assert!(!source.revision.is_missing);
    assert!(source.diagnostics.is_empty(), "{:?}", source.diagnostics);
    assert!(asset.source_ids.contains(&source.id));
    let mut disabled_names = inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.context_id == asset.context_id
                && declaration.source_id == source.id
                && declaration.native_kind == AgentAssetCategory::Skill
                && declaration.role == AgentAssetDeclarationRole::StateOverlay
        })
        .map(|declaration| declaration.native_id.as_str())
        .collect::<Vec<_>>();
    disabled_names.sort_unstable();
    assert_eq!(disabled_names.as_slice(), expected_disabled);
    let overlays = inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.context_id == asset.context_id
                && declaration.source_id == source.id
                && declaration.native_kind == AgentAssetCategory::Skill
                && declaration.native_id == SKILL
                && declaration.role == AgentAssetDeclarationRole::StateOverlay
        })
        .collect::<Vec<_>>();
    assert_eq!(overlays.len(), 1);
    let overlay = overlays[0];
    assert_eq!(overlay.declaration_key, "skills.disabled:review-helper");
    assert_eq!(overlay.declared_state, AgentAssetDeclaredState::Disabled);
    assert_eq!(overlay.evidence.revision.identity, source.revision.identity);
    assert!(asset.represented_declaration_ids.contains(&overlay.id));
    assert!(asset.resolution.contributor_ids.contains(&overlay.id));
    source
}

#[test]
fn grok_skill_overlay_drift_rejects_plan_with_unchanged_asset_and_inspection_revision() {
    let temporary = tempfile::tempdir().unwrap();
    let inspector = Arc::new(inspector(temporary.path()));
    let manifest = inspector.home.join(".grok/skills/review-helper/SKILL.md");
    let config = inspector.home.join(".grok/config.toml");
    fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    fs::write(&manifest, MANIFEST).unwrap();
    fs::write(&config, INITIAL_CONFIG).unwrap();

    let before = inspector.inspect().unwrap();
    let original = skill(&before.inventory);
    let original_control = control_source(&before.inventory, original, &config, &[SKILL]);
    let inspection = before
        .inventory
        .sources
        .iter()
        .find(|source| source.id == original.inspection_source_id)
        .unwrap();
    assert_eq!(Path::new(&inspection.path), manifest);
    assert_eq!(original.revision.identity, inspection.revision.identity);
    assert_ne!(original.inspection_source_id, original_control.id);
    let request = AgentAssetPlanRequest {
        asset_id: original.stable_id.clone(),
        action: AgentAssetActionKind::Enable,
        workspace: None,
        expected_revision: original.revision.identity.clone(),
        installation_id: Some(before.inventory.installations[0].id.clone()),
    };
    let (original_prepared, _) = prepare_request(inspector.as_ref(), &before, &request).unwrap();
    assert!(matches!(
        &original_prepared.execution,
        MutationExecution::AtomicFile { .. }
    ));
    let service = MutationService::default();
    let actor = "grok-overlay-plan-fixture";
    let plan = service
        .plan(actor, request.clone(), inspector.clone())
        .unwrap();
    assert_eq!(plan.source_ids, vec![original_control.id.clone()]);

    // The target remains disabled and Enable stays available. Only another
    // disabled name is added; an old replacement would wrongly erase that edit.
    fs::write(&config, CHANGED_CONFIG).unwrap();
    let changed = inspector.inspect().unwrap();
    let current = skill(&changed.inventory);
    let current_control = control_source(
        &changed.inventory,
        current,
        &config,
        &[SKILL, "unrelated-skill"],
    );
    assert_eq!(current.stable_id, original.stable_id);
    assert_eq!(current.context_id, original.context_id);
    assert_eq!(current.inspection_source_id, original.inspection_source_id);
    assert_eq!(current.revision.identity, original.revision.identity);
    assert_eq!(current_control.id, original_control.id);
    assert_ne!(
        current_control.revision.identity,
        original_control.revision.identity
    );
    assert_eq!(fs::read(&manifest).unwrap(), MANIFEST.as_bytes());

    // Re-preparing the original request succeeds: neither action unavailability
    // nor an inspection revision mismatch can explain the service conflict.
    let (current_prepared, _) = prepare_request(inspector.as_ref(), &changed, &request).unwrap();
    assert!(matches!(
        &current_prepared.execution,
        MutationExecution::AtomicFile { .. }
    ));
    assert_ne!(current_prepared.signature, original_prepared.signature);
    let operation = service
        .start(
            actor,
            &AgentAssetApplyRequest {
                plan_token: plan.token,
                asset_id: original.stable_id.clone(),
                action: AgentAssetActionKind::Enable,
            },
        )
        .unwrap();
    service.run_operation(&operation.id);
    let completed = service.operation(actor, &operation.id).unwrap();
    assert_eq!(completed.phase, AgentAssetOperationPhase::Completed);
    assert_eq!(
        completed.outcome,
        Some(AgentAssetOperationOutcome::UnchangedConflict)
    );
    assert_eq!(fs::read(&config).unwrap(), CHANGED_CONFIG.as_bytes());
    assert_eq!(fs::read(&manifest).unwrap(), MANIFEST.as_bytes());
    let final_snapshot = inspector.inspect().unwrap();
    let final_asset = skill(&final_snapshot.inventory);
    assert_eq!(final_asset.stable_id, original.stable_id);
    assert_eq!(final_asset.revision.identity, original.revision.identity);
}
