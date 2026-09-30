//! Native Codex inputs traverse discovery, parsing, resolution, assessment and projection.
use super::*;
use crate::models::{AgentAssetPolicyReference, AgentAssetRecord, AgentEnvironmentInventory};
use crate::services::agent_cli::contracts::{
    AgentAssetDeclaredStateProofDraft as DeclaredProof,
    AgentAssetDeclaredUnknownCauseDraft as UnknownCause,
    AgentAssetEffectiveStateProofDraft as EffectiveProof, AgentAssetProjectedDraft,
    AgentAssetStateEvidenceRefDraft as Evidence, AgentAssetTerminalCauseDraft as TerminalCause,
    CodexRequirementsPayload,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

// Each config source emits Plugin and Hook-feature control roots. User config
// also emits ordered Skill rules and per-rule Hook state, including empty sets.
const NATIVE_CONFIG_CONTROL_COUNT: usize = 2 * CODEX_FIXTURE_CONFIG_SOURCES.len() + 2;

#[derive(Default)]
struct NativeCapture {
    parsed: NativePayloadDebugCollector,
    drafts: Vec<AgentAssetProjectedDraft>,
}

thread_local! {
    static NATIVE_CAPTURE: RefCell<Option<NativeCapture>> = const { RefCell::new(None) };
}

fn parse_with_capture(request: AgentAssetParseRequest<'_>, output: &mut dyn AgentParseOutput) {
    NATIVE_CAPTURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let capture = &mut slot.as_mut().expect("active Codex fixture capture").parsed;
        definition(AgentCliKind::Codex)
            .environment()
            .parse(request, &mut TeeParseOutput { capture, output });
    });
}

fn resolve_with_capture<const REVERSE: bool>(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    let mut declarations = request.declarations.to_vec();
    let mut sources = request.sources.to_vec();
    if REVERSE {
        declarations.reverse();
        sources.reverse();
    }
    let mut captured = ReorderingResolveOutput::default();
    definition(AgentCliKind::Codex).environment().resolve(
        AgentAssetResolveRequest {
            context: request.context,
            declarations: &declarations,
            sources: &sources,
        },
        &mut captured,
    );
    NATIVE_CAPTURE.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .expect("active Codex fixture capture")
            .drafts
            .extend(captured.drafts.iter().cloned());
    });
    for diagnostic in captured.diagnostics {
        output.emit_diagnostic(diagnostic);
    }
    for draft in captured.drafts {
        if output.emit_draft(draft).is_break() {
            return;
        }
    }
}

fn native_adapter<const REVERSE: bool>() -> EnvironmentAdapter {
    let native = definition(AgentCliKind::Codex).environment();
    EnvironmentAdapter::with_pipeline(
        |request, output| {
            definition(AgentCliKind::Codex)
                .environment()
                .discover_contexts(request, output)
        },
        |request, output| {
            definition(AgentCliKind::Codex)
                .environment()
                .discover_sources(request, output)
        },
        Some(|request, output| {
            definition(AgentCliKind::Codex)
                .environment()
                .discover_follow_up_sources(request, output)
        }),
        "codex-native-matrix",
        parse_with_capture,
        resolve_with_capture::<REVERSE>,
        native.state_assessor(),
    )
    .with_workspace_trust_authority(
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
    )
}

struct MatrixSnapshots {
    files: CodexFixtureSnapshotPort,
    directories: BTreeMap<&'static str, Vec<&'static str>>,
}

impl SnapshotPort for MatrixSnapshots {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        let entries = self
            .directories
            .get(request.source.native_source_key.as_str());
        let mut snapshot = self.files.snapshot(request, run);
        // Forward/reversed runs share the same observation, as well as the
        // same source bytes. Compare full public rows without wall-clock drift.
        let revision = match &mut snapshot {
            AgentAssetSnapshot::File { revision, .. }
            | AgentAssetSnapshot::Missing { revision }
            | AgentAssetSnapshot::DirectoryManifest { revision, .. }
            | AgentAssetSnapshot::Blocked { revision, .. } => revision,
        };
        revision.observed_at = "2026-09-11T00:00:00+00:00".to_owned();
        let Some(entries) = entries else {
            return snapshot;
        };
        let AgentAssetSnapshot::DirectoryManifest { revision, .. } = snapshot else {
            panic!("Codex Skill fixture must be a directory");
        };
        AgentAssetSnapshot::DirectoryManifest {
            entries: entries
                .iter()
                .map(|name| AgentAssetDirectoryEntry {
                    name: (*name).to_owned(),
                    source_kind: AgentAssetSourceKind::Directory,
                    is_symlink: false,
                })
                .collect(),
            revision,
            complete: true,
        }
    }
}

struct NativeFixture {
    root: PathBuf,
    workspace: PathBuf,
}

impl NativeFixture {
    fn new(name: &str) -> Self {
        let root = test_root(name);
        fs::create_dir_all(root.join(".codex")).unwrap();
        fs::create_dir_all(root.join("workspace/.codex")).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let workspace = fs::canonicalize(root.join("workspace")).unwrap();
        Self { root, workspace }
    }

    fn run<const REVERSE: bool>(
        &self,
        mut files: Vec<(&'static str, Vec<u8>)>,
        directories: impl IntoIterator<Item = (&'static str, Vec<&'static str>)>,
    ) -> NativeRun {
        // Only prepend valid trust configuration to config. Requirements bytes,
        // including invalid UTF-8, reach the production parser unchanged.
        let trust = format!(
            "[projects.{}]\ntrust_level = 'trusted'\n",
            toml::Value::String(self.workspace.to_string_lossy().into_owned())
        );
        let config = files
            .iter_mut()
            .find(|(key, _)| *key == "config")
            .expect("fixture config");
        config.1 = [trust.as_bytes(), config.1.as_slice()].concat();
        let directories = directories.into_iter().collect::<BTreeMap<_, _>>();
        let (mut files, _) = CodexFixtureSnapshotPort::new(files, []);
        for (source, entries) in &directories {
            for name in entries {
                files.files.insert(
                    skill_source_key(source, name),
                    format!("---\nname: {name}\ndescription: Native fixture Skill\n---\nbh-private-skill-body\n").into_bytes(),
                );
            }
        }
        let snapshots = MatrixSnapshots { files, directories };
        let mut installation = fixture_installation();
        installation.agent_kind = AgentCliKind::Codex;
        let (installations, _) = FakeInstallationPort::new(vec![installation]);
        let definitions = [AgentCliDefinition {
            environment: native_adapter::<REVERSE>(),
            ..*definition(AgentCliKind::Codex)
        }];
        NATIVE_CAPTURE.with(|slot| *slot.borrow_mut() = Some(NativeCapture::default()));
        let inventory = build_inventory_with(
            InventoryInput {
                home: &self.root,
                workspace: Some(&self.workspace),
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
        let capture = NATIVE_CAPTURE.with(|slot| slot.borrow_mut().take().unwrap());
        assert_eq!(inventory.contexts.len(), 1);
        assert_eq!(
            inventory.contexts[0].trust_context,
            AgentTrustState::Trusted
        );
        assert_no_structural_projection_diagnostics(&inventory);
        assert_codex_feature_policy_payloads(
            &capture.parsed.declarations,
            CODEX_FIXTURE_CONFIG_SOURCES,
        );
        for feature in capture
            .parsed
            .declarations
            .iter()
            .filter(|asset| asset.declaration_key == "codex-hook-feature-policy")
        {
            assert!(inventory.assets.iter().all(|asset| {
                !asset
                    .represented_declaration_ids
                    .contains(&feature.declaration_id)
                    && !asset
                        .resolution
                        .contributor_ids
                        .contains(&feature.declaration_id)
            }));
        }
        NativeRun { inventory, capture }
    }
}

fn skill_source_key(root: &str, entry: &str) -> String {
    format!("codex-skill:{root}:{}", URL_SAFE_NO_PAD.encode(entry))
}

impl Drop for NativeFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct NativeRun {
    inventory: AgentEnvironmentInventory,
    capture: NativeCapture,
}

impl NativeRun {
    fn declaration(&self, source: &str, category: AgentAssetCategory, key: &str) -> String {
        let expected = super::super::stable_id(
            "declaration",
            &[
                &self.inventory.contexts[0].id,
                source,
                category.key().as_str(),
                key,
            ],
        );
        let matches = self
            .inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.id == expected)
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1, "{source}/{key}");
        assert_eq!(matches[0].native_kind, category);
        assert!(self.capture.parsed.declarations.iter().any(|declaration| {
            declaration.declaration_id == expected
                && declaration.source_key == source
                && declaration.declaration_key == key
        }));
        expected
    }

    fn draft(&self, projection_key: &str) -> &AgentAssetProjectedDraft {
        let matches = self
            .capture
            .drafts
            .iter()
            .filter(|draft| draft.projection_key == projection_key)
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1, "{projection_key}");
        matches[0]
    }

    fn assert_private_values_redacted(&self) {
        let public = serde_json::to_string(&self.inventory).unwrap();
        let debug = format!(
            "{:?} {:?} {:?}",
            self.capture.parsed.declarations,
            self.capture.parsed.payload_debug,
            self.capture.drafts
        );
        // Every command, URL and matcher value below carries this sentinel.
        assert!(!public.contains("bh-private"), "{public}");
        assert!(!debug.contains("bh-private"), "{debug}");
    }

    fn assert_status_neighbor(&self) {
        let status = assets_with(&self.inventory, AgentAssetCategory::StatusUi, "status-line");
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].declared_state, AgentAssetState::Enabled);
        assert_eq!(status[0].effective_state, AgentAssetState::Enabled);
        assert_eq!(
            status[0].resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert_eq!(status[0].resolution.terminal, None);
        assert_eq!(status[0].resolution.control_source, None);
        let id = self.declaration("config", AgentAssetCategory::StatusUi, "status-line");
        assert_identity_set(status[0], std::slice::from_ref(&id));
    }
}

fn assets_with<'a>(
    inventory: &'a AgentEnvironmentInventory,
    category: AgentAssetCategory,
    native_id: &str,
) -> Vec<&'a AgentAssetRecord> {
    inventory
        .assets
        .iter()
        .filter(|asset| asset.category == category && asset.native_id == native_id)
        .collect()
}

fn assert_identity_set(asset: &AgentAssetRecord, ids: &[String]) {
    let expected = ids.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(expected.len(), ids.len());
    assert_eq!(
        asset
            .represented_declaration_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>(),
        expected
    );
    assert_eq!(
        asset
            .resolution
            .contributor_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>(),
        expected
    );
}

fn evidence(ids: &[String]) -> Vec<Evidence> {
    let mut values = ids
        .iter()
        .map(|id| Evidence::Declaration {
            declaration_id: id.clone(),
        })
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn assert_terminal(draft: &AgentAssetProjectedDraft, cause: TerminalCause, ids: &[String]) {
    assert_eq!(
        draft.state_proof.effective,
        EffectiveProof::Terminal {
            terminal: if cause == TerminalCause::TypedPolicy {
                AgentAssetResolutionTerminal::PolicyBlocked
            } else {
                AgentAssetResolutionTerminal::Unknown
            },
            cause,
            evidence: evidence(ids),
            input: Box::new(EffectiveProof::Intrinsic),
        }
    );
}

fn with_status(config: &str) -> Vec<u8> {
    format!("{config}\n[tui]\nstatus_line = []\n").into_bytes()
}

fn assert_same_rows_and_proofs(first: &NativeRun, reversed: &NativeRun) {
    assert_eq!(
        serde_json::to_value(&first.inventory.assets).unwrap(),
        serde_json::to_value(&reversed.inventory.assets).unwrap()
    );
    // Inventory stamps when each parsed declaration was materialized. This
    // timestamp is independent of the snapshot observation and input ordering.
    // Preserve every semantic field, including the fixed snapshot revision.
    let declarations = |run: &NativeRun| {
        let mut declarations = run.inventory.declarations.clone();
        for declaration in &mut declarations {
            declaration.evidence.observed_at.clear();
        }
        serde_json::to_value(declarations).unwrap()
    };
    assert_eq!(declarations(first), declarations(reversed));
    let proofs = |run: &NativeRun| {
        run.capture
            .drafts
            .iter()
            .map(|draft| (draft.projection_key.clone(), draft.state_proof.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(proofs(first), proofs(reversed));
}

#[test]
fn codex_native_requirements_identity_matrix_keeps_exact_terminal_authority() {
    const STDIO: &str = "command = 'bh-private-runner'\nargs = ['bh-private-exact', 'bh-private-prefix/path', 'bh-private-regex-42']";
    const HTTP: &str = "url = 'https://bh-private.invalid/mcp/42'";
    const LEGACY_COMMAND: &str = "{ command = 'bh-private-runner' }";
    const LEGACY_URL: &str = "{ url = 'https://bh-private.invalid/mcp/42' }";
    const COMMAND: &str = "{ command = { executable = 'bh-private-runner', args = [{ match = 'exact', value = 'bh-private-exact' }, { match = 'prefix', value = 'bh-private-prefix/' }, { match = 'regex', expression = 'bh-private-regex-[0-9]+' }] } }";
    const URL_REGEX: &str =
        "{ url = { match = 'regex', expression = 'https://bh-private[.]invalid/mcp/[0-9]+' } }";
    let cases = [
        ("legacy-command-ignores-args", STDIO, LEGACY_COMMAND, AgentMcpTransport::Stdio, true),
        ("legacy-command-requires-stdio", HTTP, LEGACY_COMMAND, AgentMcpTransport::Http, false),
        ("legacy-url-exact", HTTP, LEGACY_URL, AgentMcpTransport::Http, true),
        ("legacy-url-different", "url = 'https://bh-private.invalid/mcp/420'", LEGACY_URL, AgentMcpTransport::Http, false),
        ("legacy-url-requires-http", STDIO, LEGACY_URL, AgentMcpTransport::Stdio, false),
        ("command-all-matchers", STDIO, COMMAND, AgentMcpTransport::Stdio, true),
        ("command-executable", "command = 'bh-private-other'\nargs = ['bh-private-exact', 'bh-private-prefix/path', 'bh-private-regex-42']", COMMAND, AgentMcpTransport::Stdio, false),
        ("command-argument-count", "command = 'bh-private-runner'\nargs = ['bh-private-exact', 'bh-private-prefix/path']", COMMAND, AgentMcpTransport::Stdio, false),
        ("command-argument-position", "command = 'bh-private-runner'\nargs = ['bh-private-prefix/path', 'bh-private-exact', 'bh-private-regex-42']", COMMAND, AgentMcpTransport::Stdio, false),
        ("command-exact-whole-value", "command = 'bh-private-runner'\nargs = ['bh-private-exact-extra', 'bh-private-prefix/path', 'bh-private-regex-42']", COMMAND, AgentMcpTransport::Stdio, false),
        ("command-prefix-start", "command = 'bh-private-runner'\nargs = ['bh-private-exact', 'before-bh-private-prefix/path', 'bh-private-regex-42']", COMMAND, AgentMcpTransport::Stdio, false),
        ("command-regex-leading-text", "command = 'bh-private-runner'\nargs = ['bh-private-exact', 'bh-private-prefix/path', 'before-bh-private-regex-42']", COMMAND, AgentMcpTransport::Stdio, false),
        ("command-regex-trailing-text", "command = 'bh-private-runner'\nargs = ['bh-private-exact', 'bh-private-prefix/path', 'bh-private-regex-42-after']", COMMAND, AgentMcpTransport::Stdio, false),
        ("command-requires-stdio", HTTP, COMMAND, AgentMcpTransport::Http, false),
        ("url-matcher-exact", HTTP, "{ url = { match = 'exact', value = 'https://bh-private.invalid/mcp/42' } }", AgentMcpTransport::Http, true),
        ("url-matcher-prefix", HTTP, "{ url = { match = 'prefix', value = 'https://bh-private.invalid/mcp/' } }", AgentMcpTransport::Http, true),
        ("url-matcher-regex", HTTP, URL_REGEX, AgentMcpTransport::Http, true),
        ("url-regex-leading-text", "url = 'https://other.invalid/before-https://bh-private.invalid/mcp/42'", URL_REGEX, AgentMcpTransport::Http, false),
        ("url-regex-trailing-text", "url = 'https://bh-private.invalid/mcp/42/after'", URL_REGEX, AgentMcpTransport::Http, false),
        ("url-matcher-requires-http", STDIO, URL_REGEX, AgentMcpTransport::Stdio, false),
    ];
    for (name, config, identity, transport, allowed) in cases {
        let fixture = NativeFixture::new(name);
        let run = fixture.run::<false>(
            vec![
                (
                    "config",
                    with_status(&format!("[mcp_servers.server]\nenabled = true\n{config}")),
                ),
                (
                    "system-requirements",
                    format!("[mcp_servers.server]\nidentity = {identity}\n").into_bytes(),
                ),
            ],
            [],
        );
        assert_eq!(run.inventory.assets.len(), 2, "{name}");
        assert_eq!(
            run.inventory.declarations.len(),
            4 + NATIVE_CONFIG_CONTROL_COUNT,
            "{name}"
        );
        run.assert_status_neighbor();
        let asset = codex_mcp_asset(&run.inventory, "server");
        let definition = run.declaration("config", AgentAssetCategory::Mcp, "server");
        let policy = run.declaration(
            "system-requirements",
            AgentAssetCategory::Mcp,
            "policy:requirements:server",
        );
        let root = run.declaration(
            "system-requirements",
            AgentAssetCategory::Mcp,
            "policy:requirements-root",
        );
        assert_identity_set(asset, &[definition.clone(), policy.clone()]);
        assert!(!asset.represented_declaration_ids.contains(&root));
        assert_eq!(
            asset.resolution.relation,
            AgentAssetResolutionRelation::Independent,
            "{name}"
        );
        assert_eq!(asset.declared_state, AgentAssetState::Enabled, "{name}");
        assert_eq!(
            asset.effective_state,
            if allowed {
                AgentAssetState::Enabled
            } else {
                AgentAssetState::Blocked
            },
            "{name}"
        );
        assert_eq!(
            asset.details,
            AgentAssetDetails::Mcp {
                transport,
                declared_state: AgentAssetDeclaredState::Enabled,
                approval_state: AgentMcpApprovalState::NotRequired,
                effective_availability: if allowed {
                    AgentAssetEffectiveAvailability::Available
                } else {
                    AgentAssetEffectiveAvailability::PolicyBlocked
                },
            },
            "{name}"
        );
        let draft = run.draft("mcp:server");
        assert_eq!(
            draft.state_proof.declared,
            DeclaredProof::Definition {
                declaration_ids: vec![definition.clone()],
                selected_id: definition,
            },
            "{name}"
        );
        if allowed {
            assert_eq!(asset.resolution.terminal, None, "{name}");
            assert_eq!(asset.resolution.control_source, None, "{name}");
            assert_eq!(
                draft.state_proof.effective,
                EffectiveProof::Intrinsic,
                "{name}"
            );
        } else {
            assert_eq!(
                asset.resolution.terminal,
                Some(AgentAssetResolutionTerminal::PolicyBlocked),
                "{name}"
            );
            assert_eq!(
                asset.resolution.control_source,
                Some(AgentAssetPolicyReference::Declaration {
                    declaration_id: policy.clone(),
                }),
                "{name}"
            );
            assert_terminal(draft, TerminalCause::TypedPolicy, &[policy]);
        }
        run.assert_private_values_redacted();
    }
}

#[test]
fn codex_native_invalid_requirements_preserve_known_declared_states() {
    for (name, requirements) in [
        ("malformed-toml", b"[mcp_servers.server\nbh-private-invalid".as_slice()),
        ("invalid-utf8", b"\xff[mcp_servers.server]\nidentity={command='bh-private-runner'}"),
        ("wrong-table", b"mcp_servers = 'bh-private-invalid'"),
        ("invalid-tail-after-valid-entry", b"[mcp_servers.enabled]\nidentity={command='bh-private-runner'}\n[mcp_servers.invalid.identity]\nurl={match='regex',expression='[bh-private-invalid'}"),
    ] {
        let fixture = NativeFixture::new(name);
        let run = fixture.run::<false>(
            vec![
                ("config", with_status("[mcp_servers.enabled]\ncommand='bh-private-runner'\nenabled=true\n[mcp_servers.disabled]\ncommand='bh-private-runner'\nenabled=false")),
                ("system-requirements", requirements.to_vec()),
            ],
            [],
        );
        assert_eq!(run.inventory.assets.len(), 3, "{name}");
        assert_eq!(run.inventory.declarations.len(), 4 + NATIVE_CONFIG_CONTROL_COUNT, "{name}");
        run.assert_status_neighbor();
        let root = run.declaration("system-requirements", AgentAssetCategory::Mcp, "policy:requirements-root");
        let roots = run.capture.parsed.declarations.iter().filter(|declaration| {
            matches!(declaration.native_payload, AgentAssetNativePayload::CodexRequirements(_))
        }).collect::<Vec<_>>();
        assert_eq!(roots.len(), 1, "{name}");
        assert!(matches!(roots[0].native_payload, AgentAssetNativePayload::CodexRequirements(CodexRequirementsPayload::InvalidRequirements)), "{name}");
        let source = run.inventory.sources.iter().find(|source| source.id == run.inventory.declarations.iter().find(|declaration| declaration.id == root).unwrap().source_id).unwrap();
        assert_eq!(source.diagnostics.iter().filter(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Malformed { format: AgentAssetDocumentFormat::Toml, .. })).count(), 1, "{name}");
        for (native_id, state, declared_state) in [
            ("enabled", AgentAssetState::Enabled, AgentAssetDeclaredState::Enabled),
            ("disabled", AgentAssetState::Disabled, AgentAssetDeclaredState::Disabled),
        ] {
            let asset = codex_mcp_asset(&run.inventory, native_id);
            let id = run.declaration("config", AgentAssetCategory::Mcp, native_id);
            assert_identity_set(asset, std::slice::from_ref(&id));
            assert_eq!(asset.declared_state, state, "{name}/{native_id}");
            assert_eq!(asset.effective_state, AgentAssetState::Unknown, "{name}/{native_id}");
            assert_eq!(asset.resolution.terminal, Some(AgentAssetResolutionTerminal::Unknown));
            assert_eq!(asset.resolution.control_source, Some(AgentAssetPolicyReference::Declaration { declaration_id: root.clone() }));
            assert_eq!(asset.details, AgentAssetDetails::Mcp {
                transport: AgentMcpTransport::Stdio,
                declared_state,
                approval_state: AgentMcpApprovalState::NotRequired,
                effective_availability: AgentAssetEffectiveAvailability::Unknown,
            });
            let draft = run.draft(&format!("mcp:{native_id}"));
            assert_eq!(draft.state_proof.declared, DeclaredProof::Definition { declaration_ids: vec![id.clone()], selected_id: id });
            assert_terminal(draft, TerminalCause::InvalidControl, std::slice::from_ref(&root));
        }
        run.assert_private_values_redacted();
    }
}

#[test]
fn codex_native_higher_definition_overrides_lower_overlay_after_reversal() {
    let fixture = NativeFixture::new("codex-native-overlay-precedence");
    let files = vec![
        ("config", with_status("[mcp_servers.server]\nenabled=false")),
        (
            "workspace-config",
            b"[mcp_servers.server]\ncommand='bh-private-runner'\nenabled=true".to_vec(),
        ),
    ];
    let first = fixture.run::<false>(files.clone(), []);
    let reversed = fixture.run::<true>(files, []);
    for run in [&first, &reversed] {
        assert_eq!(run.inventory.assets.len(), 2);
        assert_eq!(
            run.inventory.declarations.len(),
            4 + NATIVE_CONFIG_CONTROL_COUNT
        );
        run.assert_status_neighbor();
        let lower = run.declaration("config", AgentAssetCategory::Mcp, "server");
        let higher = run.declaration("workspace-config", AgentAssetCategory::Mcp, "server");
        let asset = codex_mcp_asset(&run.inventory, "server");
        assert_identity_set(asset, &[lower.clone(), higher.clone()]);
        let roles = run
            .inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.native_id == "server")
            .map(|declaration| (declaration.id.clone(), declaration.role))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(roles[&lower], AgentAssetDeclarationRole::StateOverlay);
        assert_eq!(roles[&higher], AgentAssetDeclarationRole::Definition);
        assert_eq!(
            asset.resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert_eq!(asset.declared_state, AgentAssetState::Enabled);
        assert_eq!(asset.effective_state, AgentAssetState::Enabled);
        assert_eq!(asset.resolution.terminal, None);
        assert_eq!(asset.resolution.control_source, None);
        let draft = run.draft("mcp:server");
        assert_eq!(
            draft.state_proof.declared,
            DeclaredProof::Definition {
                declaration_ids: vec![higher.clone()],
                selected_id: higher,
            }
        );
        assert_eq!(draft.state_proof.effective, EffectiveProof::Intrinsic);
        run.assert_private_values_redacted();
    }
    assert_same_rows_and_proofs(&first, &reversed);
}

#[test]
fn codex_native_invalid_merge_uses_all_config_inputs_without_policy_substitution() {
    let fixture = NativeFixture::new("codex-native-invalid-merge");
    let files = vec![
        (
            "config",
            with_status("[mcp_servers.server]\ncommand='bh-private-runner'\nenabled=true"),
        ),
        (
            "workspace-config",
            b"[mcp_servers.server]\nargs='bh-private-invalid-array'".to_vec(),
        ),
        (
            "system-requirements",
            b"[mcp_servers.server]\nidentity={command='bh-private-other'}".to_vec(),
        ),
    ];
    let first = fixture.run::<false>(files.clone(), []);
    let reversed = fixture.run::<true>(files, []);
    for run in [&first, &reversed] {
        assert_eq!(run.inventory.assets.len(), 2);
        assert_eq!(
            run.inventory.declarations.len(),
            5 + NATIVE_CONFIG_CONTROL_COUNT
        );
        run.assert_status_neighbor();
        let user = run.declaration("config", AgentAssetCategory::Mcp, "server");
        let workspace = run.declaration("workspace-config", AgentAssetCategory::Mcp, "server");
        let policy = run.declaration(
            "system-requirements",
            AgentAssetCategory::Mcp,
            "policy:requirements:server",
        );
        let asset = codex_mcp_asset(&run.inventory, "server");
        assert_identity_set(asset, &[user.clone(), workspace.clone(), policy.clone()]);
        assert_eq!(
            asset.resolution.relation,
            AgentAssetResolutionRelation::Merged
        );
        assert_eq!(asset.declared_state, AgentAssetState::Unknown);
        assert_eq!(asset.effective_state, AgentAssetState::Unknown);
        assert_eq!(
            asset.resolution.terminal,
            Some(AgentAssetResolutionTerminal::Unknown)
        );
        assert_eq!(asset.resolution.control_source, None);
        assert_eq!(
            run.inventory
                .declarations
                .iter()
                .filter(|declaration| {
                    declaration.native_id == "server"
                        && declaration.role == AgentAssetDeclarationRole::Definition
                })
                .count(),
            2
        );
        let draft = run.draft("mcp:server");
        assert_eq!(
            draft.state_proof.declared,
            DeclaredProof::Unknown {
                cause: UnknownCause::InvalidNativeMerge,
                evidence: evidence(&[user, workspace]),
            }
        );
        assert_terminal(draft, TerminalCause::DeclaredUnknown, &[]);
        assert!(run.inventory.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Toml,
                ..
            }
        )));
        run.assert_private_values_redacted();
    }
    assert_same_rows_and_proofs(&first, &reversed);
}

#[test]
fn codex_native_hook_status_and_skill_rows_retain_native_proofs_after_reversal() {
    let fixture = NativeFixture::new("codex-native-non-mcp");
    let files = vec![
        (
            "config",
            with_status("[mcp_servers.normal]\ncommand='bh-private-runner'\nenabled=true"),
        ),
        (
            "workspace-config",
            b"[tui]\nstatus_line='bh-private-status-command'".to_vec(),
        ),
        (
            "hooks",
            br#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"bh-private-user-hook"}]}],"Stop":[]}}"#.to_vec(),
        ),
        ("workspace-hooks", br#"{"hooks":{"SessionStart":[]}}"#.to_vec()),
    ];
    let directories = [
        ("codex-skills", vec!["solo"]),
        ("shared-skills", vec!["duplicate"]),
        ("workspace-skills", vec!["duplicate"]),
    ];
    let first = fixture.run::<false>(files.clone(), directories.clone());
    let reversed = fixture.run::<true>(files, directories);
    for run in [&first, &reversed] {
        assert_eq!(run.inventory.assets.len(), 7);
        assert_eq!(
            run.inventory.declarations.len(),
            8 + NATIVE_CONFIG_CONTROL_COUNT
        );
        let normal = codex_mcp_asset(&run.inventory, "normal");
        assert_eq!(normal.effective_state, AgentAssetState::Enabled);
        assert_eq!(normal.resolution.terminal, None);
        assert_eq!(normal.resolution.control_source, None);
        assert_identity_set(
            normal,
            &[run.declaration("config", AgentAssetCategory::Mcp, "normal")],
        );
        {
            let (category, native_id, lower_source, higher_source, winner_state) = (
                AgentAssetCategory::StatusUi,
                "status-line",
                "config",
                "workspace-config",
                AgentAssetState::Enabled,
            );
            let rows = assets_with(&run.inventory, category, native_id);
            assert_eq!(rows.len(), 2);
            let lower = run.declaration(lower_source, category, native_id);
            let higher = run.declaration(higher_source, category, native_id);
            let winner = rows
                .iter()
                .copied()
                .find(|asset| {
                    asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner
                })
                .unwrap();
            let loser = rows
                .iter()
                .copied()
                .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
                .unwrap();
            assert_identity_set(winner, &[lower.clone(), higher.clone()]);
            assert_identity_set(loser, std::slice::from_ref(&lower));
            assert_eq!(winner.declared_state, winner_state);
            assert_eq!(winner.effective_state, winner_state);
            assert_eq!(
                winner.resolution.winner_id.as_deref(),
                Some(winner.stable_id.as_str())
            );
            assert_eq!(loser.declared_state, AgentAssetState::Enabled);
            assert_eq!(loser.effective_state, AgentAssetState::Shadowed);
            assert_eq!(
                loser.resolution.winner_id.as_deref(),
                Some(winner.stable_id.as_str())
            );
            for row in [winner, loser] {
                assert_eq!(row.resolution.terminal, None);
                assert_eq!(row.resolution.control_source, None);
                let draft = run
                    .capture
                    .drafts
                    .iter()
                    .find(|draft| {
                        draft.native_kind == category
                            && draft.native_id == native_id
                            && draft.resolution.relation == row.resolution.relation
                    })
                    .unwrap();
                let selected = if row.resolution.relation == AgentAssetResolutionRelation::Replaced
                {
                    &lower
                } else {
                    &higher
                };
                assert_eq!(
                    draft.state_proof.declared,
                    DeclaredProof::Definition {
                        declaration_ids: vec![selected.clone()],
                        selected_id: selected.clone()
                    }
                );
                if row.resolution.relation == AgentAssetResolutionRelation::Replaced {
                    assert!(
                        matches!(&draft.state_proof.effective, EffectiveProof::Shadowed { winner, input } if winner.category == category && winner.native_id == native_id && **input == EffectiveProof::Intrinsic)
                    );
                } else {
                    assert_eq!(draft.state_proof.effective, EffectiveProof::Intrinsic);
                }
            }
            assert_eq!(
                winner.details,
                AgentAssetDetails::StatusUi {
                    mode: AgentStatusUiMode::Command,
                    command_present: true
                }
            );
            assert_eq!(
                loser.details,
                AgentAssetDetails::StatusUi {
                    mode: AgentStatusUiMode::BuiltIn,
                    command_present: false
                }
            );
        }
        let hook_rows = run
            .inventory
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Hook)
            .collect::<Vec<_>>();
        assert_eq!(hook_rows.len(), 1);
        let hook = hook_rows[0];
        assert_eq!(
            hook.native_id,
            format!(
                "{}:session_start:0:0",
                Path::new(&run.inventory.contexts[0].config_root)
                    .join("hooks.json")
                    .display()
            )
        );
        assert_eq!(hook.declared_state, AgentAssetState::Enabled);
        assert_eq!(hook.effective_state, AgentAssetState::Unknown);
        assert_eq!(hook.trust_state, AgentTrustState::Untrusted);
        assert_eq!(
            hook.resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        let hook_definition = run.declaration("hooks", AgentAssetCategory::Hook, &hook.native_id);
        assert_identity_set(hook, std::slice::from_ref(&hook_definition));
        assert!(!hook.represented_declaration_ids.contains(&run.declaration(
            "config",
            AgentAssetCategory::Hook,
            "codex-hook-user-state"
        )));
        let draft = run
            .capture
            .drafts
            .iter()
            .find(|draft| draft.native_kind == AgentAssetCategory::Hook)
            .unwrap();
        assert_eq!(
            draft.state_proof.declared,
            DeclaredProof::NativeDefault {
                definition_id: hook_definition,
                outcome: AgentAssetDeclaredState::Enabled,
            }
        );
        assert_eq!(draft.state_proof.effective, EffectiveProof::Intrinsic);
        // The empty Stop event is part of its file's configuration index,
        // not another configured handler or another public asset.
        assert_eq!(
            run.inventory
                .hook_rule_counts
                .iter()
                .find(|count| count.agent_kind == AgentCliKind::Codex)
                .unwrap()
                .rule_count,
            Some(1),
        );
        for (native_id, sources, relation) in [
            (
                "solo",
                vec!["codex-skills"],
                AgentAssetResolutionRelation::Independent,
            ),
            (
                "duplicate",
                vec!["shared-skills", "workspace-skills"],
                AgentAssetResolutionRelation::Additive,
            ),
        ] {
            let rows = assets_with(&run.inventory, AgentAssetCategory::Skill, native_id);
            assert_eq!(rows.len(), sources.len());
            let source_keys_by_id = sources
                .iter()
                .map(|source| {
                    let source_key = skill_source_key(source, native_id);
                    (
                        run.declaration(&source_key, AgentAssetCategory::Skill, "SKILL.md"),
                        source_key,
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let ids = source_keys_by_id.keys().cloned().collect::<BTreeSet<_>>();
            assert_eq!(ids.len(), sources.len());
            for (id, source_key) in &source_keys_by_id {
                // Every row retains the full group's evidence; only its own
                // contributor identifies the Definition supplying the row.
                let matching_rows = rows
                    .iter()
                    .copied()
                    .filter(|asset| asset.resolution.contributor_ids.contains(id))
                    .collect::<Vec<_>>();
                assert_eq!(matching_rows.len(), 1);
                let asset = matching_rows[0];
                assert_eq!(asset.represented_declaration_ids.len(), ids.len());
                assert_eq!(
                    asset
                        .represented_declaration_ids
                        .iter()
                        .cloned()
                        .collect::<BTreeSet<_>>(),
                    ids
                );
                assert_eq!(
                    asset.resolution.contributor_ids.as_slice(),
                    std::slice::from_ref(id)
                );
                let declaration = run
                    .inventory
                    .declarations
                    .iter()
                    .find(|declaration| &declaration.id == id)
                    .unwrap();
                assert_eq!(asset.provenance.len(), 1);
                assert_eq!(&asset.provenance[0].declaration_id, id);
                assert_eq!(asset.provenance[0].source_id, declaration.source_id);
                assert_eq!(asset.provenance[0].scope, declaration.scope);
                assert_eq!(asset.inspection_source_id, declaration.source_id);
                assert_eq!(asset.resolution.relation, relation);
                assert_eq!(asset.resolution.terminal, None);
                assert_eq!(asset.resolution.control_source, None);
                assert_eq!(asset.declared_state, AgentAssetState::Enabled);
                assert_eq!(asset.effective_state, AgentAssetState::Enabled);
                assert!(Path::new(asset.path.as_deref().unwrap())
                    .file_name()
                    .is_some_and(|name| name == "SKILL.md"));
                assert!(!asset.is_directory);
                assert_eq!(
                    asset.details,
                    AgentAssetDetails::Skill {
                        enabled: AgentAssetDeclaredState::Enabled,
                        invocation_policy: AgentSkillInvocationPolicy::Unknown
                    }
                );
                let matching_drafts = run
                    .capture
                    .drafts
                    .iter()
                    .filter(|draft| {
                        draft.native_kind == AgentAssetCategory::Skill
                            && draft.native_id == native_id
                            && draft.contributor_ids.contains(id)
                    })
                    .collect::<Vec<_>>();
                assert_eq!(matching_drafts.len(), 1);
                let draft = matching_drafts[0];
                assert_eq!(draft.represented_declaration_ids.len(), ids.len());
                assert_eq!(
                    draft
                        .represented_declaration_ids
                        .iter()
                        .cloned()
                        .collect::<BTreeSet<_>>(),
                    ids
                );
                assert_eq!(draft.contributor_ids.as_slice(), std::slice::from_ref(id));
                // Drafts retain native source keys; public rows use stable IDs.
                assert_eq!(&draft.inspection_source_id, source_key);
                assert_eq!(
                    draft.state_proof.declared,
                    DeclaredProof::NativeDefault {
                        definition_id: id.clone(),
                        outcome: AgentAssetDeclaredState::Enabled,
                    }
                );
                assert_eq!(draft.state_proof.effective, EffectiveProof::Intrinsic);
            }
        }
        run.assert_private_values_redacted();
    }
    assert_same_rows_and_proofs(&first, &reversed);
}
