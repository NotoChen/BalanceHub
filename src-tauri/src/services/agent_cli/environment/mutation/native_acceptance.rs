//! Explicit local acceptance. Run the compiled test binary inside a macOS
//! sandbox denying network and every write outside the synthetic fixture root.

use super::super::inventory::RealInstallationDiscoveryPort;
use super::{native_test_support::NativeFixtureInspector, MutationInspector, MutationService};
use crate::{models::*, services::agent_cli::definition};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcceptanceFixture {
    fixture_root: PathBuf,
    home: PathBuf,
    workspace: PathBuf,
    cli_paths: BTreeMap<AgentCliKind, String>,
}

#[test]
#[ignore = "requires synthetic fixtures, exact local CLIs and an external network-denying macOS sandbox"]
fn production_native_mutation_acceptance_eight_cells() {
    assert_eq!(
        std::env::var("BALANCEHUB_NATIVE_ACCEPTANCE_SANDBOX").as_deref(),
        Ok("1")
    );
    let manifest = PathBuf::from(
        std::env::var_os("BALANCEHUB_NATIVE_ACCEPTANCE_FIXTURE").expect("fixture manifest path"),
    );
    let fixture: AcceptanceFixture = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    let root = fixture.fixture_root.canonicalize().unwrap();
    let home = fixture.home.canonicalize().unwrap();
    let workspace = fixture.workspace.canonicalize().unwrap();
    assert!(home.starts_with(&root) && workspace.starts_with(&root));
    assert_ne!(home, root);
    let mut settings = AppSettings::default();
    for (&kind, path) in &fixture.cli_paths {
        settings.set_agent_cli_path(kind, path.clone());
    }
    let cells = [
        (AgentCliKind::Codex, AgentAssetCategory::Mcp),
        (AgentCliKind::Codex, AgentAssetCategory::Skill),
        (AgentCliKind::Codex, AgentAssetCategory::Plugin),
        (AgentCliKind::Gemini, AgentAssetCategory::Extension),
        (AgentCliKind::Gemini, AgentAssetCategory::Skill),
        (AgentCliKind::ClaudeCode, AgentAssetCategory::Plugin),
        (AgentCliKind::Grok, AgentAssetCategory::Mcp),
        (AgentCliKind::Grok, AgentAssetCategory::Skill),
    ];
    let service = MutationService::default();
    let actor = "explicit-native-acceptance";
    let mut results = Vec::new();
    for (kind, category) in cells {
        let inspector = Arc::new(NativeFixtureInspector {
            home: home.clone(),
            workspace: None,
            settings: settings.clone(),
            definitions: vec![*definition(kind)],
            installations: Arc::new(RealInstallationDiscoveryPort),
        });
        let initial = inspector.inspect().unwrap();
        let candidates = initial
            .inventory
            .assets
            .iter()
            .filter(|asset| {
                asset.category == category
                    && asset.relationships.provided_by.is_none()
                    && asset.resolution.relation != AgentAssetResolutionRelation::Replaced
                    && asset.actions.iter().any(|entry| {
                        entry.action == AgentAssetActionKind::Disable && entry.available
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            candidates.len(),
            1,
            "{kind:?}/{category:?}: {:?}",
            initial
                .inventory
                .assets
                .iter()
                .filter(|asset| asset.category == category)
                .map(|asset| (
                    &asset.native_id,
                    asset.effective_state,
                    &asset.actions,
                    &asset.diagnostics
                ))
                .collect::<Vec<_>>()
        );
        let selected_id = candidates[0].stable_id.clone();
        for action in [AgentAssetActionKind::Disable, AgentAssetActionKind::Enable] {
            let snapshot = inspector.inspect().unwrap();
            let asset = snapshot
                .inventory
                .assets
                .iter()
                .find(|asset| asset.stable_id == selected_id)
                .expect("the selected asset must survive both native operations");
            let expected_before = if action == AgentAssetActionKind::Disable {
                AgentAssetState::Enabled
            } else {
                AgentAssetState::Disabled
            };
            assert_eq!(
                asset.effective_state, expected_before,
                "{kind:?}/{category:?}/{action:?} must perform a state transition"
            );
            assert!(asset
                .actions
                .iter()
                .any(|entry| entry.action == action && entry.available));
            eprintln!(
                "production native acceptance: {kind:?}/{category:?}/{action:?} {}",
                asset.native_id
            );
            let plan = service
                .plan(
                    actor,
                    AgentAssetPlanRequest {
                        asset_id: asset.stable_id.clone(),
                        action,
                        workspace: None,
                        expected_revision: asset.revision.identity.clone(),
                        installation_id: asset.selected_action_installation_id.clone(),
                    },
                    inspector.clone(),
                )
                .unwrap();
            assert!(!plan.source_ids.is_empty());
            assert!(plan.affected_asset_ids.contains(&asset.stable_id));
            let operation = service
                .start(
                    actor,
                    &AgentAssetApplyRequest {
                        plan_token: plan.token,
                        asset_id: asset.stable_id.clone(),
                        action,
                    },
                )
                .unwrap();
            assert_eq!(operation.phase, AgentAssetOperationPhase::Preparing);
            assert!(operation.can_cancel);
            service.run_operation(&operation.id);
            let completed = service.operation(actor, &operation.id).unwrap();
            assert_eq!(completed.phase, AgentAssetOperationPhase::Completed);
            assert_eq!(
                completed.outcome,
                Some(AgentAssetOperationOutcome::AppliedVerified),
                "{kind:?}/{category:?}/{action:?}: {completed:?}"
            );
            let after = inspector.inspect().unwrap();
            let observed = after
                .inventory
                .assets
                .iter()
                .find(|current| current.stable_id == asset.stable_id)
                .unwrap();
            assert_eq!(
                observed.effective_state,
                if action == AgentAssetActionKind::Enable {
                    AgentAssetState::Enabled
                } else {
                    AgentAssetState::Disabled
                }
            );
            results.push(serde_json::json!({
                "agent": kind, "category": category, "action": action,
                "assetId": selected_id, "beforeState": asset.effective_state, "afterState": observed.effective_state,
                "outcome": completed.outcome, "affectedCount": plan.affected_asset_ids.len(),
                "mechanismId": plan.mechanism_id,
            }));
        }
    }
    assert_eq!(results.len(), 16);
    fs::write(
        root.join("production-mutation-acceptance.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
}
