//! Production catalog regressions for non-copyable rules and policy ownership.
use super::{
    hook_tests::{
        adopt, assert_verified, binding_for, edit_request, plan, primary_target, run, write_json,
    },
    native::hooks::HookNativeRule,
    tests::{fixture as catalog_fixture, fixture_contexts},
    *,
};
use crate::services::{
    agent_cli::{
        self,
        contracts::*,
        environment::{
            config_document::{self, ConfigDocumentFormat},
            mutation::native_test_support::catalog_fixture_inventory,
            source_stable_id,
        },
        AgentCliDefinition,
    },
    agent_runtime::managed_hook::CodexHookService,
};
use serde_json::{json, Value};
use std::{fs, ops::ControlFlow, path::Path};

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
    fn emit_diagnostic(&mut self, _: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        AgentDiagnosticEmission::Accepted
    }
}
impl InitialSourceOutput for SourceList {
    fn emit_initial(&mut self, source: AgentAssetSourceSpec) -> ControlFlow<AgentOutputStop> {
        self.0.push(source);
        ControlFlow::Continue(())
    }
}

fn local_sources(request: AgentSourceDiscoveryRequest<'_>, output: &mut dyn InitialSourceOutput) {
    agent_cli::definition(request.context.agent_kind)
        .environment
        .discover_sources(request, &mut LocalSources(output));
}
fn local_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Vec<AgentConfigurationContext> {
    let mut contexts = fixture_contexts(request, output);
    if request.workspace.is_some() {
        for context in &mut contexts {
            context.trust_context = AgentTrustState::Trusted;
        }
    }
    contexts
}
fn gemini_followups(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    agent_cli::definition(AgentCliKind::Gemini)
        .environment
        .discover_follow_up_sources(request, output);
}
fn parse_native(request: AgentAssetParseRequest<'_>, output: &mut dyn AgentParseOutput) {
    agent_cli::definition(request.context.agent_kind)
        .environment
        .parse(request, output);
}
fn resolve_native(request: AgentAssetResolveRequest<'_>, output: &mut dyn AgentResolveOutput) {
    agent_cli::definition(request.context.agent_kind)
        .environment
        .resolve(request, output);
}

struct LocalInspector {
    home: PathBuf,
    workspace: Option<PathBuf>,
    settings: AppSettings,
    kind: AgentCliKind,
}
impl MutationInspector for LocalInspector {
    fn inspect(&self) -> Result<MutationInventory, AgentAssetMutationError> {
        let native = agent_cli::definition(self.kind);
        let environment = EnvironmentAdapter::with_pipeline(
            local_contexts,
            local_sources,
            (self.kind == AgentCliKind::Gemini)
                .then_some(gemini_followups as AgentFollowUpSourceDiscovery),
            if self.kind == AgentCliKind::Gemini {
                "@google/gemini-cli"
            } else {
                "@openai/codex"
            },
            parse_native,
            resolve_native,
            native.environment.state_assessor(),
        )
        .with_hook_adapter(native.environment.hook_adapter().unwrap());
        let mut snapshot = catalog_fixture_inventory(
            &self.home,
            self.workspace.as_deref(),
            None,
            &[AgentCliDefinition {
                environment,
                ..*native
            }],
        )
        .map_err(|_| {
            AgentAssetMutationError::new(AgentAssetMutationErrorKind::PreparationFailed)
        })?;
        // Native roles still see each readonly scope as explicitly absent. The
        // production scanner never opens the machine's System/Managed settings.
        let context = snapshot.inventory.contexts[0].clone();
        let mut sources = SourceList::default();
        native.environment.discover_sources(
            AgentSourceDiscoveryRequest {
                context: &context,
                home: &self.home,
                workspace: self.workspace.as_deref(),
                installations: &[],
            },
            &mut sources,
        );
        for source in sources.0.into_iter().filter(|source| {
            matches!(
                source.scope,
                AgentAssetScope::System | AgentAssetScope::Managed
            )
        }) {
            snapshot.inventory.sources.push(AgentAssetSource {
                id: source_stable_id(&context, &source.path),
                context_id: context.id.clone(),
                environment_id: context.environment_id.clone(),
                workspace_id: context.workspace_id.clone(),
                label: source.label,
                scope: source.scope,
                origin: source.origin,
                path: source.path.to_string_lossy().into_owned(),
                allowed_root: source.allowed_root.to_string_lossy().into_owned(),
                precedence: source.precedence,
                writable: false,
                sensitive: source.sensitive,
                source_kind: source.source_kind,
                categories: source.categories,
                revision: AgentAssetRevision {
                    is_missing: true,
                    ..Default::default()
                },
                diagnostics: Vec::new(),
                access: Default::default(),
                actions: Vec::new(),
            });
        }
        Ok(snapshot)
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

fn fixture(kind: AgentCliKind) -> (tempfile::TempDir, Arc<LocalInspector>, CatalogService) {
    let (temporary, base, service) = catalog_fixture();
    let inspector = Arc::new(LocalInspector {
        home: base.home.clone(),
        workspace: None,
        settings: base.settings.clone(),
        kind,
    });
    (temporary, inspector, service)
}

fn native_rule(inspector: &LocalInspector, binding: &AgentCatalogBinding) -> HookNativeRule {
    (agent_cli::definition(inspector.kind)
        .environment
        .hook_adapter()
        .unwrap()
        .read_hook)(&inspector.inspect().unwrap(), &binding.native)
    .unwrap()
}

#[test]
fn readonly_runtime_and_plugin_hooks_toggle_through_catalog_without_shared_adoption() {
    let (_temporary, inspector, service) = fixture(AgentCliKind::Gemini);
    let settings = inspector.home.join(".gemini/settings.json");
    write_json(&settings, &json!({"keep":"user-setting"}));
    let extension = inspector.home.join(".gemini/extensions/fixture-provider");
    write_json(
        &extension.join("gemini-extension.json"),
        &json!({"name":"fixture-provider","version":"1.0.0"}),
    );
    let hooks = extension.join("hooks/hooks.json");
    write_json(
        &hooks,
        &json!({"hooks":{"BeforeAgent":[{"hooks":[
            {"type":"runtime","name":"fixture-runtime"},{"type":"plugin"}
        ]}]}}),
    );
    let before = fs::read(&hooks).unwrap();
    let initial = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let observed_ids = initial
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
        .flat_map(|item| {
            item.bindings
                .iter()
                .map(|binding| (binding.native.native_id.clone(), item.id.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(observed_ids.len(), 2);
    for native_id in [
        "fixture-provider:BeforeAgent:0:0",
        "fixture-provider:BeforeAgent:0:1",
    ] {
        for (action, state) in [
            (
                AgentCatalogAction::Disable,
                AgentAssetDeclaredState::Disabled,
            ),
            (AgentCatalogAction::Enable, AgentAssetDeclaredState::Enabled),
        ] {
            let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
            let (item, binding) = catalog
                .assets
                .iter()
                .find_map(|item| {
                    item.bindings
                        .iter()
                        .find(|binding| binding.native.native_id == native_id)
                        .map(|binding| (item, binding))
                })
                .expect("native runtime/plugin Hook must be inventoried");
            assert_eq!(
                item.id, observed_ids[native_id],
                "refresh must retain observed-only native Hook identity"
            );
            assert_eq!(item.ownership, AgentCatalogOwnership::Observed);
            assert!(item.version.is_none() && !item.application.available && !binding.can_adopt);
            assert!(binding.applied_version.is_none());
            let action_kind = if action == AgentCatalogAction::Enable {
                AgentAssetActionKind::Enable
            } else {
                AgentAssetActionKind::Disable
            };
            assert!(
                binding
                    .actions
                    .iter()
                    .any(|candidate| candidate.action == action_kind && candidate.available),
                "{:?}",
                binding.actions
            );
            let confirmation = plan(
                &service,
                inspector.clone(),
                &item.id,
                action,
                vec![binding.id.clone()],
            );
            assert_verified(&run(&service, confirmation));
            assert_eq!(native_rule(&inspector, binding).enabled, state);
            assert_eq!(
                fs::read(&hooks).unwrap(),
                before,
                "readonly package definitions must never be copied or rewritten"
            );
        }
    }
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    for item in catalog
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
    {
        assert_eq!(item.ownership, AgentCatalogOwnership::Observed);
        assert!(item.version.is_none() && !item.application.available);
        assert!(item
            .bindings
            .iter()
            .all(|binding| binding.applied_version.is_none()));
        assert!(item
            .bindings
            .iter()
            .all(|binding| { observed_ids.get(&binding.native.native_id) == Some(&item.id) }));
        assert!(service.definition(&item.id).is_err());
    }
    let value: Value = serde_json::from_slice(&fs::read(settings).unwrap()).unwrap();
    assert_eq!(value["keep"], "user-setting");
    assert_eq!(value["hooksConfig"]["disabled"], json!([]));
}

fn codex_integration(
    inspector: &LocalInspector,
    root: &Path,
) -> (CodexHookService, PathBuf, PathBuf, PathBuf) {
    let app_data = root.canonicalize().unwrap().join("app-data");
    fs::create_dir(&app_data).unwrap();
    let helper = app_data.join("balancehub");
    fs::write(&helper, b"fixture helper, never executed").unwrap();
    let hooks = inspector.home.join(".codex/hooks.json");
    // Codex's standalone document denies unknown top-level fields. Preserve
    // neighboring native metadata through its actual supported field.
    write_json(&hooks, &json!({"description":"native-user-setting"}));
    let manifest = app_data.join("agent-hooks/codex/ownership.json");
    let integration =
        CodexHookService::new(hooks.clone(), manifest.clone(), helper, app_data.clone());
    integration
        .apply(integration.plan(AgentHookMutation::Install))
        .unwrap();
    assert!(integration.inspect().ownership.is_some());
    (integration, app_data, hooks, manifest)
}

#[test]
fn unadopted_native_toggle_keeps_durable_partial_ownership_intent_after_restart() {
    let (temporary, inspector, service) = fixture(AgentCliKind::Codex);
    let (integration, app_data, hooks, manifest) = codex_integration(&inspector, temporary.path());
    let service = service.with_managed_hook_root(app_data.clone());
    let native_before = fs::read(&hooks).unwrap();
    let value: Value = serde_json::from_slice(&native_before).unwrap();
    let command = value["hooks"]["SessionStart"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    let (item, binding) = binding_for(&service, inspector.as_ref(), AgentCliKind::Codex, command);
    assert!(item.version.is_none());
    let confirmation = plan(
        &service,
        inspector.clone(),
        &item.id,
        AgentCatalogAction::Disable,
        vec![binding.id],
    );
    assert!(confirmation.notes.iter().any(|note| note.contains("解除")));
    let mut outside_manifest = fs::read(&manifest).unwrap();
    outside_manifest.push(b'\n');
    let conflict_path = manifest.clone();
    let conflict_bytes = outside_manifest.clone();
    *service.after_hook_write.lock().unwrap() = Some(Box::new(move || {
        // Native policy has committed. Simulate another valid ownership-file
        // revision before its auxiliary replacement, forcing a partial outcome.
        fs::write(conflict_path, conflict_bytes).unwrap();
    }));
    let done = run(&service, confirmation);
    assert_eq!(
        done.targets[0].outcome,
        Some(AgentAssetOperationOutcome::AppliedUnverified)
    );
    assert_eq!(fs::read(&hooks).unwrap(), native_before);
    assert_eq!(fs::read(&manifest).unwrap(), outside_manifest);
    assert!(integration.inspect().ownership.is_some());
    let policy_path = inspector.home.join(".codex/config.toml");
    let policy_after = fs::read(&policy_path).unwrap();
    let library_path = temporary.path().join("library/library.json");
    let library: Value = serde_json::from_slice(&fs::read(&library_path).unwrap()).unwrap();
    let receipts = library["entries"][&item.id]["receipts"]
        .as_object()
        .unwrap();
    assert_eq!(receipts.len(), 1);
    let receipt = receipts.values().next().unwrap();
    assert_eq!(receipt["version"], 0);
    assert_eq!(
        receipt["hook"]["pending"]["files"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    drop(service);
    let restarted = CatalogService::new(
        temporary.path().canonicalize().unwrap().join("library"),
        Arc::new(MutationService::default()),
    )
    .with_managed_hook_root(app_data);
    let catalog = restarted.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|current| current.id == item.id)
        .unwrap();
    assert_eq!(item.ownership, AgentCatalogOwnership::Observed);
    assert!(item.version.is_none() && !item.application.available);
    assert!(item
        .bindings
        .iter()
        .all(|binding| binding.applied_version.is_none()));
    assert_eq!(item.unresolved_targets.len(), 1);
    assert_eq!(
        item.unresolved_targets[0].state,
        AgentCatalogUnresolvedState::Unknown
    );
    assert!(item.unresolved_targets[0].message.contains("对账"));
    assert!(item.unresolved_targets[0].actions.is_empty());
    assert!(restarted.definition(&item.id).is_err());
    let other = catalog
        .assets
        .iter()
        .find(|candidate| {
            candidate.id != item.id
                && candidate.category == AgentAssetCategory::Hook
                && !candidate.bindings.is_empty()
        })
        .unwrap();
    let pending_binding = item
        .bindings
        .first()
        .expect("policy-only partial commit retains the observed native definition");
    let before_association = fs::read(&library_path).unwrap();
    for (destination, binding_id) in [
        (&other.id, &pending_binding.id),
        (&item.id, &other.bindings[0].id),
    ] {
        let source = catalog
            .assets
            .iter()
            .find(|item| {
                item.bindings
                    .iter()
                    .any(|binding| &binding.id == binding_id)
            })
            .unwrap();
        let relation = restarted
            .preview_relation(
                "fixture",
                AgentCatalogRelationPreviewRequest {
                    intent: AgentCatalogRelationIntent::Merge {
                        destination_asset_id: destination.clone(),
                        source_asset_id: source.id.clone(),
                    },
                    expected_revision: catalog.revision.clone(),
                    workspace: None,
                },
                inspector.clone(),
            )
            .unwrap();
        assert!(!relation.available);
        assert!(relation.token.is_none());
        let error = relation.reason.unwrap();
        assert!(error.contains("尚未完成提交对账"), "{error}");
        assert_eq!(fs::read(&library_path).unwrap(), before_association);
    }
    assert_eq!(fs::read(policy_path).unwrap(), policy_after);
    assert_eq!(fs::read(manifest).unwrap(), outside_manifest);
    assert_eq!(
        fs::read(hooks).unwrap(),
        native_before,
        "recovery must never replay writes"
    );
}

#[test]
fn disabled_gemini_rename_transfers_only_the_owned_peer_actually_disabled_by_policy() {
    for new_name in ["ordinary-renamed", "balancehub:gemini:sessionstart:v1"] {
        let (temporary, inspector, service) = fixture(AgentCliKind::Gemini);
        let settings = inspector.home.join(".gemini/settings.json");
        let identity = "balancehub:gemini:sessionstart:v1";
        let owned = json!({"matcher":"*","hooks":[{"type":"command","name":identity,"command":"printf fixture-session-integration"}]});
        write_json(
            &settings,
            &json!({"hooksConfig":{"disabled":["ordinary-disabled"]},"hooks":{
                "BeforeAgent":[{"hooks":[{"type":"command","name":"ordinary-disabled","command":"printf ordinary-disabled"}]}],
                "SessionStart":[owned.clone()]
            }}),
        );
        let app_data = temporary.path().canonicalize().unwrap().join("app-data");
        let manifest = app_data.join("agent-hooks/gemini/ownership.json");
        let ownership = AgentHookOwnership {
            agent_kind: AgentCliKind::Gemini,
            runtime_scope: AgentRuntimeScope::Native,
            config_path: settings.to_string_lossy().into_owned(),
            helper_version: "agent-hook-v1".into(),
            installed_at: 1,
            enabled: true,
            resources: vec![AgentHookOwnedResource {
                event_name: "SessionStart".into(),
                structural_identity: identity.into(),
                content_fingerprint: format!(
                    "sha256:{}",
                    digest(&serde_json::to_vec(&owned).unwrap())
                ),
            }],
        };
        write_json(
            &manifest,
            &json!({"schemaVersion":1,"catalogDetached":false,"ownership":ownership}),
        );
        let manifest_before = fs::read(&manifest).unwrap();
        let service = service.with_managed_hook_root(app_data);
        let (_, peer) = binding_for(
            &service,
            inspector.as_ref(),
            AgentCliKind::Gemini,
            "printf fixture-session-integration",
        );
        assert_eq!(
            native_rule(&inspector, &peer).enabled,
            AgentAssetDeclaredState::Enabled
        );
        let (_, selected) = binding_for(
            &service,
            inspector.as_ref(),
            AgentCliKind::Gemini,
            "printf ordinary-disabled",
        );
        let saved = adopt(&service, inspector.clone(), &selected.id);
        let mut request = edit_request(&saved);
        let variant = &mut request.hook.as_mut().unwrap().variants[0];
        let mut group: Value = serde_json::from_str(&variant.group_json).unwrap();
        group["hooks"][0]["name"] = json!(new_name);
        variant.group_json = serde_json::to_string(&group).unwrap();
        let saved = service.save(request).unwrap();
        let confirmation = plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::ApplyDefinition,
            vec![primary_target(
                &service,
                inspector.as_ref(),
                AgentCliKind::Gemini,
            )],
        );
        let affects_owned = new_name == identity;
        assert_eq!(
            confirmation.notes.iter().any(|note| note.contains("解除")),
            affects_owned
        );
        if affects_owned {
            assert!(confirmation.targets[0]
                .affected_asset_ids
                .contains(&peer.id));
        }
        assert_verified(&run(&service, confirmation));
        let current: Value = serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
        assert_eq!(current["hooks"]["SessionStart"][0], owned);
        let expected = if affects_owned {
            AgentAssetDeclaredState::Disabled
        } else {
            AgentAssetDeclaredState::Enabled
        };
        assert_eq!(native_rule(&inspector, &peer).enabled, expected);
        if affects_owned {
            let receipt: Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
            assert_eq!(receipt["catalogDetached"], true);
            assert_eq!(receipt["ownership"]["enabled"], false);
        } else {
            assert_eq!(fs::read(manifest).unwrap(), manifest_before);
        }
    }
}

#[test]
fn codex_policy_index_rebase_preserves_unrelated_session_ownership_and_state() {
    let (temporary, inspector, service) = fixture(AgentCliKind::Codex);
    let (integration, app_data, hooks, manifest) = codex_integration(&inspector, temporary.path());
    let service = service.with_managed_hook_root(app_data);
    let mut document: Value = serde_json::from_slice(&fs::read(&hooks).unwrap()).unwrap();
    let owned = document["hooks"]["SessionStart"][0].clone();
    let command = owned["hooks"][0]["command"].as_str().unwrap();
    let retained_command = "printf retain-ordinary-neighbor";
    let retained = json!({"type":"command","command":retained_command});
    document["hooks"]["SessionStart"]
        .as_array_mut()
        .unwrap()
        .insert(
            0,
            json!({"hooks":[
                {"type":"command","command":"printf remove-ordinary-neighbor"},
                retained
            ]}),
        );
    write_json(&hooks, &document);
    let policy = inspector.home.join(".codex/config.toml");
    let owned_key = format!("{}:session_start:1:0", hooks.display());
    let old_key = format!("{}:session_start:0:1", hooks.display());
    let new_key = format!("{}:session_start:0:0", hooks.display());
    let state = json!({"enabled":false,"trusted_hash":"retain-ordinary-trust","custom":17});
    let owned_state = json!({"enabled":false,"trusted_hash":"retain-owned-trust","custom":23});
    fs::write(
        &policy,
        config_document::serialize(
            &json!({"hooks":{"state":{
                (old_key.clone()):state.clone(),
                (new_key.clone()):{"enabled":true,"trusted_hash":"remove-ordinary-trust","custom":7},
                (owned_key.clone()):owned_state.clone()
            }}}),
            ConfigDocumentFormat::Toml,
        )
        .unwrap(),
    )
    .unwrap();
    let (_, peer) = binding_for(&service, inspector.as_ref(), AgentCliKind::Codex, command);
    assert_eq!(
        native_rule(&inspector, &peer).enabled,
        AgentAssetDeclaredState::Disabled
    );
    let (_, neighbor) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::Codex,
        retained_command,
    );
    assert_eq!(
        native_rule(&inspector, &neighbor).enabled,
        AgentAssetDeclaredState::Disabled
    );
    let manifest_before = fs::read(&manifest).unwrap();
    let (item, selected) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::Codex,
        "printf remove-ordinary-neighbor",
    );
    let confirmation = plan(
        &service,
        inspector.clone(),
        &item.id,
        AgentCatalogAction::RemoveBinding,
        vec![selected.id],
    );
    assert!(!confirmation.notes.iter().any(|note| note.contains("解除")));
    assert_verified(&run(&service, confirmation));
    let current: Value = serde_json::from_slice(&fs::read(&hooks).unwrap()).unwrap();
    assert_eq!(
        current["hooks"]["SessionStart"],
        json!([{"hooks":[retained]}, owned])
    );
    assert_eq!(current["description"], document["description"]);
    let current =
        config_document::parse(&fs::read(policy).unwrap(), ConfigDocumentFormat::Toml).unwrap();
    assert_eq!(current["hooks"]["state"][new_key], state);
    assert!(current["hooks"]["state"].get(old_key).is_none());
    assert_eq!(current["hooks"]["state"][owned_key], owned_state);
    assert_eq!(current["hooks"]["state"].as_object().unwrap().len(), 2);
    assert_eq!(fs::read(manifest).unwrap(), manifest_before);
    assert!(integration.inspect().ownership.is_some());
    let (_, peer) = binding_for(&service, inspector.as_ref(), AgentCliKind::Codex, command);
    assert_eq!(
        native_rule(&inspector, &peer).enabled,
        AgentAssetDeclaredState::Disabled
    );
    let (_, neighbor) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::Codex,
        retained_command,
    );
    assert_eq!(
        native_rule(&inspector, &neighbor).enabled,
        AgentAssetDeclaredState::Disabled
    );
}

#[test]
fn same_destination_policy_files_cancel_only_the_untouched_hook_target() {
    let (_temporary, base, service) = fixture(AgentCliKind::Gemini);
    let workspace = base.home.join("project");
    let user = base.home.join(".gemini/settings.json");
    let project = workspace.join(".gemini/settings.json");
    write_json(
        &user,
        &json!({"hooksConfig":{"disabled":["user-disabled"]},"hooks":{"BeforeAgent":[{"hooks":[
            {"type":"command","name":"user-disabled","command":"printf user-disabled"},
            {"type":"command","name":"project-disabled","command":"printf project-disabled"}
        ]}]}}),
    );
    write_json(
        &project,
        &json!({"hooksConfig":{"disabled":["project-disabled"]}}),
    );
    let user_before = fs::read(&user).unwrap();
    let project_before = fs::read(&project).unwrap();
    let inspector = Arc::new(LocalInspector {
        home: base.home.clone(),
        workspace: Some(workspace),
        settings: base.settings.clone(),
        kind: AgentCliKind::Gemini,
    });
    let service = Arc::new(service);
    let (item, user_binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::Gemini,
        "printf user-disabled",
    );
    let (_, project_binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::Gemini,
        "printf project-disabled",
    );
    assert_eq!(
        native_rule(&inspector, &user_binding).enabled,
        AgentAssetDeclaredState::Disabled
    );
    assert_eq!(
        native_rule(&inspector, &project_binding).enabled,
        AgentAssetDeclaredState::Disabled
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let source = catalog
        .assets
        .iter()
        .find(|item| {
            item.bindings
                .iter()
                .any(|binding| binding.id == project_binding.id)
        })
        .unwrap();
    let relation = service
        .preview_relation(
            "fixture",
            AgentCatalogRelationPreviewRequest {
                intent: AgentCatalogRelationIntent::Merge {
                    destination_asset_id: item.id.clone(),
                    source_asset_id: source.id.clone(),
                },
                expected_revision: catalog.revision,
                workspace: inspector
                    .workspace()
                    .map(|path| path.to_string_lossy().into_owned()),
            },
            inspector.clone(),
        )
        .unwrap();
    assert!(relation.available, "{:?}", relation.reason);
    service
        .commit_relation(
            "fixture",
            AgentCatalogRelationCommitRequest {
                plan_token: relation.token.unwrap(),
                relation_key: relation.relation_key,
                action: relation.action,
            },
        )
        .unwrap();
    let confirmation = plan(
        &service,
        inspector.clone(),
        &item.id,
        AgentCatalogAction::Enable,
        vec![user_binding.id.clone(), project_binding.id.clone()],
    );
    assert!(
        confirmation.targets.iter().all(|target| target.available),
        "{:?}",
        confirmation.targets
    );
    let operation = service
        .start(
            "fixture",
            &AgentCatalogApplyRequest {
                plan_token: confirmation.token.unwrap(),
                asset_id: confirmation.asset_id,
                action: confirmation.action,
            },
        )
        .unwrap();
    let weak = Arc::downgrade(&service);
    let operation_id = operation.id.clone();
    *service.after_hook_write.lock().unwrap() = Some(Box::new(move || {
        weak.upgrade()
            .unwrap()
            .cancel("fixture", &operation_id)
            .unwrap();
    }));
    service.run_operation(&operation.id);
    let done = service.operation("fixture", &operation.id).unwrap();
    assert_eq!(
        done.targets
            .iter()
            .filter(
                |target| target.outcome == Some(AgentAssetOperationOutcome::CanceledBeforeCommit)
            )
            .count(),
        1,
        "{:?}",
        done.targets
    );
    assert_eq!(
        done.targets
            .iter()
            .filter(|target| target.outcome == Some(AgentAssetOperationOutcome::AppliedUnverified))
            .count(),
        1
    );
    assert_ne!(
        fs::read(&user).unwrap() == user_before,
        fs::read(&project).unwrap() == project_before,
        "only one policy file may change"
    );
    for binding in [&user_binding, &project_binding] {
        let outcome = done
            .targets
            .iter()
            .find(|target| target.target_id == binding.id)
            .unwrap()
            .outcome;
        assert_eq!(
            native_rule(&inspector, binding).enabled,
            if outcome == Some(AgentAssetOperationOutcome::CanceledBeforeCommit) {
                AgentAssetDeclaredState::Disabled
            } else {
                AgentAssetDeclaredState::Enabled
            }
        );
    }
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|current| current.id == item.id)
        .unwrap();
    assert!(
        item.unresolved_targets.is_empty(),
        "each target reconciles from only its own policy file"
    );
}

#[test]
fn observed_hook_association_moves_successful_native_toggle_receipts_with_the_binding() {
    let (_temporary, inspector, service) = fixture(AgentCliKind::Codex);
    let hooks = inspector.home.join(".codex/hooks.json");
    write_json(
        &hooks,
        &json!({"hooks":{"SessionStart":[{"hooks":[
            {"type":"command","command":"printf observed-a"},
            {"type":"command","command":"printf observed-b"}
        ]}]}}),
    );
    let (source, selected) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::Codex,
        "printf observed-a",
    );
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &source.id,
            AgentCatalogAction::Disable,
            vec![selected.id.clone()],
        ),
    ));
    let (destination, _) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::Codex,
        "printf observed-b",
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let current_source = catalog
        .assets
        .iter()
        .find(|item| {
            item.bindings
                .iter()
                .any(|binding| binding.id == selected.id)
        })
        .unwrap();
    assert_eq!(current_source.id, source.id);
    let relation = service
        .preview_relation(
            "fixture",
            AgentCatalogRelationPreviewRequest {
                intent: AgentCatalogRelationIntent::Merge {
                    destination_asset_id: destination.id.clone(),
                    source_asset_id: current_source.id.clone(),
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
            "fixture",
            AgentCatalogRelationCommitRequest {
                plan_token: relation.token.unwrap(),
                relation_key: relation.relation_key,
                action: relation.action,
            },
        )
        .unwrap();
    for _ in 0..2 {
        let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
        assert!(catalog.assets.iter().all(|item| item.id != source.id));
        let item = catalog
            .assets
            .iter()
            .find(|item| item.id == destination.id)
            .unwrap();
        assert_eq!(item.bindings.len(), 2);
        assert_eq!(item.ownership, AgentCatalogOwnership::Observed);
        assert!(item.version.is_none() && item.unresolved_targets.is_empty());
        assert!(item
            .bindings
            .iter()
            .all(|binding| binding.applied_version.is_none()));
    }
    assert_eq!(
        native_rule(&inspector, &selected).enabled,
        AgentAssetDeclaredState::Disabled
    );
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &destination.id,
            AgentCatalogAction::Enable,
            vec![selected.id],
        ),
    ));
    let (item, binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::Codex,
        "printf observed-a",
    );
    assert_eq!(item.id, destination.id);
    assert_eq!(
        native_rule(&inspector, &binding).enabled,
        AgentAssetDeclaredState::Enabled
    );
}
