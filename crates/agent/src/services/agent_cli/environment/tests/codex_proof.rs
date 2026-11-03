//! Completion witnesses are validated even when requirements allow the target.
use super::grok_proof::{
    assert_anchor_metadata, assert_target_rejected, emit_captured, native_inventory,
    native_resolve, NativeProjectionFixture, HASH_ORDER_CONTEXTS,
};
use super::*;
use crate::services::agent_cli::contracts::{
    AgentAssetParser, AgentDiagnosticEmission, AgentOutputStop, CodexRequirementsPayload,
};
use std::ops::ControlFlow;

struct CorruptRequirements<'a> {
    output: &'a mut dyn AgentParseOutput,
    fault: u8,
}

impl AgentDiagnosticOutput for CorruptRequirements<'_> {
    fn has_regular_capacity(&self) -> bool {
        self.output.has_regular_capacity()
    }
    fn emit_diagnostic(&mut self, diagnostic: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.output.emit_diagnostic(diagnostic)
    }
}

impl AgentParseOutput for CorruptRequirements<'_> {
    fn emit_declaration(
        &mut self,
        mut asset: crate::services::agent_cli::contracts::ParsedAgentAsset,
    ) -> ControlFlow<AgentOutputStop> {
        let AgentAssetNativePayload::CodexRequirements(payload) = &asset.native_payload else {
            return self.output.emit_declaration(asset);
        };
        let entry = matches!(payload, CodexRequirementsPayload::Entry(_));
        match (self.fault, entry) {
            (1, false) | (5, true) => {
                asset.declaration_id = "invalid-requirement-witness".to_owned()
            }
            (2, false) | (6, true) => asset.logical_origin.scope = AgentAssetScope::User,
            (3, false) => asset.source_key = "config".to_owned(),
            _ => {}
        }
        let duplicate = (self.fault == 4 && !entry).then(|| asset.clone());
        self.output.emit_declaration(asset)?;
        if let Some(duplicate) = duplicate {
            self.output.emit_declaration(duplicate)?;
        }
        ControlFlow::Continue(())
    }
}

fn parse<const FAULT: u8>(request: AgentAssetParseRequest<'_>, output: &mut dyn AgentParseOutput) {
    definition(AgentCliKind::Codex).environment().parse(
        request,
        &mut CorruptRequirements {
            output,
            fault: FAULT,
        },
    );
}

fn build(
    requirements: Option<&[u8]>,
    parser: AgentAssetParser,
) -> (PathBuf, crate::models::AgentEnvironmentInventory) {
    let root = test_root("codex-requirements-witness");
    fs::create_dir_all(root.join(".codex")).unwrap();
    let mut files = vec![(
        "config",
        b"[mcp_servers.server]\ncommand=\"runner\"\n[tui]\nstatus_line=[]".to_vec(),
    )];
    if let Some(requirements) = requirements {
        files.push(("system-requirements", requirements.to_vec()));
    }
    let (snapshots, _) = CodexFixtureSnapshotPort::new(files, []);
    let mut installation = fixture_installation();
    installation.agent_kind = AgentCliKind::Codex;
    let (installations, _) = FakeInstallationPort::new(vec![installation]);
    let real = definition(AgentCliKind::Codex).environment();
    let environment = EnvironmentAdapter::with_pipeline(
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
        None,
        "codex-requirements-proof-fixture",
        parser,
        |request, output| {
            definition(AgentCliKind::Codex)
                .environment()
                .resolve(request, output)
        },
        real.state_assessor(),
    );
    let definitions = [AgentCliDefinition {
        environment,
        ..*definition(AgentCliKind::Codex)
    }];
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
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
    (root, inventory)
}

#[test]
fn codex_requirements_invalid_witness_never_becomes_no_policy_or_matching_allowlist() {
    for (requirements, expected, has_entries) in [
        (None, AgentAssetState::Enabled, false),
        (
            Some(b"other=true".as_slice()),
            AgentAssetState::Enabled,
            false,
        ),
        (
            Some(b"[mcp_servers]".as_slice()),
            AgentAssetState::Blocked,
            false,
        ),
        (
            Some(b"[mcp_servers.server]\nidentity={command=\"runner\"}".as_slice()),
            AgentAssetState::Enabled,
            true,
        ),
        (
            Some(b"[mcp_servers.server]\nidentity={command=\"other\"}".as_slice()),
            AgentAssetState::Blocked,
            true,
        ),
    ] {
        let (root, baseline) = build(requirements, parse::<0>);
        assert_eq!(baseline.assets.len(), 2, "{:?}", baseline.diagnostics);
        assert_eq!(
            codex_mcp_asset(&baseline, "server").effective_state,
            expected
        );
        fs::remove_dir_all(root).unwrap();

        for (fault, parser) in [
            (1, parse::<1> as AgentAssetParser),
            (2, parse::<2>),
            (3, parse::<3>),
            (4, parse::<4>),
            (5, parse::<5>),
            (6, parse::<6>),
        ] {
            if fault >= 5 && !has_entries {
                continue;
            }
            let (root, inventory) = build(requirements, parser);
            assert_eq!(
                inventory.assets.len(),
                1,
                "fault {fault}: {:?}",
                inventory.assets
            );
            assert_eq!(inventory.assets[0].category, AgentAssetCategory::StatusUi);
            assert_eq!(
                inventory.assets[0].effective_state,
                AgentAssetState::Enabled
            );
            assert!(inventory.declarations.iter().any(|declaration| {
                declaration.native_id == "server"
                    && declaration.role == AgentAssetDeclarationRole::Definition
            }));
            assert!(
                inventory
                    .diagnostics
                    .iter()
                    .chain(
                        inventory
                            .sources
                            .iter()
                            .flat_map(|source| &source.diagnostics)
                    )
                    .chain(
                        inventory
                            .declarations
                            .iter()
                            .flat_map(|declaration| &declaration.diagnostics)
                    )
                    .any(|diagnostic| matches!(
                        diagnostic,
                        AgentAssetDiagnostic::InvalidProjection { .. }
                            | AgentAssetDiagnostic::InvalidResolution { .. }
                    )),
                "fault {fault}"
            );
            fs::remove_dir_all(root).unwrap();
        }
    }
}

fn resolve_inspection<const FORGE: bool>(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    let mut native = native_resolve(request);
    assert_eq!(native.drafts.len(), 2);
    if FORGE {
        let active = native
            .drafts
            .iter_mut()
            .find(|draft| draft.native_id == "same")
            .unwrap();
        assert!(!active.contributor_ids.is_empty());
        assert_eq!(active.inspection_source_id, "config");
        // Change only the candidate inspection target. Native declarations,
        // trust suppression and the registered assessor remain unchanged.
        active.inspection_source_id = "workspace-config".to_owned();
    }
    emit_captured(native, output);
}

#[test]
fn codex_native_suppressed_inspection_cannot_own_participating_asset() {
    let root = test_root("codex-native-suppressed-inspection");
    fs::create_dir_all(root.join("workspace/.codex")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let trust = format!(
        "[projects.{}]\ntrust_level='untrusted'\n",
        toml::Value::String(root.join("workspace").to_string_lossy().into_owned())
    );
    let files = vec![
        (
            "config",
            format!("{trust}[mcp_servers.same]\ncommand='bh-private-user'\nenabled=true\n[tui]\nstatus_line=[]").into_bytes(),
        ),
        (
            "workspace-config",
            b"[mcp_servers.same]\ncommand='bh-private-suppressed'\nenabled=false".to_vec(),
        ),
    ];
    let baseline = native_inventory(
        &root,
        AgentCliKind::Codex,
        files.clone(),
        vec![],
        resolve_inspection::<false>,
    );
    assert_eq!(baseline.assets.len(), 2);
    // Two MCP Definitions, Status UI, requirements root and user SkillRules,
    // user HookPolicy, plus PluginConfigRoot and FeaturePolicy per config source.
    assert_eq!(
        baseline.declarations.len(),
        6 + 2 * CODEX_FIXTURE_CONFIG_SOURCES.len()
    );
    assert_codex_empty_config_policy_declarations(&baseline);
    assert_eq!(
        baseline.contexts[0].trust_context,
        AgentTrustState::Untrusted
    );
    assert_no_structural_projection_diagnostics(&baseline);
    let target = codex_mcp_asset(&baseline, "same");
    let definitions = codex_mcp_declarations(&baseline, "same");
    assert_eq!(definitions.len(), 2);
    let user = definitions
        .iter()
        .find(|declaration| declaration.scope == AgentAssetScope::User)
        .unwrap();
    let workspace = definitions
        .iter()
        .find(|declaration| declaration.scope == AgentAssetScope::Workspace)
        .unwrap();
    assert_eq!(
        user.participation,
        AgentAssetResolutionParticipation::Participates
    );
    assert_eq!(
        workspace.participation,
        AgentAssetResolutionParticipation::Suppressed {
            reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
        }
    );
    assert_eq!(target.inspection_source_id, user.source_id);
    assert_eq!(target.resolution.contributor_ids, vec![user.id.clone()]);
    assert_eq!(
        target
            .represented_declaration_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([user.id.as_str(), workspace.id.as_str()])
    );
    assert_eq!(target.declared_state, AgentAssetState::Enabled);
    assert_eq!(target.effective_state, AgentAssetState::Enabled);
    assert_eq!(
        target.resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(target.resolution.terminal, None);
    assert_eq!(target.resolution.control_source, None);

    let attacked = native_inventory(
        &root,
        AgentCliKind::Codex,
        files,
        vec![],
        resolve_inspection::<true>,
    );
    assert_target_rejected(&baseline, &attacked, "same", 1);
    assert_eq!(attacked.assets[0].category, AgentAssetCategory::StatusUi);
    assert_eq!(attacked.assets[0].native_id, "status-line");
    assert_eq!(attacked.assets[0].effective_state, AgentAssetState::Enabled);

    // With no participating Definition the real strict TrustSuppressed row
    // still owns inspection through its only represented workspace Definition.
    let suppressed = native_inventory(
        &root,
        AgentCliKind::Codex,
        vec![
            (
                "config",
                format!("{trust}[tui]\nstatus_line=[]").into_bytes(),
            ),
            (
                "workspace-config",
                b"[mcp_servers.same]\ncommand='bh-private-suppressed'\nenabled=false".to_vec(),
            ),
        ],
        vec![],
        resolve_inspection::<false>,
    );
    assert_eq!(suppressed.assets.len(), 2);
    // Only the participating user MCP Definition disappeared; policy roots remain.
    assert_eq!(
        suppressed.declarations.len(),
        5 + 2 * CODEX_FIXTURE_CONFIG_SOURCES.len()
    );
    assert_codex_empty_config_policy_declarations(&suppressed);
    assert_no_structural_projection_diagnostics(&suppressed);
    let target = codex_mcp_asset(&suppressed, "same");
    let definitions = codex_mcp_declarations(&suppressed, "same");
    assert_eq!(definitions.len(), 1);
    assert_eq!(
        definitions[0].participation,
        AgentAssetResolutionParticipation::Suppressed {
            reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
        }
    );
    assert_eq!(target.inspection_source_id, definitions[0].source_id);
    assert_eq!(target.scope, AgentAssetScope::Workspace);
    assert_eq!(target.precedence, 20);
    assert_eq!(
        target.represented_declaration_ids,
        vec![definitions[0].id.clone()]
    );
    assert!(target.resolution.contributor_ids.is_empty());
    assert_eq!(target.declared_state, AgentAssetState::Unknown);
    assert_eq!(target.effective_state, AgentAssetState::Unknown);
    assert_eq!(
        target.resolution.relation,
        AgentAssetResolutionRelation::Unknown
    );
    assert_eq!(
        target.resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    assert_eq!(target.resolution.control_source, None);
    assert!(matches!(
        target.details,
        AgentAssetDetails::Mcp {
            effective_availability: AgentAssetEffectiveAvailability::TrustRequired,
            ..
        }
    ));
    assert_eq!(
        suppressed
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::StatusUi
                && asset.effective_state == AgentAssetState::Enabled)
            .count(),
        1
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_native_merged_metadata_uses_anchor_in_both_hash_orders() {
    assert!(HASH_ORDER_CONTEXTS[0].1 < HASH_ORDER_CONTEXTS[0].2);
    assert!(HASH_ORDER_CONTEXTS[1].1 > HASH_ORDER_CONTEXTS[1].2);
    for (context, user_id, workspace_id) in HASH_ORDER_CONTEXTS {
        let fixture = NativeProjectionFixture::new(
            AgentCliKind::Codex,
            context,
            b"[mcp_servers.same]\ncommand='bh-private-user'\nenabled=false\n[mcp_servers.neighbor]\ncommand='bh-private-neighbor'",
            b"[mcp_servers.same]\ncommand='bh-private-workspace'\nenabled=true",
        );
        // Three MCP Definitions and the requirements root are unchanged;
        // SkillRules, HookPolicy and each config's Plugin/feature roots are policy evidence.
        assert_eq!(
            fixture.declarations.len(),
            6 + 2 * CODEX_FIXTURE_CONFIG_SOURCES.len()
        );
        assert_codex_empty_config_policy_payloads(
            &fixture.declarations,
            CODEX_FIXTURE_CONFIG_SOURCES,
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
            assert_eq!(rows.len(), 2);
            let merged = rows.iter().find(|row| row.native_id == "same").unwrap();
            assert_eq!(
                merged.resolution.relation,
                AgentAssetResolutionRelation::Merged
            );
            assert_eq!(merged.declared_state, AgentAssetState::Enabled);
            assert_eq!(merged.effective_state, AgentAssetState::Enabled);
            assert_eq!(merged.resolution.terminal, None);
            assert_eq!(merged.resolution.control_source, None);
            assert_anchor_metadata(
                &fixture,
                merged,
                "workspace-config",
                AgentAssetScope::Workspace,
                20,
            );
            assert_eq!(
                merged
                    .represented_declaration_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([user_id, workspace_id])
            );
            assert_eq!(
                merged
                    .resolution
                    .contributor_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([user_id, workspace_id])
            );
            let neighbor = rows.iter().find(|row| row.native_id == "neighbor").unwrap();
            assert_eq!(neighbor.effective_state, AgentAssetState::Enabled);
            assert_anchor_metadata(&fixture, neighbor, "config", AgentAssetScope::User, 10);
        }
    }
}
