//! Real native plans remain authoritative when candidate drafts are corrupted.
use super::*;
use crate::models::{AgentAssetNativeRef, AgentAssetRecord, AgentConfigurationContext};
use crate::services::agent_cli::contracts::{AgentAssetResolver, ParsedAgentAsset};
use crate::services::agent_cli::environment::{project_records, AgentAssetProjectionInput};

pub(super) fn native_resolve(request: AgentAssetResolveRequest<'_>) -> ReorderingResolveOutput {
    let mut captured = ReorderingResolveOutput::default();
    definition(request.context.agent_kind)
        .environment()
        .resolve(request, &mut captured);
    captured
}

pub(super) fn emit_captured(
    captured: ReorderingResolveOutput,
    output: &mut dyn AgentResolveOutput,
) {
    for diagnostic in captured.diagnostics {
        output.emit_diagnostic(diagnostic);
    }
    for draft in captured.drafts {
        if output.emit_draft(draft).is_break() {
            return;
        }
    }
}

fn resolve_unchanged(request: AgentAssetResolveRequest<'_>, output: &mut dyn AgentResolveOutput) {
    emit_captured(native_resolve(request), output);
}

#[test]
fn grok_control_shaped_mcp_names_keep_definitions_and_disable_evidence() {
    for disabled in ["['target']", "['target', ' target ', 'target']"] {
        let root = test_root("grok-control-shaped-mcp-name");
        let config = format!(
            "disabled_mcp_servers = {disabled}\n\
             [mcp_servers.target]\ncommand = 'bh-private-target'\n\
             [mcp_servers.'disabled_mcp_servers:target']\ncommand = 'bh-private-named'\n\
             [mcp_servers.neighbor]\ncommand = 'bh-private-neighbor'\n"
        );
        let inventory = native_inventory(
            &root,
            AgentCliKind::Grok,
            vec![("config", config.into_bytes())],
            vec![],
            resolve_unchanged,
        );
        assert_eq!(inventory.assets.len(), 3, "{:?}", inventory.assets);
        super::grok::assert_hook_policy_declaration(&inventory);
        assert_eq!(
            inventory.declarations.len(),
            5,
            "{:?}",
            inventory.declarations
        );
        assert_eq!(
            inventory
                .assets
                .iter()
                .map(|asset| (asset.category, asset.native_id.as_str()))
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                (AgentAssetCategory::Mcp, "target"),
                (AgentAssetCategory::Mcp, "disabled_mcp_servers:target"),
                (AgentAssetCategory::Mcp, "neighbor"),
            ])
        );
        let declarations = inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.native_kind == AgentAssetCategory::Mcp)
            .map(|declaration| (declaration.declaration_key.as_str(), declaration))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(declarations.len(), 4);
        assert_eq!(
            declarations.keys().copied().collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "mcp_servers.target",
                "mcp_servers.disabled_mcp_servers:target",
                "mcp_servers.neighbor",
                "disabled_mcp_servers:target",
            ])
        );
        assert_eq!(
            inventory
                .declarations
                .iter()
                .map(|declaration| &declaration.id)
                .collect::<BTreeSet<_>>()
                .len(),
            5
        );
        let control = declarations["disabled_mcp_servers:target"];
        assert_eq!(control.native_id, "target");
        assert_eq!(control.native_kind, AgentAssetCategory::Mcp);
        assert_eq!(control.role, AgentAssetDeclarationRole::StateOverlay);
        assert_eq!(control.declared_state, AgentAssetDeclaredState::Disabled);
        assert_eq!(
            control.participation,
            AgentAssetResolutionParticipation::Participates
        );
        for (native_id, key, state, availability) in [
            (
                "target",
                "mcp_servers.target",
                AgentAssetState::Disabled,
                AgentAssetEffectiveAvailability::Disabled,
            ),
            (
                "disabled_mcp_servers:target",
                "mcp_servers.disabled_mcp_servers:target",
                AgentAssetState::Enabled,
                AgentAssetEffectiveAvailability::Available,
            ),
            (
                "neighbor",
                "mcp_servers.neighbor",
                AgentAssetState::Enabled,
                AgentAssetEffectiveAvailability::Available,
            ),
        ] {
            let definition = declarations[key];
            assert_eq!(definition.native_kind, AgentAssetCategory::Mcp);
            assert_eq!(definition.native_id, native_id);
            assert_eq!(definition.role, AgentAssetDeclarationRole::Definition);
            assert_eq!(definition.declared_state, AgentAssetDeclaredState::Unknown);
            assert_eq!(definition.source_id, control.source_id);
            let asset = inventory
                .assets
                .iter()
                .find(|asset| asset.native_id == native_id)
                .unwrap();
            assert_eq!(asset.declared_state, state);
            assert_eq!(asset.effective_state, state);
            assert_eq!(
                asset.resolution.relation,
                AgentAssetResolutionRelation::Independent
            );
            assert_eq!(asset.resolution.terminal, None);
            let declared_state = if state == AgentAssetState::Disabled {
                AgentAssetDeclaredState::Disabled
            } else {
                AgentAssetDeclaredState::Enabled
            };
            assert_eq!(
                asset.details,
                AgentAssetDetails::Mcp {
                    transport: AgentMcpTransport::Stdio,
                    declared_state,
                    approval_state: AgentMcpApprovalState::NotRequired,
                    effective_availability: availability,
                }
            );
            let mut evidence = BTreeSet::from([definition.id.as_str()]);
            if native_id == "target" {
                evidence.insert(control.id.as_str());
            }
            assert_eq!(
                asset
                    .represented_declaration_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                evidence
            );
            assert_eq!(
                asset
                    .resolution
                    .contributor_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                evidence
            );
        }
        assert_no_structural_projection_diagnostics(&inventory);
        let serialized = serde_json::to_string(&inventory).unwrap();
        assert!(!serialized.contains("bh-private-"));
        fs::remove_dir_all(root).unwrap();
    }
}

pub(super) fn native_inventory(
    root: &Path,
    kind: AgentCliKind,
    files: Vec<(&'static str, Vec<u8>)>,
    directories: Vec<(&'static str, Vec<AgentAssetDirectoryEntry>)>,
    resolver: AgentAssetResolver,
) -> crate::models::AgentEnvironmentInventory {
    let isolated_root = if kind == AgentCliKind::Grok {
        fs::create_dir_all(root).unwrap();
        root.canonicalize().unwrap()
    } else {
        root.to_path_buf()
    };
    let root = isolated_root.as_path();
    let workspace = root.join("workspace");
    let config_name = format!(".{}", kind.key());
    fs::create_dir_all(root.join(&config_name)).unwrap();
    fs::create_dir_all(workspace.join(config_name)).unwrap();
    if kind == AgentCliKind::Grok {
        super::grok::prepare_packages(root, &directories);
        super::grok::write_workspace_trust(root, &workspace, true);
    }
    let (snapshots, _) = ClaudeFixtureSnapshotPort::new(files, directories, []);
    let snapshots = super::grok::PackageSnapshots::new(root, snapshots);
    let snapshots = if kind == AgentCliKind::Grok {
        snapshots.with_workspace_trust()
    } else {
        snapshots
    };
    let mut installation = fixture_installation();
    installation.agent_kind = kind;
    let (installations, _) = FakeInstallationPort::new(vec![installation]);
    let native = definition(kind).environment();
    let mut adapter = EnvironmentAdapter::with_pipeline(
        |request, output| {
            definition(request.agent_kind)
                .environment()
                .discover_contexts(request, output)
        },
        |request, output| {
            definition(request.context.agent_kind)
                .environment()
                .discover_sources(request, output)
        },
        if kind == AgentCliKind::Grok {
            Some(|request, output| {
                definition(AgentCliKind::Grok)
                    .environment()
                    .discover_follow_up_sources(request, output)
            })
        } else {
            Some(|request, output| {
                definition(AgentCliKind::Codex)
                    .environment()
                    .discover_follow_up_sources(request, output)
            })
        },
        "native-projection-proof-fixture",
        |request, output| {
            definition(request.context.agent_kind)
                .environment()
                .parse(request, output)
        },
        resolver,
        native.state_assessor(),
    );
    if kind == AgentCliKind::Grok {
        adapter = adapter.with_definition_selector(|request| {
            definition(AgentCliKind::Grok)
                .environment()
                .definition_suppressions(request)
        });
    }
    if native.has_workspace_trust_authority() {
        adapter = match kind {
            AgentCliKind::Codex => adapter.with_workspace_trust_authority(
                |request, output| {
                    definition(AgentCliKind::Codex)
                        .environment()
                        .discover_workspace_trust_sources(request, output)
                },
                |request, output| {
                    definition(AgentCliKind::Codex)
                        .environment()
                        .resolve_workspace_trust(request, output)
                },
            ),
            AgentCliKind::Grok => adapter.with_workspace_trust_authority(
                |request, output| {
                    definition(AgentCliKind::Grok)
                        .environment()
                        .discover_workspace_trust_sources(request, output)
                },
                |request, output| {
                    definition(AgentCliKind::Grok)
                        .environment()
                        .resolve_workspace_trust(request, output)
                },
            ),
            _ => panic!("fixture supports Codex/Grok"),
        };
    }
    let definitions = [AgentCliDefinition {
        environment: adapter,
        ..*definition(kind)
    }];
    let inventory = build_inventory_with(
        InventoryInput {
            home: root,
            workspace: Some(&workspace),
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();
    if kind == AgentCliKind::Grok {
        assert_eq!(inventory.contexts.len(), 1);
        assert_eq!(
            inventory.contexts[0].trust_context,
            AgentTrustState::Trusted
        );
        assert!(inventory
            .sources
            .iter()
            .any(|source| { Path::new(&source.path) == root.join(".grok/trusted_folders.toml") }));
    }
    inventory
}

pub(super) fn assert_target_rejected(
    baseline: &crate::models::AgentEnvironmentInventory,
    attacked: &crate::models::AgentEnvironmentInventory,
    native_id: &str,
    survivors: usize,
) {
    assert_eq!(attacked.assets.len(), survivors, "{:?}", attacked.assets);
    assert!(attacked
        .assets
        .iter()
        .all(|asset| asset.native_id != native_id));
    assert_eq!(attacked.declarations.len(), baseline.declarations.len());
    let ids = |inventory: &crate::models::AgentEnvironmentInventory| {
        inventory
            .declarations
            .iter()
            .map(|declaration| declaration.id.clone())
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(ids(attacked), ids(baseline));
    assert!(
        attacked
            .diagnostics
            .iter()
            .chain(
                attacked
                    .sources
                    .iter()
                    .flat_map(|source| &source.diagnostics)
            )
            .chain(
                attacked
                    .declarations
                    .iter()
                    .flat_map(|declaration| &declaration.diagnostics)
            )
            .chain(attacked.assets.iter().flat_map(|asset| {
                asset
                    .diagnostics
                    .iter()
                    .chain(&asset.resolution.diagnostics)
            }))
            .any(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::InvalidProjection { .. }
                    | AgentAssetDiagnostic::InvalidResolution { .. }
            )),
        "{:?}",
        attacked.diagnostics
    );
}

fn replace_with_merge(request: AgentAssetResolveRequest<'_>, output: &mut dyn AgentResolveOutput) {
    let mut native = native_resolve(request);
    assert_eq!(native.drafts.len(), 3);
    native.drafts.retain(|draft| {
        !(draft.native_id == "same"
            && draft.resolution.relation == AgentAssetResolutionRelation::Replaced)
    });
    let winner = native
        .drafts
        .iter_mut()
        .find(|draft| draft.native_id == "same")
        .unwrap();
    assert_eq!(
        winner.resolution.relation,
        AgentAssetResolutionRelation::ReplaceWinner
    );
    winner.resolution.relation = AgentAssetResolutionRelation::Merged;
    winner.resolution.winner = None;
    emit_captured(native, output);
}

#[test]
fn grok_native_replacement_cannot_be_forged_as_merge_without_loser() {
    let root = test_root("grok-native-false-merge");
    let files = vec![
        ("config", b"[mcp_servers.same]\ncommand='bh-private-user'\nenabled=false\n[mcp_servers.neighbor]\ncommand='bh-private-neighbor'".to_vec()),
        ("workspace-config", b"[mcp_servers.same]\nurl='https://bh-private.invalid'\nenabled=true".to_vec()),
    ];
    let baseline = native_inventory(
        &root,
        AgentCliKind::Grok,
        files.clone(),
        vec![],
        resolve_unchanged,
    );
    assert_eq!(baseline.assets.len(), 3);
    super::grok::assert_hook_policy_declaration(&baseline);
    assert_eq!(baseline.declarations.len(), 4);
    assert_eq!(
        baseline
            .declarations
            .iter()
            .filter(|declaration| declaration.native_kind == AgentAssetCategory::Mcp)
            .count(),
        3
    );
    assert_no_structural_projection_diagnostics(&baseline);
    let same = baseline
        .assets
        .iter()
        .filter(|asset| asset.native_id == "same")
        .collect::<Vec<_>>();
    assert_eq!(same.len(), 2);
    assert!(same
        .iter()
        .any(|asset| asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner));
    assert!(same
        .iter()
        .any(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced));
    let attacked = native_inventory(&root, AgentCliKind::Grok, files, vec![], replace_with_merge);
    assert_target_rejected(&baseline, &attacked, "same", 1);
    assert_eq!(attacked.assets[0].native_id, "neighbor");
    assert_eq!(attacked.assets[0].effective_state, AgentAssetState::Enabled);
    assert_eq!(attacked.assets[0].resolution.terminal, None);
    fs::remove_dir_all(root).unwrap();
}

fn unrelated_relationship<const FIELD: u8>(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    let mut native = native_resolve(request);
    assert_eq!(native.drafts.len(), 3);
    let parent = native
        .drafts
        .iter()
        .find(|draft| {
            draft.native_kind == AgentAssetCategory::Plugin && draft.native_id == "parent"
        })
        .unwrap();
    assert_eq!(parent.effective_state, AgentAssetState::Unknown);
    let reference = AgentAssetNativeRef {
        category: parent.native_kind,
        native_id: parent.native_id.clone(),
        qualifier: Some(parent.projection_key.clone()),
    };
    let target = native
        .drafts
        .iter_mut()
        .find(|draft| draft.native_id == "target")
        .unwrap();
    assert!(
        target.provided_by.is_none()
            && target.action_owner.is_none()
            && target.explicitly_affected.is_empty()
    );
    match FIELD {
        0 => target.provided_by = Some(reference),
        1 => target.action_owner = Some(reference),
        2 => target.explicitly_affected.push(reference),
        _ => unreachable!("known relationship field"),
    }
    emit_captured(native, output);
}

#[test]
fn grok_native_relationships_reject_unrelated_resolvable_assets() {
    let root = test_root("grok-native-unrelated-relationships");
    let files = vec![("config", b"[plugins]\nenabled=['parent']\n[mcp_servers.target]\ncommand='bh-private-target'\n[mcp_servers.neighbor]\ncommand='bh-private-neighbor'".to_vec())];
    let directories = vec![(
        "plugins",
        vec![AgentAssetDirectoryEntry {
            name: "parent".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        }],
    )];
    let baseline = native_inventory(
        &root,
        AgentCliKind::Grok,
        files.clone(),
        directories.clone(),
        resolve_unchanged,
    );
    assert_eq!(baseline.assets.len(), 3);
    super::grok::assert_hook_policy_declaration(&baseline);
    assert_eq!(baseline.declarations.len(), 6);
    assert_eq!(
        baseline
            .declarations
            .iter()
            .filter(|declaration| declaration.native_kind != AgentAssetCategory::Hook)
            .fold(BTreeMap::new(), |mut counts, declaration| {
                *counts.entry(declaration.native_kind).or_insert(0) += 1;
                counts
            }),
        BTreeMap::from([
            (AgentAssetCategory::Mcp, 2),
            (AgentAssetCategory::Plugin, 3)
        ])
    );
    assert_no_structural_projection_diagnostics(&baseline);
    for asset in &baseline.assets {
        assert_eq!(
            asset.effective_state,
            if asset.category == AgentAssetCategory::Plugin {
                AgentAssetState::Unknown
            } else {
                AgentAssetState::Enabled
            }
        );
        assert!(asset.relationships.provided_by.is_none());
        assert!(asset.relationships.action_owner.is_none());
        assert!(asset.relationships.affected_asset_ids.is_empty());
    }
    for resolver in [
        unrelated_relationship::<0> as AgentAssetResolver,
        unrelated_relationship::<1>,
        unrelated_relationship::<2>,
    ] {
        let attacked = native_inventory(
            &root,
            AgentCliKind::Grok,
            files.clone(),
            directories.clone(),
            resolver,
        );
        assert_target_rejected(&baseline, &attacked, "target", 2);
        assert_eq!(
            attacked
                .assets
                .iter()
                .map(|asset| asset.native_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["parent", "neighbor"])
        );
        for asset in &attacked.assets {
            assert_eq!(
                asset.effective_state,
                if asset.category == AgentAssetCategory::Plugin {
                    AgentAssetState::Unknown
                } else {
                    AgentAssetState::Enabled
                }
            );
            assert!(asset.relationships.provided_by.is_none());
            assert!(asset.relationships.action_owner.is_none());
            assert!(asset.relationships.affected_asset_ids.is_empty());
        }
    }
    fs::remove_dir_all(root).unwrap();
}

/// Context identity is deliberately fixed so both hash orderings are tested
/// deterministically. Discovery, parser, resolver, assessor and projector remain
/// native; no configuration file is opened and no draft is repaired.
pub(super) struct NativeProjectionFixture {
    pub(super) context: AgentConfigurationContext,
    pub(super) declarations: Vec<ParsedAgentAsset>,
    inputs: Vec<(AgentAssetSourceSpec, AgentAssetSnapshot)>,
}

impl NativeProjectionFixture {
    pub(super) fn new(kind: AgentCliKind, context_id: &str, user: &[u8], workspace: &[u8]) -> Self {
        let root = std::env::temp_dir().join("balancehub-native-anchor-proof");
        let workspace_root = root.join("workspace");
        let context = AgentConfigurationContext {
            id: context_id.to_owned(),
            environment_id: native_environment().id,
            agent_kind: kind,
            config_root: root
                .join(format!(".{}", kind.key()))
                .to_string_lossy()
                .into_owned(),
            profile: "default".to_owned(),
            workspace_id: Some("workspace:fixture".to_owned()),
            trust_context: AgentTrustState::Trusted,
            parser_version: 1,
            schema_facts: BTreeMap::new(),
            compatible_installation_ids: Vec::new(),
        };
        let native = definition(kind).environment();
        let mut discovery = ReorderingInitialSourceOutput::default();
        native.discover_sources(
            AgentSourceDiscoveryRequest {
                installations: &[],
                context: &context,
                home: &root,
                workspace: Some(&workspace_root),
            },
            &mut discovery,
        );
        assert!(discovery.diagnostics.is_empty());
        let inputs = discovery
            .sources
            .into_iter()
            .map(|source| {
                let bytes = match source.native_source_key.as_str() {
                    "config" => Some(user),
                    "workspace-config" => Some(workspace),
                    _ => None,
                };
                let snapshot = match bytes {
                    Some(bytes) => AgentAssetSnapshot::File {
                        bytes: bytes.to_vec(),
                        revision: AgentAssetRevision {
                            identity: format!(
                                "fixture:{}:{}",
                                source.native_source_key,
                                bytes.len()
                            ),
                            observed_at: "2026-09-11T00:00:00+00:00".to_owned(),
                            size_bytes: Some(bytes.len() as u64),
                            ..Default::default()
                        },
                    },
                    None => AgentAssetSnapshot::Missing {
                        revision: AgentAssetRevision {
                            identity: format!("fixture-missing:{}", source.native_source_key),
                            observed_at: "2026-09-11T00:00:00+00:00".to_owned(),
                            is_missing: true,
                            ..Default::default()
                        },
                    },
                };
                (source, snapshot)
            })
            .collect::<Vec<_>>();
        let mut parsed = NativePayloadDebugCollector::default();
        for (source, snapshot) in &inputs {
            native.parse(
                AgentAssetParseRequest {
                    native_home: None,
                    context: &context,
                    source,
                    snapshot,
                    workspace_canonical: Some(&workspace_root),
                    workspace_lexical: Some(&workspace_root),
                },
                &mut parsed,
            );
        }
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        Self {
            context,
            declarations: parsed.declarations,
            inputs,
        }
    }

    pub(super) fn source(&self, key: &str) -> &AgentAssetSourceSpec {
        &self
            .inputs
            .iter()
            .find(|(source, _)| source.native_source_key == key)
            .unwrap()
            .0
    }

    pub(super) fn project(&self, reverse: bool) -> Vec<AgentAssetRecord> {
        let mut declarations = self.declarations.clone();
        let mut sources = self
            .inputs
            .iter()
            .map(|(spec, snapshot)| AgentAssetResolveSource { spec, snapshot })
            .collect::<Vec<_>>();
        if reverse {
            declarations.reverse();
            sources.reverse();
        }
        let native = definition(self.context.agent_kind).environment();
        let resolved = native_resolve(AgentAssetResolveRequest {
            context: &self.context,
            declarations: &declarations,
            sources: &sources,
        });
        assert!(
            resolved.diagnostics.is_empty(),
            "{:?}",
            resolved.diagnostics
        );
        let source_specs = sources
            .iter()
            .map(|source| source.spec.clone())
            .collect::<Vec<_>>();
        // Supply the same observed source metadata to both projections. An
        // empty map asks production to create a new missing-source observation.
        let source_map = self
            .inputs
            .iter()
            .map(|(source, snapshot)| {
                let id = source_stable_id(&self.context, &source.path);
                (
                    id.clone(),
                    AgentAssetSource {
                        id,
                        context_id: self.context.id.clone(),
                        label: source.label.clone(),
                        scope: source.scope,
                        origin: source.origin,
                        environment_id: self.context.environment_id.clone(),
                        workspace_id: self.context.workspace_id.clone(),
                        path: source.path.to_string_lossy().into_owned(),
                        allowed_root: source.allowed_root.to_string_lossy().into_owned(),
                        precedence: source.precedence,
                        writable: source.writable,
                        sensitive: source.sensitive,
                        source_kind: source.source_kind,
                        categories: source.categories.clone(),
                        revision: super::super::snapshot::snapshot_revision_of(snapshot),
                        diagnostics: Vec::new(),
                        access: crate::models::AgentAssetAccess::default(),
                        actions: Vec::new(),
                    },
                )
            })
            .collect();
        let mut run = inventory_run(AgentAssetLimits::DEFAULT);
        let result = project_records(
            &native_environment(),
            &self.context,
            &source_specs,
            &declarations,
            AgentAssetProjectionInput {
                drafts: resolved.drafts,
                state_assessor: native.state_assessor(),
            },
            &source_map,
            &mut run,
        );
        let diagnostics = run.finish_diagnostics();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        result.records
    }
}

pub(super) const HASH_ORDER_CONTEXTS: [(&str, &str, &str); 2] = [
    (
        "review-context-0",
        "declaration:9ad50a3fe7416fcbc458619a5af85f2b2e41a30d44110b7a02117f352e0b5a56",
        "declaration:e27dc49fa7b94b14ad5fa215aa5b47beb6ca963980a7ff87555e65b6aa003654",
    ),
    (
        "review-context-1",
        "declaration:b3a1bae7243901b23eb56a2542031560aba5a3db915faee7e465317c900f1389",
        "declaration:9ff3108597738107f666f84a4a7e2eb87cf818c60fd347ea29443a231f22627c",
    ),
];

pub(super) fn assert_anchor_metadata(
    fixture: &NativeProjectionFixture,
    record: &AgentAssetRecord,
    key: &str,
    scope: AgentAssetScope,
    precedence: u32,
) {
    let source = fixture.source(key);
    assert_eq!(record.scope, scope);
    assert_eq!(record.precedence, precedence);
    assert_eq!(
        record.path,
        Some(source.path.to_string_lossy().into_owned())
    );
    assert_eq!(
        record.inspection_source_id,
        source_stable_id(&fixture.context, &source.path)
    );
    let snapshot = &fixture
        .inputs
        .iter()
        .find(|(input, _)| input.native_source_key == key)
        .unwrap()
        .1;
    assert_eq!(
        serde_json::to_value(&record.revision).unwrap(),
        serde_json::to_value(super::super::snapshot::snapshot_revision_of(snapshot)).unwrap()
    );
}

#[test]
fn grok_native_anchor_metadata_ignores_declaration_hash_order() {
    // Codex imports HASH_ORDER_CONTEXTS with its own raw MCP key. Grok's
    // field-qualified key needs independent fixed tuples for both hash orders.
    const GROK_HASH_ORDER_CONTEXTS: [(&str, &str, &str); 2] = [
        (
            "review-grok-field-context-1",
            "declaration:1a5494aa85cefee48b167ea5562f625e55e5ab36f3a9aaf42675f815a086c634",
            "declaration:3fd98502b1d42b06c09f77b045dea9c99f7c39a034fcdd99754e796115dde2a6",
        ),
        (
            "review-grok-field-context-0",
            "declaration:fa686d0b5147d22f61a28d249d8620b2bff8e406faedbcf80c0963a29893de18",
            "declaration:ac3bfaa84545f60c9df4d6bce5696dcfef7c23bfa3eac129be682c9f3a3fe765",
        ),
    ];
    assert!(GROK_HASH_ORDER_CONTEXTS[0].1 < GROK_HASH_ORDER_CONTEXTS[0].2);
    assert!(GROK_HASH_ORDER_CONTEXTS[1].1 > GROK_HASH_ORDER_CONTEXTS[1].2);
    for (context, user_id, workspace_id) in GROK_HASH_ORDER_CONTEXTS {
        let fixture = NativeProjectionFixture::new(
            AgentCliKind::Grok,
            context,
            b"[mcp_servers.same]\ncommand='bh-private-user'\nenabled=false\n[mcp_servers.neighbor]\ncommand='bh-private-neighbor'",
            b"[mcp_servers.same]\nurl='https://bh-private.invalid'\nenabled=true",
        );
        let policies = fixture
            .declarations
            .iter()
            .filter(|declaration| declaration.category == AgentAssetCategory::Hook)
            .collect::<Vec<_>>();
        assert_eq!(policies.len(), 1);
        let policy = policies[0];
        assert_eq!(policy.source_key, "hook-disabled-state");
        assert_eq!(policy.native_id, "hook-disabled-state");
        assert_eq!(policy.declaration_key, "hook-disabled-state");
        assert_eq!(policy.role, AgentAssetDeclarationRole::PolicyOverlay);
        assert_eq!(policy.declared_state, AgentAssetDeclaredState::Unknown);
        assert_eq!(
            policy.participation,
            AgentAssetResolutionParticipation::Participates
        );
        assert!(policy.provided_by.is_none());
        assert!(policy.action_owner.is_none());
        assert!(policy.explicitly_affected.is_empty());
        assert!(policy.facts.is_empty());
        assert!(matches!(
            &policy.native_payload,
            AgentAssetNativePayload::GrokHook(
                crate::services::agent_cli::grok::GrokHookPayload::Disabled {
                    names: Some(names),
                }
            ) if names.is_empty()
        ));
        assert_eq!(fixture.declarations.len(), 4);
        assert_eq!(
            fixture
                .declarations
                .iter()
                .filter(|declaration| declaration.category == AgentAssetCategory::Mcp)
                .count(),
            3
        );
        let ids = fixture
            .declarations
            .iter()
            .filter(|declaration| declaration.native_id == "same")
            .map(|declaration| declaration.declaration_id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(ids, BTreeSet::from([user_id, workspace_id]));
        let forward = fixture.project(false);
        let reversed = fixture.project(true);
        assert_eq!(
            serde_json::to_value(&forward).unwrap(),
            serde_json::to_value(&reversed).unwrap()
        );
        for rows in [&forward, &reversed] {
            assert_eq!(rows.len(), 3);
            let winner = rows
                .iter()
                .find(|row| {
                    row.native_id == "same"
                        && row.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner
                })
                .unwrap();
            let loser = rows
                .iter()
                .find(|row| {
                    row.native_id == "same"
                        && row.resolution.relation == AgentAssetResolutionRelation::Replaced
                })
                .unwrap();
            assert_anchor_metadata(
                &fixture,
                winner,
                "workspace-config",
                AgentAssetScope::Workspace,
                20,
            );
            assert_anchor_metadata(&fixture, loser, "config", AgentAssetScope::User, 10);
            assert_eq!(winner.effective_state, AgentAssetState::Enabled);
            assert_eq!(
                winner.details,
                AgentAssetDetails::Mcp {
                    transport: AgentMcpTransport::Http,
                    declared_state: AgentAssetDeclaredState::Enabled,
                    approval_state: AgentMcpApprovalState::NotRequired,
                    effective_availability: AgentAssetEffectiveAvailability::Available,
                }
            );
            assert_eq!(loser.effective_state, AgentAssetState::Shadowed);
            assert_eq!(
                loser.details,
                AgentAssetDetails::Mcp {
                    transport: AgentMcpTransport::Stdio,
                    declared_state: AgentAssetDeclaredState::Disabled,
                    approval_state: AgentMcpApprovalState::NotRequired,
                    effective_availability: AgentAssetEffectiveAvailability::Disabled,
                }
            );
            assert_eq!(
                winner.resolution.winner_id.as_deref(),
                Some(winner.stable_id.as_str())
            );
            assert_eq!(
                loser.resolution.winner_id.as_deref(),
                Some(winner.stable_id.as_str())
            );
            assert_eq!(loser.represented_declaration_ids, vec![user_id.to_owned()]);
            assert_eq!(loser.resolution.contributor_ids, vec![user_id.to_owned()]);
            assert_eq!(
                winner
                    .represented_declaration_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([user_id, workspace_id])
            );
            assert_eq!(
                winner
                    .resolution
                    .contributor_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([user_id, workspace_id])
            );
        }
    }
}
