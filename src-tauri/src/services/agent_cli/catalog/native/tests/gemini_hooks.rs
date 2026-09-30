//! Production Gemini inventory and native Hook preparation over synthetic files.
//! System/Managed settings are explicitly absent fixture sources; host policy
//! files and installed Agent configuration are never inspected by these tests.

use crate::{
    models::*,
    services::agent_cli::{
        self,
        catalog::native::hooks::*,
        config_support::parse_jsonc_document,
        contracts::*,
        environment::{
            mutation::{native_test_support::catalog_fixture_inventory, MutationInventory},
            source_stable_id,
        },
        AgentCliDefinition,
    },
};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    ops::ControlFlow,
    path::{Path, PathBuf},
};

fn adapter() -> &'static NativeHookAdapter {
    agent_cli::definition(AgentCliKind::Gemini)
        .environment
        .hook_adapter()
        .unwrap()
}

struct LocalSources<'a>(&'a mut dyn InitialSourceOutput);

impl AgentDiagnosticOutput for LocalSources<'_> {
    fn has_regular_capacity(&self) -> bool {
        self.0.has_regular_capacity()
    }

    fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.0.emit_diagnostic(value)
    }
}

impl InitialSourceOutput for LocalSources<'_> {
    fn emit_initial(&mut self, source: AgentAssetSourceSpec) -> ControlFlow<AgentOutputStop> {
        if matches!(
            source.scope,
            AgentAssetScope::System | AgentAssetScope::Managed
        ) {
            ControlFlow::Continue(())
        } else {
            self.0.emit_initial(source)
        }
    }
}

#[derive(Default)]
struct SourceList(Vec<AgentAssetSourceSpec>);

impl AgentDiagnosticOutput for SourceList {
    fn has_regular_capacity(&self) -> bool {
        true
    }

    fn emit_diagnostic(&mut self, _value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        AgentDiagnosticEmission::Accepted
    }
}

impl InitialSourceOutput for SourceList {
    fn emit_initial(&mut self, source: AgentAssetSourceSpec) -> ControlFlow<AgentOutputStop> {
        self.0.push(source);
        ControlFlow::Continue(())
    }
}

fn fixture_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Vec<AgentConfigurationContext> {
    let mut contexts = agent_cli::definition(AgentCliKind::Gemini)
        .environment
        .discover_contexts(request, output);
    for context in &mut contexts {
        context.config_root = request.home.join(".gemini").to_string_lossy().into_owned();
    }
    contexts
}

fn fixture_sources(request: AgentSourceDiscoveryRequest<'_>, output: &mut dyn InitialSourceOutput) {
    agent_cli::definition(AgentCliKind::Gemini)
        .environment
        .discover_sources(request, &mut LocalSources(output));
}

fn fixture_followups(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    agent_cli::definition(AgentCliKind::Gemini)
        .environment
        .discover_follow_up_sources(request, output);
}

fn fixture_parse(request: AgentAssetParseRequest<'_>, output: &mut dyn AgentParseOutput) {
    agent_cli::definition(AgentCliKind::Gemini)
        .environment
        .parse(request, output);
}

fn fixture_resolve(request: AgentAssetResolveRequest<'_>, output: &mut dyn AgentResolveOutput) {
    agent_cli::definition(AgentCliKind::Gemini)
        .environment
        .resolve(request, output);
}

fn fixture_trust_sources(
    request: AgentWorkspaceTrustSourceRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    agent_cli::definition(AgentCliKind::Gemini)
        .environment
        .discover_workspace_trust_sources(request, output);
}

fn fixture_trust(
    request: AgentWorkspaceTrustResolveRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> AgentTrustState {
    agent_cli::definition(AgentCliKind::Gemini)
        .environment
        .resolve_workspace_trust(request, output)
}

fn fixture_definition() -> AgentCliDefinition {
    let native = agent_cli::definition(AgentCliKind::Gemini);
    AgentCliDefinition {
        environment: EnvironmentAdapter::with_pipeline(
            fixture_contexts,
            fixture_sources,
            Some(fixture_followups),
            "@google/gemini-cli",
            fixture_parse,
            fixture_resolve,
            native.environment.state_assessor(),
        )
        .with_workspace_trust_authority(fixture_trust_sources, fixture_trust)
        .with_hook_adapter(adapter()),
        ..*native
    }
}

struct Fixture {
    temporary: tempfile::TempDir,
    home: PathBuf,
    workspace: PathBuf,
}

impl Fixture {
    fn new(settings: &str, workspace_settings: &str) -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let home = root.join("home");
        let workspace = root.join("project");
        fs::create_dir_all(home.join(".gemini")).unwrap();
        fs::create_dir_all(workspace.join(".gemini")).unwrap();
        fs::write(home.join(".gemini/settings.json"), settings).unwrap();
        fs::write(workspace.join(".gemini/settings.json"), workspace_settings).unwrap();
        fs::write(
            home.join(".gemini/trustedFolders.json"),
            serde_json::to_vec(&json!({ (workspace.to_string_lossy().as_ref()): "TRUST_FOLDER" }))
                .unwrap(),
        )
        .unwrap();
        Self {
            temporary,
            home,
            workspace,
        }
    }

    fn extension(&self, hooks: &str) -> PathBuf {
        let root = self.home.join(".gemini/extensions/fixture-provider");
        fs::create_dir_all(root.join("hooks")).unwrap();
        fs::write(
            root.join("gemini-extension.json"),
            br#"{"name":"fixture-provider","version":"1.0.0"}"#,
        )
        .unwrap();
        let path = root.join("hooks/hooks.json");
        fs::write(&path, hooks).unwrap();
        path
    }

    fn inspect(&self) -> MutationInventory {
        let mut snapshot = catalog_fixture_inventory(
            &self.home,
            Some(&self.workspace),
            None,
            &[fixture_definition()],
        )
        .unwrap();
        assert_eq!(snapshot.inventory.contexts.len(), 1);
        let context = snapshot.inventory.contexts[0].clone();
        assert_eq!(context.trust_context, AgentTrustState::Trusted);
        let mut native_sources = SourceList::default();
        agent_cli::definition(AgentCliKind::Gemini)
            .environment
            .discover_sources(
                AgentSourceDiscoveryRequest {
                    context: &context,
                    home: &self.home,
                    workspace: Some(&self.workspace),
                    installations: &[],
                },
                &mut native_sources,
            );
        // Complete the fixture's absent native scopes without opening host paths.
        // No authorizing write anchors are manufactured for these readonly files.
        for spec in native_sources.0.into_iter().filter(|source| {
            matches!(
                source.scope,
                AgentAssetScope::System | AgentAssetScope::Managed
            )
        }) {
            snapshot.inventory.sources.push(AgentAssetSource {
                id: source_stable_id(&context, &spec.path),
                context_id: context.id.clone(),
                environment_id: context.environment_id.clone(),
                workspace_id: context.workspace_id.clone(),
                label: spec.label,
                scope: spec.scope,
                origin: spec.origin,
                path: spec.path.to_string_lossy().into_owned(),
                allowed_root: spec.allowed_root.to_string_lossy().into_owned(),
                precedence: spec.precedence,
                writable: false,
                sensitive: spec.sensitive,
                source_kind: spec.source_kind,
                categories: spec.categories,
                revision: AgentAssetRevision {
                    is_missing: true,
                    ..Default::default()
                },
                diagnostics: Vec::new(),
                access: Default::default(),
                actions: Vec::new(),
            });
        }
        snapshot
    }

    fn user_path(&self) -> PathBuf {
        self.home.join(".gemini/settings.json")
    }

    fn write_prepared(&self, snapshot: &MutationInventory, prepared: &HookNativePrepared) {
        let root = self.temporary.path().canonicalize().unwrap();
        let mut written = BTreeSet::new();
        for write in &prepared.writes {
            assert!(written.insert(&write.source_id));
            let source = snapshot
                .inventory
                .sources
                .iter()
                .find(|source| source.id == write.source_id)
                .unwrap();
            let path = Path::new(&source.path);
            assert!(path.starts_with(&root));
            fs::write(path, &write.bytes).unwrap();
        }
    }
}

fn source_id(snapshot: &MutationInventory, path: &Path) -> String {
    snapshot
        .inventory
        .sources
        .iter()
        .find(|source| Path::new(&source.path) == path)
        .unwrap()
        .id
        .clone()
}

fn rule(snapshot: &MutationInventory, path: &Path, native_id: &str) -> HookNativeRule {
    (adapter().read_hook)(snapshot, public_rule(snapshot, path, native_id)).unwrap()
}

fn public_rule<'a>(
    snapshot: &'a MutationInventory,
    path: &Path,
    native_id: &str,
) -> &'a AgentAssetRecord {
    let source = source_id(snapshot, path);
    snapshot
        .inventory
        .assets
        .iter()
        .find(|asset| {
            asset.category == AgentAssetCategory::Hook
                && asset.inspection_source_id == source
                && asset.native_id == native_id
        })
        .unwrap()
}

fn target(snapshot: &MutationInventory, scope: AgentAssetScope) -> HookNativeDestination {
    (adapter().hook_targets)(&snapshot.inventory, &snapshot.inventory.contexts[0])
        .into_iter()
        .find(|destination| destination.scope == scope)
        .unwrap()
}

fn document(path: &Path) -> Value {
    parse_jsonc_document(&fs::read_to_string(path).unwrap()).unwrap()
}

fn command_definition(event: &str, command: &str) -> HookNativeDefinition {
    HookNativeDefinition {
        event: event.to_owned(),
        group: json!({ "hooks": [{ "type": "command", "command": command }] }),
    }
}

#[test]
fn native_enable_clears_every_exact_case_writable_scope_and_preserves_global_toggle() {
    let fixture = Fixture::new(
        r#"{
          // user policy comment
          "hooksConfig": {"enabled": false, "disabled": ["shared", "Shared", "shared"]},
          "hooks": {"BeforeTool": [{"matcher":"fixture-private-matcher", "hooks":[
            {"type":"command","name":"shared","command":"fixture-private-command"},
            {"type":"command","name":"Shared","command":"fixture-case-command"}
          ]}]},
          "unknown": {"preserve": [1, 2]}
        }"#,
        r#"{"hooksConfig":{"disabled":["shared","unrelated"]},"hooks":{"AfterTool":[{"hooks":[{"type":"command","name":"shared","command":"fixture-workspace-command"}]}]}}"#,
    );
    let plugin = fixture.extension(r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","name":"shared","command":"fixture-extension-command"}]}]}}"#);
    let plugin_before = fs::read(&plugin).unwrap();
    let snapshot = fixture.inspect();
    assert_eq!(
        snapshot
            .inventory
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Hook)
            .count(),
        4
    );
    let serialized = serde_json::to_string(&snapshot.inventory).unwrap();
    for private in [
        "fixture-private-matcher",
        "fixture-private-command",
        "fixture-workspace-command",
        "fixture-extension-command",
    ] {
        assert!(!serialized.contains(private));
    }
    let original = rule(&snapshot, &fixture.user_path(), "BeforeTool:0:0");
    assert_eq!(original.enabled, AgentAssetDeclaredState::Disabled);
    for (path, native_id) in [
        (fixture.user_path(), "BeforeTool:0:0"),
        (plugin.clone(), "fixture-provider:SessionStart:0:0"),
    ] {
        let row = public_rule(&snapshot, &path, native_id);
        assert_eq!(row.effective_state, AgentAssetState::Blocked);
        let Some(AgentAssetPolicyReference::Declaration { declaration_id }) =
            row.resolution.control_source.as_ref()
        else {
            panic!("global Hook policy must retain its declaration witness");
        };
        assert!(snapshot.inventory.declarations.iter().any(|declaration| {
            declaration.id == *declaration_id && declaration.native_id == "hooksConfig.enabled"
        }));
    }
    (adapter().validate_adoption)(&original).unwrap();
    let prepared = (adapter().prepare_hook_edits)(
        &snapshot,
        &target(&snapshot, AgentAssetScope::User),
        &[HookNativeEdit::SetEnabled {
            original,
            enabled: true,
        }],
    )
    .unwrap();
    assert_eq!(prepared.writes.len(), 2);
    assert_eq!(prepared.affected_asset_ids.len(), 3);
    assert_eq!(prepared.expectations.len(), 3);
    assert!(prepared
        .expectations
        .iter()
        .all(|expected| expected.enabled == Some(AgentAssetDeclaredState::Enabled)));
    assert!(prepared
        .required_capabilities
        .contains(&HookNativeCapability::NativeSwitch));
    assert_eq!(prepared.read_source_ids.len(), 9);
    assert!(prepared.read_source_ids.contains(&source_id(
        &snapshot,
        &fixture.home.join(".gemini/trustedFolders.json")
    )));
    for relative in [
        ".gemini/extensions",
        ".gemini/extensions/fixture-provider/gemini-extension.json",
        ".gemini/extensions/extension-enablement.json",
    ] {
        assert!(prepared
            .read_source_ids
            .contains(&source_id(&snapshot, &fixture.home.join(relative))));
    }
    fixture.write_prepared(&snapshot, &prepared);
    let user = document(&fixture.user_path());
    assert_eq!(user["hooksConfig"]["disabled"], json!(["Shared"]));
    assert_eq!(user["hooksConfig"]["enabled"], false);
    assert_eq!(
        document(&fixture.workspace.join(".gemini/settings.json"))["hooksConfig"]["disabled"],
        json!(["unrelated"])
    );
    assert!(fs::read_to_string(fixture.user_path())
        .unwrap()
        .contains("// user policy comment"));
    assert!(fs::read_to_string(fixture.user_path())
        .unwrap()
        .contains("\"preserve\": [1, 2]"));
    assert_eq!(fs::read(&plugin).unwrap(), plugin_before);
    let after = fixture.inspect();
    for (path, native_id) in [
        (fixture.user_path(), "BeforeTool:0:0"),
        (plugin.clone(), "fixture-provider:SessionStart:0:0"),
    ] {
        let row = public_rule(&after, &path, native_id);
        assert_eq!(row.declared_state, AgentAssetState::Enabled);
        assert_eq!(row.effective_state, AgentAssetState::Blocked);
        assert_eq!(
            row.resolution.terminal,
            Some(AgentAssetResolutionTerminal::PolicyBlocked)
        );
    }
    assert_eq!(
        rule(&after, &fixture.user_path(), "BeforeTool:0:0").enabled,
        AgentAssetDeclaredState::Enabled
    );
    assert_eq!(
        rule(&after, &fixture.user_path(), "BeforeTool:0:1").enabled,
        AgentAssetDeclaredState::Disabled
    );
    assert_eq!(
        rule(&after, &plugin, "fixture-provider:SessionStart:0:0").enabled,
        AgentAssetDeclaredState::Enabled
    );
}

#[test]
fn extension_and_runtime_hooks_use_user_policy_without_editing_readonly_definitions() {
    let fixture = Fixture::new("{}", "{}");
    let plugin = fixture.extension(r#"{"hooks":{"BeforeAgent":[{"hooks":[{"type":"runtime","name":"fixture-runtime"},{"type":"plugin"}]}]}}"#);
    let snapshot = fixture.inspect();
    let before = fs::read(&plugin).unwrap();
    let runtime = rule(&snapshot, &plugin, "fixture-provider:BeforeAgent:0:0");
    assert!((adapter().validate_adoption)(&runtime).is_err());
    let plugin_rule = rule(&snapshot, &plugin, "fixture-provider:BeforeAgent:0:1");
    let prepared = (adapter().prepare_hook_edits)(
        &snapshot,
        &target(&snapshot, AgentAssetScope::User),
        &[
            HookNativeEdit::SetEnabled {
                original: runtime,
                enabled: false,
            },
            HookNativeEdit::SetEnabled {
                original: plugin_rule,
                enabled: false,
            },
        ],
    )
    .unwrap();
    assert_eq!(prepared.writes.len(), 1);
    assert_eq!(prepared.affected_asset_ids.len(), 2);
    assert_eq!(
        prepared.writes[0].source_id,
        source_id(&snapshot, &fixture.user_path())
    );
    fixture.write_prepared(&snapshot, &prepared);
    assert_eq!(
        document(&fixture.user_path())["hooksConfig"]["disabled"],
        json!(["fixture-runtime", "unknown-hook"])
    );
    assert_eq!(fs::read(&plugin).unwrap(), before);
    let after = fixture.inspect();
    assert_eq!(
        rule(&after, &plugin, "fixture-provider:BeforeAgent:0:0").enabled,
        AgentAssetDeclaredState::Disabled
    );
}

#[test]
fn readonly_policy_prevents_false_enable_and_does_not_write_any_file() {
    let fixture = Fixture::new(r#"{"hooksConfig":{"disabled":["shared"]}}"#, "{}");
    let plugin = fixture.extension(r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","name":"shared","command":"fixture-noop"}]}]}}"#);
    let mut snapshot = fixture.inspect();
    let user_id = source_id(&snapshot, &fixture.user_path());
    snapshot
        .inventory
        .sources
        .iter_mut()
        .find(|source| source.id == user_id)
        .unwrap()
        .writable = false;
    let original = rule(&snapshot, &plugin, "fixture-provider:SessionStart:0:0");
    let before = fs::read(fixture.user_path()).unwrap();
    let result = (adapter().prepare_hook_edits)(
        &snapshot,
        &target(&snapshot, AgentAssetScope::Workspace),
        &[HookNativeEdit::SetEnabled {
            original,
            enabled: true,
        }],
    );
    assert!(matches!(result, Err(reason) if reason.contains("只读配置禁用")));
    assert_eq!(fs::read(fixture.user_path()).unwrap(), before);
    assert_eq!(
        document(&fixture.workspace.join(".gemini/settings.json")),
        json!({})
    );
}

#[test]
fn missing_same_identity_inventory_peer_blocks_policy_mutation() {
    let fixture = Fixture::new(
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","name":"fixture-shared","command":"fixture-first"},{"type":"command","name":"fixture-shared","command":"fixture-peer"}]}]}}"#,
        "{}",
    );
    let mut snapshot = fixture.inspect();
    let original = rule(&snapshot, &fixture.user_path(), "SessionStart:0:0");
    let peer = rule(&snapshot, &fixture.user_path(), "SessionStart:0:1");
    snapshot
        .inventory
        .assets
        .retain(|asset| Some(&asset.stable_id) != peer.native_asset_id.as_ref());
    let before = fs::read(fixture.user_path()).unwrap();
    let result = (adapter().prepare_hook_edits)(
        &snapshot,
        &target(&snapshot, AgentAssetScope::User),
        &[HookNativeEdit::SetEnabled {
            original,
            enabled: false,
        }],
    );
    assert!(matches!(result, Err(reason) if reason.contains("无法确认全部影响")));
    assert_eq!(fs::read(fixture.user_path()).unwrap(), before);
}

#[test]
fn incomplete_hook_discovery_blocks_identity_policy_changes() {
    let fixture = Fixture::new(
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"fixture-first"}]}]}}"#,
        "{}",
    );
    let plugin = fixture.extension("{invalid-extension-hooks");
    let snapshot = fixture.inspect();
    let original = rule(&snapshot, &fixture.user_path(), "SessionStart:0:0");
    assert!(snapshot
        .inventory
        .hook_rule_counts
        .iter()
        .find(|count| count.agent_kind == AgentCliKind::Gemini)
        .unwrap()
        .rule_count
        .is_none());
    let before = fs::read(fixture.user_path()).unwrap();
    let result = (adapter().prepare_hook_edits)(
        &snapshot,
        &target(&snapshot, AgentAssetScope::User),
        &[HookNativeEdit::SetEnabled {
            original,
            enabled: false,
        }],
    );
    assert!(matches!(result, Err(reason) if reason.contains("盘点不完整")));
    assert_eq!(fs::read(fixture.user_path()).unwrap(), before);
    assert_eq!(
        fs::read_to_string(plugin).unwrap(),
        "{invalid-extension-hooks"
    );
}

#[test]
fn same_file_multi_node_edits_keep_jsonc_neighbors_metadata_and_unknown_fields() {
    let fixture = Fixture::new(
        r#"{
          "hooks": {"BeforeTool": [{"matcher":".*", "customGroup":{"keep":true}, "hooks":[
            {"type":"command","command":"fixture-first","timeout":1000,"custom":{"retain":true}},
            // untouched middle hook
            {"type":"command","command":"fixture-middle","extra":{"items":[1,2]}},
            {"type":"command","command":"fixture-last"}
          ]}]},
          "mcpServers": {"fixture":{"command":"fixture-server", "env":{"TOKEN":"synthetic-value"}}},
          "ui": {"footer":{"visible":false}}
        }"#,
        "{}",
    );
    let before = document(&fixture.user_path());
    let snapshot = fixture.inspect();
    let first = rule(&snapshot, &fixture.user_path(), "BeforeTool:0:0");
    let last = rule(&snapshot, &fixture.user_path(), "BeforeTool:0:2");
    let mut replacement = first.definition.clone();
    replacement.group["hooks"][0]["command"] = json!("fixture-first-updated");
    let prepared = (adapter().prepare_hook_edits)(
        &snapshot,
        &target(&snapshot, AgentAssetScope::User),
        &[
            HookNativeEdit::Replace {
                original: first,
                definition: replacement,
            },
            HookNativeEdit::Remove { original: last },
            HookNativeEdit::Add {
                definition: command_definition("SessionStart", "fixture-added"),
            },
        ],
    )
    .unwrap();
    assert_eq!(prepared.writes.len(), 1);
    assert_eq!(prepared.required_capabilities.len(), 1);
    assert!(prepared
        .required_capabilities
        .contains(&HookNativeCapability::Configuration));
    fixture.write_prepared(&snapshot, &prepared);
    let after = document(&fixture.user_path());
    assert_eq!(
        after["hooks"]["BeforeTool"][0]["hooks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        after["hooks"]["BeforeTool"][0]["hooks"][0]["command"],
        "fixture-first-updated"
    );
    assert_eq!(
        after["hooks"]["BeforeTool"][0]["hooks"][0]["custom"],
        before["hooks"]["BeforeTool"][0]["hooks"][0]["custom"]
    );
    assert_eq!(
        after["hooks"]["BeforeTool"][0]["hooks"][1],
        before["hooks"]["BeforeTool"][0]["hooks"][1]
    );
    assert_eq!(
        after["hooks"]["BeforeTool"][0]["customGroup"],
        before["hooks"]["BeforeTool"][0]["customGroup"]
    );
    assert_eq!(after["mcpServers"], before["mcpServers"]);
    assert_eq!(after["ui"], before["ui"]);
    let text = fs::read_to_string(fixture.user_path()).unwrap();
    assert!(text.contains("// untouched middle hook"));
    assert!(text.contains("\"items\":[1,2]"));
    let final_snapshot = fixture.inspect();
    assert_eq!(
        (adapter().inspect_source)(
            &final_snapshot,
            &source_id(&final_snapshot, &fixture.user_path())
        )
        .unwrap()
        .len(),
        3
    );
}

#[test]
fn sequential_group_rejects_standalone_adoption_and_allows_only_proven_in_place_edits() {
    let fixture = Fixture::new(
        r#"{"hooks":{"BeforeTool":[{"matcher":".*","sequential":true,"hooks":[{"type":"command","command":"fixture-first"},{"type":"command","command":"fixture-second"}]}]}}"#,
        "{}",
    );
    let snapshot = fixture.inspect();
    let original = rule(&snapshot, &fixture.user_path(), "BeforeTool:0:0");
    assert!((adapter().validate_adoption)(&original).is_err());
    let mut replacement = original.definition.clone();
    replacement.group["hooks"][0]["command"] = json!("fixture-first-updated");
    let prepared = (adapter().prepare_hook_edits)(
        &snapshot,
        &target(&snapshot, AgentAssetScope::User),
        &[HookNativeEdit::Replace {
            original: original.clone(),
            definition: replacement.clone(),
        }],
    )
    .unwrap();
    assert_eq!(prepared.writes.len(), 1);
    for change_event in [false, true] {
        let mut detached = replacement.clone();
        if change_event {
            detached.event = "AfterTool".to_owned();
        } else {
            detached.group["matcher"] = json!("different");
        }
        assert!((adapter().prepare_hook_edits)(
            &snapshot,
            &target(&snapshot, AgentAssetScope::User),
            &[HookNativeEdit::Replace {
                original: original.clone(),
                definition: detached
            }],
        )
        .is_err());
    }
    fixture.write_prepared(&snapshot, &prepared);
    let after = document(&fixture.user_path());
    assert_eq!(after["hooks"]["BeforeTool"].as_array().unwrap().len(), 1);
    assert_eq!(after["hooks"]["BeforeTool"][0]["sequential"], true);
    assert_eq!(
        after["hooks"]["BeforeTool"][0]["hooks"][1]["command"],
        "fixture-second"
    );
}

#[test]
fn disabled_command_rename_preserves_state_and_discloses_new_identity_peers() {
    let fixture = Fixture::new(
        r#"{"hooksConfig":{"enabled":false,"disabled":["fixture-old"]},"hooks":{"BeforeTool":[{"hooks":[{"type":"command","command":"fixture-old"},{"type":"command","name":"fixture-new","command":"fixture-peer"}]}]}}"#,
        "{}",
    );
    let snapshot = fixture.inspect();
    let original = rule(&snapshot, &fixture.user_path(), "BeforeTool:0:0");
    let mut replacement = original.definition.clone();
    replacement.group["hooks"][0]["command"] = json!("fixture-new");
    let prepared = (adapter().prepare_hook_edits)(
        &snapshot,
        &target(&snapshot, AgentAssetScope::User),
        &[HookNativeEdit::Replace {
            original,
            definition: replacement,
        }],
    )
    .unwrap();
    assert_eq!(prepared.writes.len(), 1);
    assert_eq!(prepared.affected_asset_ids.len(), 2);
    assert!(prepared
        .required_capabilities
        .contains(&HookNativeCapability::NativeSwitch));
    assert!(prepared.read_source_ids.contains(&source_id(
        &snapshot,
        &fixture.home.join(".gemini/extensions")
    )));
    assert!(prepared
        .expectations
        .iter()
        .filter(|expected| expected.occurrences > 0)
        .all(|expected| expected.enabled == Some(AgentAssetDeclaredState::Disabled)));
    assert!(prepared.notes.join(" ").contains("原生禁用身份"));
    fixture.write_prepared(&snapshot, &prepared);
    let user = document(&fixture.user_path());
    assert_eq!(
        user["hooksConfig"]["disabled"],
        json!(["fixture-old", "fixture-new"])
    );
    assert_eq!(user["hooksConfig"]["enabled"], false);
    let after = fixture.inspect();
    assert_eq!(
        rule(&after, &fixture.user_path(), "BeforeTool:0:0").enabled,
        AgentAssetDeclaredState::Disabled
    );
    assert_eq!(
        rule(&after, &fixture.user_path(), "BeforeTool:0:1").enabled,
        AgentAssetDeclaredState::Disabled
    );
}

#[test]
fn applying_enabled_rule_under_existing_disabled_identity_never_enables_its_peers() {
    let fixture = Fixture::new(
        r#"{"hooksConfig":{"disabled":["fixture-new"]},"hooks":{"BeforeAgent":[{"hooks":[{"type":"command","command":"fixture-old"},{"type":"command","command":"fixture-other","name":"fixture-new"}]}]}}"#,
        "{}",
    );
    let snapshot = fixture.inspect();
    let original = rule(&snapshot, &fixture.user_path(), "BeforeAgent:0:0");
    assert_eq!(original.enabled, AgentAssetDeclaredState::Enabled);
    let mut replacement = original.definition.clone();
    replacement.group["hooks"][0]["name"] = json!("fixture-new");
    let prepared = (adapter().prepare_hook_edits)(
        &snapshot,
        &target(&snapshot, AgentAssetScope::User),
        &[HookNativeEdit::Replace {
            original,
            definition: replacement,
        }],
    )
    .unwrap();
    assert!(prepared
        .expectations
        .iter()
        .filter(|expected| expected.occurrences > 0)
        .all(|expected| expected.enabled == Some(AgentAssetDeclaredState::Disabled)));
    fixture.write_prepared(&snapshot, &prepared);
    assert_eq!(
        document(&fixture.user_path())["hooksConfig"]["disabled"],
        json!(["fixture-new"])
    );
    let after = fixture.inspect();
    assert_eq!(
        rule(&after, &fixture.user_path(), "BeforeAgent:0:0").enabled,
        AgentAssetDeclaredState::Disabled
    );
    assert_eq!(
        rule(&after, &fixture.user_path(), "BeforeAgent:0:1").enabled,
        AgentAssetDeclaredState::Disabled
    );
}

#[test]
fn native_targets_reject_package_lookalikes_readonly_files_and_wrong_roles() {
    let fixture = Fixture::new("{}", "{}");
    let mut snapshot = fixture.inspect();
    let user = target(&snapshot, AgentAssetScope::User);
    let mut lookalike = snapshot
        .inventory
        .sources
        .iter()
        .find(|source| source.id == user.source_id)
        .unwrap()
        .clone();
    lookalike.id = "fixture-package-source".to_owned();
    lookalike.path = fixture
        .home
        .join(".gemini/extensions/fake/settings.json")
        .to_string_lossy()
        .into_owned();
    lookalike.origin = AgentAssetInstallationOrigin::NativePackage;
    lookalike.writable = true;
    snapshot.inventory.sources.push(lookalike.clone());
    let targets = (adapter().hook_targets)(&snapshot.inventory, &snapshot.inventory.contexts[0]);
    assert_eq!(targets.len(), 2);
    let edits = [HookNativeEdit::Add {
        definition: command_definition("SessionStart", "fixture-command"),
    }];
    let forged = HookNativeDestination {
        source_id: lookalike.id,
        scope: AgentAssetScope::User,
        role: "settings".to_owned(),
    };
    assert!((adapter().prepare_hook_edits)(&snapshot, &forged, &edits).is_err());
    let wrong_role = HookNativeDestination {
        role: "workspace-settings".to_owned(),
        ..user.clone()
    };
    assert!((adapter().prepare_hook_edits)(&snapshot, &wrong_role, &edits).is_err());
    snapshot
        .inventory
        .sources
        .iter_mut()
        .find(|source| source.id == user.source_id)
        .unwrap()
        .writable = false;
    assert!((adapter().prepare_hook_edits)(&snapshot, &user, &edits).is_err());
    assert_eq!(document(&fixture.user_path()), json!({}));
}

#[test]
fn stale_documents_and_incomplete_or_invalid_policies_never_produce_native_writes() {
    let fixture = Fixture::new(
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"fixture-original"}]}]}}"#,
        "{}",
    );
    let snapshot = fixture.inspect();
    let original = rule(&snapshot, &fixture.user_path(), "SessionStart:0:0");
    let edits = [HookNativeEdit::SetEnabled {
        original,
        enabled: false,
    }];
    fs::write(fixture.user_path(), "{\"external\":true}").unwrap();
    assert!((adapter().prepare_hook_edits)(
        &snapshot,
        &target(&snapshot, AgentAssetScope::User),
        &edits
    )
    .is_err());
    assert_eq!(document(&fixture.user_path()), json!({"external":true}));

    let fixture = Fixture::new(
        r#"{"hooksConfig":{"disabled":[false]},"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"fixture-original"}]}]}}"#,
        "{}",
    );
    let mut snapshot = fixture.inspect();
    let original = rule(&snapshot, &fixture.user_path(), "SessionStart:0:0");
    assert_eq!(original.enabled, AgentAssetDeclaredState::Unknown);
    assert!((adapter().prepare_hook_edits)(
        &snapshot,
        &target(&snapshot, AgentAssetScope::User),
        &[HookNativeEdit::SetEnabled {
            original,
            enabled: true
        }],
    )
    .is_err());
    assert_eq!(
        document(&fixture.user_path())["hooksConfig"]["disabled"],
        json!([false])
    );
    snapshot
        .inventory
        .sources
        .retain(|source| source.scope != AgentAssetScope::Managed);
    assert!(
        (adapter().inspect_source)(&snapshot, &source_id(&snapshot, &fixture.user_path())).is_err()
    );
}

#[test]
fn native_jsonc_trailing_commas_remain_invalid_and_are_never_rewritten() {
    let contents = r#"{
      // Gemini accepts this comment, but its native JSON.parse rejects the comma.
      "hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"fixture-command"},]}]}
    }"#;
    let fixture = Fixture::new(contents, "{}");
    let snapshot = fixture.inspect();
    assert!(!snapshot
        .inventory
        .assets
        .iter()
        .any(|asset| asset.category == AgentAssetCategory::Hook));
    assert!(snapshot
        .inventory
        .sources
        .iter()
        .find(|source| Path::new(&source.path) == fixture.user_path())
        .unwrap()
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::DiscoveryIncomplete {
                agent_kind: AgentCliKind::Gemini,
                category: AgentAssetCategory::Hook,
                reason: AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            }
        )));
    let user_source_id = source_id(&snapshot, &fixture.user_path());
    let hook_declarations = snapshot
        .inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.source_id == user_source_id
                && declaration.native_kind == AgentAssetCategory::Hook
        })
        .collect::<Vec<_>>();
    assert_eq!(hook_declarations.len(), 2);
    assert!(hook_declarations.iter().all(|declaration| {
        declaration.role == AgentAssetDeclarationRole::PolicyOverlay
            && declaration.declared_state == AgentAssetDeclaredState::Unknown
    }));
    assert_eq!(
        hook_declarations
            .iter()
            .map(|declaration| declaration.declaration_key.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "hooksConfig.disabled.invalid",
            "hooksConfig.enabled.invalid",
        ])
    );
    assert!(
        (adapter().inspect_source)(&snapshot, &source_id(&snapshot, &fixture.user_path())).is_err()
    );
    assert!((adapter().prepare_hook_edits)(
        &snapshot,
        &target(&snapshot, AgentAssetScope::User),
        &[HookNativeEdit::Add {
            definition: command_definition("SessionStart", "fixture-added")
        }],
    )
    .is_err());
    assert_eq!(fs::read_to_string(fixture.user_path()).unwrap(), contents);
}

#[test]
fn unrelated_invalid_extension_does_not_block_proven_in_place_command_edit() {
    let fixture = Fixture::new(
        r#"{"hooks":{"AfterAgent":[{"hooks":[{"type":"command","command":"fixture-original"}]}]}}"#,
        "{}",
    );
    let plugin = fixture.extension("{invalid-extension-hooks");
    let snapshot = fixture.inspect();
    let original = rule(&snapshot, &fixture.user_path(), "AfterAgent:0:0");
    let mut replacement = original.definition.clone();
    replacement.group["hooks"][0]["command"] = json!("fixture-updated");
    let prepared = (adapter().prepare_hook_edits)(
        &snapshot,
        &target(&snapshot, AgentAssetScope::User),
        &[HookNativeEdit::Replace {
            original,
            definition: replacement,
        }],
    )
    .unwrap();
    assert_eq!(prepared.writes.len(), 1);
    assert!(!prepared
        .read_source_ids
        .contains(&source_id(&snapshot, &plugin)));
    fixture.write_prepared(&snapshot, &prepared);
    assert_eq!(
        document(&fixture.user_path())["hooks"]["AfterAgent"][0]["hooks"][0]["command"],
        "fixture-updated"
    );
    assert_eq!(
        fs::read_to_string(&plugin).unwrap(),
        "{invalid-extension-hooks"
    );
}
