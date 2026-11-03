//! Production catalog CRUD producer. Independent CLI inspection consumes the
//! immutable stage homes; this module never certifies native behavior itself.
use super::{
    hook_tests::{
        assert_verified, binding_for, plan, primary_target, run, update_command, write_json,
    },
    native_acceptance::copy_stage_directory,
    tests::FixtureInspector,
    *,
};
use crate::services::agent_cli;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Inputs {
    fixture_root: PathBuf,
    cli_paths: BTreeMap<AgentCliKind, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    revision: u32,
    fixture_digest: String,
    fixtures: Vec<NativeFixture>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeFixture {
    agent_kind: AgentCliKind,
    schema_version: String,
    version: String,
    source_role: String,
    scope: AgentAssetScope,
    stages: Vec<Stage>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Stage {
    action: &'static str,
    home: PathBuf,
    workspace: Option<PathBuf>,
    /// Exact path prefix still used inside unchanged copied native state keys.
    original_home: PathBuf,
    original_workspace: Option<PathBuf>,
    expected: Vec<ExpectedHook>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedHook {
    event: String,
    command: String,
    configured: bool,
    /// None means that only native definition membership is under acceptance.
    enabled: Option<bool>,
}

struct StageSource<'a> {
    root: &'a Path,
    home: &'a Path,
    kind: AgentCliKind,
    event: &'a str,
    neighbor: &'a str,
}

impl StageSource<'_> {
    fn snapshot(
        &self,
        action: &'static str,
        command: &str,
        configured: bool,
        enabled: Option<bool>,
    ) -> Stage {
        let directory = self.root.join(format!("{}-{action}", self.kind.key()));
        fs::create_dir(&directory).unwrap();
        let snapshot_home = directory.join("home");
        copy_stage_directory(self.home, &snapshot_home);
        Stage {
            action,
            home: snapshot_home,
            workspace: None,
            original_home: self.home.to_path_buf(),
            original_workspace: None,
            expected: vec![
                ExpectedHook {
                    event: self.event.to_owned(),
                    command: command.to_owned(),
                    configured,
                    enabled,
                },
                ExpectedHook {
                    event: self.event.to_owned(),
                    command: self.neighbor.to_owned(),
                    configured: true,
                    enabled: enabled.map(|_| true),
                },
            ],
        }
    }
}

#[test]
#[ignore = "requires explicit synthetic fixture roots, exact CLIs and an external deny-network sandbox"]
fn production_catalog_hook_crud_report() {
    assert_eq!(
        std::env::var("BALANCEHUB_HOOK_ACCEPTANCE_SANDBOX").as_deref(),
        Ok("1")
    );
    let inputs: Inputs = serde_json::from_slice(
        &fs::read(
            std::env::var_os("BALANCEHUB_HOOK_ACCEPTANCE_FIXTURE")
                .expect("explicit Hook fixture manifest"),
        )
        .unwrap(),
    )
    .unwrap();
    let root = inputs.fixture_root.canonicalize().unwrap();
    let report_root = PathBuf::from(
        std::env::var_os("BALANCEHUB_HOOK_REPORT_ROOT").expect("explicit report root"),
    )
    .canonicalize()
    .unwrap();
    let temporary = std::env::temp_dir().canonicalize().unwrap();
    assert!(root.starts_with(&temporary) && root != temporary);
    assert!(report_root.starts_with(&root));
    let output_path = report_root.join("hook-crud-report.json");
    assert!(
        !output_path.exists(),
        "never replace an earlier acceptance report"
    );
    let work = root.join("hook-crud-work");
    let stages_root = root.join("hook-crud-stages");
    fs::create_dir(&work).unwrap();
    fs::create_dir(&stages_root).unwrap();
    let fixture: Value = serde_json::from_str(include_str!("native/hook-fixture.json")).unwrap();
    let command = fixture["command"].as_str().unwrap();
    let updated = fixture["updatedCommand"].as_str().unwrap();
    let neighbor = fixture["unrelatedCommand"].as_str().unwrap();
    let mut report = Report {
        revision: 1,
        fixture_digest: native::hook_evidence::fixture_digest(),
        fixtures: Vec::new(),
    };
    native::hook_evidence::with_isolated_fixture(&root, || {
        for (kind, directory, event_key, filename, schema) in [
            (AgentCliKind::ClaudeCode, ".claude", "claudeEvent", "settings.json", "claude-hook-2.1.270"),
            (AgentCliKind::Codex, ".codex", "codexEvent", "hooks.json", "codex-hook-rust-v0.154.0"),
            (AgentCliKind::Gemini, ".gemini", "geminiEvent", "settings.json", "gemini-hooks-0.59.0"),
            (AgentCliKind::Grok, ".grok", "grokEvent", "hooks/hooks.json", "grok-hook-72a61251fcffb464bcc687aeb5a998e5a98ec0c9"),
        ] {
            let fixture_root = work.join(kind.key());
            fs::create_dir(&fixture_root).unwrap();
            let home = fixture_root.join("home");
            fs::create_dir(&home).unwrap();
            for directory in [".claude", ".codex", ".gemini", ".grok"] { fs::create_dir(home.join(directory)).unwrap(); }
            if kind == AgentCliKind::Codex {
                fs::write(home.join(".codex/config.toml"), "[features]\nhooks = true\n").unwrap();
            }
            if kind == AgentCliKind::Grok {
                fs::write(home.join(".grok/config.toml"), "[auth]\npreferred_method = \"api_key\"\n").unwrap();
            }
            let event = fixture[event_key].as_str().unwrap();
            let matcher = if kind == AgentCliKind::Codex { ".*" } else { "*" };
            let stages = StageSource { root: &stages_root, home: &home, kind, event, neighbor };
            let document = home.join(directory).join(filename);
            write_json(&document, &json!({"hooks":{(event):[{"matcher":matcher,"hooks":[{"type":"command","command":neighbor}]}]}}));
            let mut settings = AppSettings::default();
            settings.set_agent_cli_path(kind, inputs.cli_paths.get(&kind).expect("explicit installation for each Agent").clone());
            let inspector = Arc::new(FixtureInspector { home: home.clone(), settings, discover_installations: true });
            let service = CatalogService::new(fixture_root.join("library"), Arc::new(MutationService::default()));
            let target_id = primary_target(&service, inspector.as_ref(), kind);
            let initial = inspector.inspect().unwrap();
            let target = hook_targets::resolve(&initial, &target_id, None).unwrap();
            let installed = initial.inventory.installations.iter().find(|installation| installation.agent_kind == kind
                && installation.availability == AgentInstallationAvailability::Available
                && target.context.compatible_installation_ids.contains(&installation.id)).expect("fixture CLI must be available");
            let version = installed.installed_version.clone().expect("fixture CLI must report its exact version");
            assert!(!version.starts_with("source:"), "source pins cannot certify installed binaries");
            let mut record = NativeFixture { agent_kind: kind, schema_version: schema.to_owned(), version,
                source_role: target.destination.role.clone(), scope: AgentAssetScope::User, stages: Vec::new() };
            let saved = service.save(AgentCatalogSaveRequest { asset_id: None, expected_version: None,
                name: format!("user-{}-hook-fixture", kind.key()), category: AgentAssetCategory::Hook,
                mcp: None, skill_markdown: None, hook: Some(AgentCatalogHookInput { variants: vec![AgentCatalogHookVariantInput {
                    agent_kind: kind, event: event.to_owned(), group_json: json!({"matcher":matcher,"hooks":[{"type":"command","command":command}]}).to_string(),
                }] }),
            }).unwrap();
            assert_verified(&run(&service, plan(&service, inspector.clone(), &saved.asset_id, AgentCatalogAction::ApplyDefinition, vec![target_id.clone()])));
            record.stages.push(stages.snapshot("created", command, true, Some(true)));
            let saved = update_command(&service, &saved, kind, updated);
            assert_verified(&run(&service, plan(&service, inspector.clone(), &saved.asset_id, AgentCatalogAction::ApplyDefinition, vec![target_id])));
            record.stages.push(stages.snapshot("updated", updated, true, Some(true)));
            {
                let (_, binding) = binding_for(&service, inspector.as_ref(), kind, updated);
                assert_verified(&run(&service, plan(&service, inspector.clone(), &saved.asset_id, AgentCatalogAction::Disable, vec![binding.id])));
                let suspended = agent_cli::definition(kind).environment.hook_adapter().unwrap().switch_mode == native::hooks::HookNativeSwitchMode::Suspend;
                record.stages.push(stages.snapshot("disabled", updated, !suspended, Some(false)));
                let enable_id = if suspended {
                    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
                    catalog.assets.iter().find(|item| item.id == saved.asset_id).unwrap().unresolved_targets[0].target_id.clone()
                } else { binding_for(&service, inspector.as_ref(), kind, updated).1.id };
                assert_verified(&run(&service, plan(&service, inspector.clone(), &saved.asset_id, AgentCatalogAction::Enable, vec![enable_id])));
                record.stages.push(stages.snapshot("enabled", updated, true, Some(true)));
            }
            let (_, binding) = binding_for(&service, inspector.as_ref(), kind, updated);
            assert_verified(&run(&service, plan(&service, inspector.clone(), &saved.asset_id, AgentCatalogAction::RemoveBinding, vec![binding.id])));
            record.stages.push(stages.snapshot("removed", updated, false, Some(false)));
            assert_eq!(service.definition(&saved.asset_id).unwrap().version, 2);
            report.fixtures.push(record);
        }
    }).unwrap();
    assert_eq!(report.fixtures.len(), 4);
    fs::write(output_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
