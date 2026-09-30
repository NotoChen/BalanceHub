//! Production budgets through native discovery and isolated real snapshots.
use super::super::{
    inventory::production_limits, run::MonotonicClock, snapshot::revision_for_missing,
};
use super::*;
use crate::models::{AgentEnvironmentInventory, AppSettings};
use std::time::{Duration, Instant};

const KINDS: [AgentCliKind; 4] = [
    AgentCliKind::Codex,
    AgentCliKind::ClaudeCode,
    AgentCliKind::Gemini,
    AgentCliKind::Grok,
];
const ONE_HOOK: &str = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"bh-production-budget-fixture"}]}]}}"#;

#[derive(Debug)]
struct SnapshotAttempt {
    path: PathBuf,
    elapsed_ms: u64,
}

struct IsolatedSnapshots<'a> {
    home: &'a Path,
    real: RealSnapshotPort,
    clock: Arc<ManualClock>,
    started_at: Instant,
    attempts: Mutex<Vec<SnapshotAttempt>>,
}

impl SnapshotPort for IsolatedSnapshots<'_> {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        self.attempts.lock().unwrap().push(SnapshotAttempt {
            path: request.source.path.clone(),
            elapsed_ms: self.clock.now().duration_since(self.started_at).as_millis() as u64,
        });
        // All external/system locations are synthetic Missing observations.
        // Native discovery never gets a port that can read the real HOME.
        if !request.source.path.starts_with(self.home) {
            return AgentAssetSnapshot::Missing {
                revision: revision_for_missing(&request.source.path),
            };
        }
        self.real.snapshot(request, run)
    }

    fn access_anchor(
        &self,
        revision: &AgentAssetRevision,
    ) -> Option<super::super::verified_path::VerifiedPathAnchor> {
        self.real.access_anchor(revision)
    }
}

struct Fixture {
    _temporary: tempfile::TempDir,
    home: PathBuf,
}

struct Scan {
    inventory: AgentEnvironmentInventory,
    attempts: Vec<SnapshotAttempt>,
    installation_calls: Vec<String>,
    probe: Arc<StageCheckpointProbe>,
}

impl Fixture {
    fn new(skill_count: usize) -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let fixture = Self {
            home: temporary.path().canonicalize().unwrap(),
            _temporary: temporary,
        };
        for index in 0..skill_count {
            let name = format!("budget-skill-{index:03}");
            fixture.write(
                &format!(".codex/skills/{name}/SKILL.md"),
                &format!(
                    "---\nname: {name}\ndescription: Synthetic budget fixture.\n---\nFixture.\n"
                ),
            );
        }
        fixture.write(".codex/config.toml", "[features]\nhooks = true\n");
        for path in [
            ".codex/hooks.json",
            ".claude/settings.json",
            ".gemini/settings.json",
            ".grok/hooks/later.json",
        ] {
            fixture.write(path, ONE_HOOK);
        }
        fixture.write(
            ".grok/skills/later/SKILL.md",
            "---\nname: grok-later-budget-skill\ndescription: Later Agent fixture.\n---\nFixture.\n",
        );
        fixture
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.home.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn scan(&self, limits: AgentAssetLimits, advance_ms: u64) -> Scan {
        let clock = Arc::new(ManualClock::new());
        let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
        probe.advance_on(
            AgentInventoryStage::Parse,
            1,
            Duration::from_millis(advance_ms),
        );
        let snapshots = IsolatedSnapshots {
            home: &self.home,
            real: RealSnapshotPort::default(),
            started_at: clock.now(),
            clock: clock.clone(),
            attempts: Mutex::new(Vec::new()),
        };
        let (installations, installation_state) = FakeInstallationPort::new(Vec::new());
        let settings = AppSettings::default();
        let definitions = KINDS.map(|kind| {
            let native = *definition(kind);
            AgentCliDefinition {
                environment: native.environment().with_test_contexts(fixture_contexts),
                ..native
            }
        });
        let inventory = build_inventory_with(
            InventoryInput {
                home: &self.home,
                workspace: None,
                settings: Some(&settings),
            },
            InventoryPipelineDeps {
                definitions: &definitions,
                limits,
                clock,
                installations: &installations,
                snapshots: &snapshots,
                checkpoint_probe: Some(probe.clone()),
            },
        )
        .unwrap();
        let installation_calls = installation_state.lock().unwrap().calls.clone();
        Scan {
            inventory,
            attempts: snapshots.attempts.into_inner().unwrap(),
            installation_calls,
            probe,
        }
    }
}

fn fixture_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Vec<crate::models::AgentConfigurationContext> {
    let mut contexts = definition(request.agent_kind)
        .environment()
        .discover_contexts(request, output);
    let directory = match request.agent_kind {
        AgentCliKind::Codex => ".codex",
        AgentCliKind::ClaudeCode => ".claude",
        AgentCliKind::Gemini => ".gemini",
        AgentCliKind::Grok => ".grok",
    };
    for context in &mut contexts {
        context.config_root = request.home.join(directory).to_string_lossy().into_owned();
    }
    contexts
}

fn hook_count(inventory: &AgentEnvironmentInventory, kind: AgentCliKind) -> Option<u32> {
    let rows = inventory
        .hook_rule_counts
        .iter()
        .filter(|row| row.agent_kind == kind)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    rows[0].rule_count
}

#[test]
fn production_profile_finishes_rich_native_inventory_beyond_old_source_and_time_budgets() {
    let fixture = Fixture::new(150);
    let scan = fixture.scan(production_limits(), 6_000);
    let inventory = &scan.inventory;
    let codex = inventory
        .contexts
        .iter()
        .find(|context| context.agent_kind == AgentCliKind::Codex)
        .unwrap();
    let source_count = inventory
        .sources
        .iter()
        .filter(|source| source.context_id == codex.id)
        .count();
    assert!(source_count > AgentAssetLimits::DEFAULT.sources_per_context);
    assert!(source_count < AgentAssetLimits::HARD_CAP.sources_per_context);
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Skill
                && asset.label.starts_with("budget-skill-"))
            .count(),
        150
    );
    assert!(inventory.assets.iter().any(|asset| {
        asset.category == AgentAssetCategory::Skill && asset.label == "grok-later-budget-skill"
    }));
    assert_eq!(
        scan.installation_calls,
        KINDS.map(|kind| kind.key().to_owned())
    );
    for kind in KINDS {
        assert_eq!(hook_count(inventory, kind), Some(1), "{kind:?}");
    }
    let later_hook = fixture.home.join(".grok/hooks/later.json");
    let later_read = scan
        .attempts
        .iter()
        .position(|attempt| attempt.path == later_hook)
        .unwrap();
    assert!(later_read > AgentAssetLimits::DEFAULT.sources_per_context);
    assert_eq!(scan.attempts[later_read].elapsed_ms, 6_000);
    assert!(!inventory.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::BudgetExceeded { .. }
            | AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::SourcesPerContext,
                ..
            }
    )));
    let mut unchanged = inventory.limits.clone();
    assert_eq!(
        unchanged.refresh_budget_ms,
        AgentAssetLimits::HARD_CAP.refresh_budget_ms
    );
    assert_eq!(
        unchanged.sources_per_context,
        AgentAssetLimits::HARD_CAP.sources_per_context
    );
    unchanged.refresh_budget_ms = AgentAssetLimits::DEFAULT.refresh_budget_ms;
    unchanged.sources_per_context = AgentAssetLimits::DEFAULT.sources_per_context;
    assert_eq!(unchanged, AgentAssetLimits::defaults());

    let old = fixture.scan(AgentAssetLimits::DEFAULT, 6_000);
    assert!(old.inventory.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::BudgetExceeded { budget_ms, .. }
            if *budget_ms == AgentAssetLimits::DEFAULT.refresh_budget_ms
    )));
    assert_eq!(hook_count(&old.inventory, AgentCliKind::Codex), None);
    assert!(!old
        .installation_calls
        .contains(&AgentCliKind::Grok.key().to_owned()));
}

#[test]
fn production_profile_keeps_the_source_hard_cap_and_unknown_counts() {
    let fixture = Fixture::new(512);
    let scan = fixture.scan(production_limits(), 0);
    let codex = scan
        .inventory
        .contexts
        .iter()
        .find(|context| context.agent_kind == AgentCliKind::Codex)
        .unwrap();
    let sources = scan
        .inventory
        .sources
        .iter()
        .filter(|source| source.context_id == codex.id)
        .collect::<Vec<_>>();
    assert_eq!(
        sources.len(),
        AgentAssetLimits::HARD_CAP.sources_per_context
    );
    assert!(scan.inventory.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::SourcesPerContext,
            accepted: 512,
            observed_at_least,
        } if *observed_at_least >= 513
    )));
    let codex_root = fixture.home.join(".codex");
    let attempts = scan
        .attempts
        .iter()
        .filter(|attempt| attempt.path.starts_with(&codex_root))
        .collect::<Vec<_>>();
    assert!(!attempts.is_empty());
    assert!(attempts.len() <= AgentAssetLimits::HARD_CAP.sources_per_context);
    assert!(attempts.iter().all(|attempt| sources
        .iter()
        .any(|source| Path::new(&source.path) == attempt.path.as_path())));
    assert!(!attempts.iter().any(|attempt| {
        attempt.path == fixture.home.join(".codex/skills/budget-skill-511/SKILL.md")
    }));
    assert_eq!(hook_count(&scan.inventory, AgentCliKind::Codex), None);
}

#[test]
fn production_profile_stops_at_the_time_hard_cap_without_later_reads_or_projection() {
    let fixture = Fixture::new(150);
    let scan = fixture.scan(
        production_limits(),
        AgentAssetLimits::HARD_CAP.refresh_budget_ms,
    );
    assert!(scan.inventory.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::BudgetExceeded { budget_ms, .. }
            if *budget_ms == AgentAssetLimits::HARD_CAP.refresh_budget_ms
    )));
    assert!(!scan.attempts.is_empty());
    assert!(scan
        .attempts
        .iter()
        .all(|attempt| { attempt.elapsed_ms < AgentAssetLimits::HARD_CAP.refresh_budget_ms }));
    assert_eq!(scan.installation_calls, [AgentCliKind::Codex.key()]);
    assert!(!scan.probe.events().iter().any(|event| matches!(
        event,
        InventoryPipelineEvent::Parse { .. }
            | InventoryPipelineEvent::Resolve { .. }
            | InventoryPipelineEvent::Project { .. }
    )));
    assert!(scan.inventory.assets.is_empty());
    assert_eq!(hook_count(&scan.inventory, AgentCliKind::Codex), None);
}
