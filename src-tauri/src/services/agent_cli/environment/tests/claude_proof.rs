//! Real Claude parser/resolver/assessor/projector regressions. Only fixture ports
//! supply files and installations; mutated drafts never feed the assessor.
use super::*;
use crate::models::AgentAssetNativeRef;
use crate::services::agent_cli::claude::ClaudeControlPayload;
use crate::services::agent_cli::contracts::{
    AgentAssetEffectiveStateProofDraft, AgentAssetParser, AgentAssetProjectedDraft,
    AgentAssetResolver, AgentDiagnosticEmission, AgentOutputStop,
};
use std::ops::ControlFlow;

struct CorruptControl<'a> {
    output: &'a mut dyn AgentParseOutput,
    fault: u8,
}
impl AgentDiagnosticOutput for CorruptControl<'_> {
    fn has_regular_capacity(&self) -> bool {
        self.output.has_regular_capacity()
    }
    fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.output.emit_diagnostic(value)
    }
}
impl AgentParseOutput for CorruptControl<'_> {
    fn emit_declaration(
        &mut self,
        mut asset: crate::services::agent_cli::contracts::ParsedAgentAsset,
    ) -> ControlFlow<AgentOutputStop> {
        let root = asset.source_key == "settings"
            && asset.declaration_key == "control:mcp:allowedMcpServers:root";
        let entry = asset.source_key == "settings"
            && asset.declaration_key == "policy:allowedMcpServers:0:alpha";
        match (self.fault, root, entry) {
            (1, true, _) => asset.declaration_id.push_str("-incorrect"),
            (2, _, true) => asset.logical_origin.scope = AgentAssetScope::Managed,
            (3, _, true) | (8, true, _) => return ControlFlow::Continue(()),
            (5, true, _) => asset.role = AgentAssetDeclarationRole::StateOverlay,
            (6, _, true) => {
                if let AgentAssetNativePayload::ClaudeControl(ClaudeControlPayload::RuleEntry {
                    ordinal,
                    ..
                }) = &mut asset.native_payload
                {
                    *ordinal = 4;
                }
            }
            (7, true, _) => {
                if let AgentAssetNativePayload::ClaudeControl(ClaudeControlPayload::ListRoot {
                    expected_entry_count,
                    ..
                }) = &mut asset.native_payload
                {
                    *expected_entry_count = 0;
                }
            }
            _ => {}
        }
        let duplicate = (self.fault == 4 && root).then(|| asset.clone());
        self.output.emit_declaration(asset)?;
        if let Some(duplicate) = duplicate {
            self.output.emit_declaration(duplicate)?;
        }
        ControlFlow::Continue(())
    }
}
fn parse<const FAULT: u8>(request: AgentAssetParseRequest<'_>, output: &mut dyn AgentParseOutput) {
    claude_test_parse(
        request,
        &mut CorruptControl {
            output,
            fault: FAULT,
        },
    );
}

struct CorruptDraft<'a> {
    output: &'a mut dyn AgentResolveOutput,
    fault: u8,
}
impl AgentDiagnosticOutput for CorruptDraft<'_> {
    fn has_regular_capacity(&self) -> bool {
        self.output.has_regular_capacity()
    }
    fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.output.emit_diagnostic(value)
    }
}
impl AgentResolveOutput for CorruptDraft<'_> {
    fn emit_draft(&mut self, mut draft: AgentAssetProjectedDraft) -> ControlFlow<AgentOutputStop> {
        if self.fault == 5 && draft.resolution.relation == AgentAssetResolutionRelation::Replaced {
            return ControlFlow::Continue(());
        }
        if draft.native_kind == AgentAssetCategory::Mcp
            && draft.native_id == "alpha"
            && draft.resolution.relation != AgentAssetResolutionRelation::Replaced
        {
            match self.fault {
                1 => {
                    if let AgentAssetDetails::Mcp {
                        approval_state,
                        effective_availability,
                        ..
                    } = &mut draft.details
                    {
                        *approval_state = AgentMcpApprovalState::Approved;
                        *effective_availability = AgentAssetEffectiveAvailability::Available;
                    }
                    draft.effective_state = AgentAssetState::Enabled;
                }
                2 => {
                    if let AgentAssetDetails::Mcp { transport, .. } = &mut draft.details {
                        *transport = AgentMcpTransport::Http;
                    }
                }
                3 => {
                    draft.provided_by = Some(AgentAssetNativeRef {
                        category: AgentAssetCategory::StatusUi,
                        native_id: "status-line".to_owned(),
                        qualifier: None,
                    });
                }
                4 => {
                    if let AgentAssetEffectiveStateProofDraft::Terminal { evidence, .. } =
                        &mut draft.state_proof.effective
                    {
                        evidence.pop();
                    }
                }
                5 => {
                    draft.resolution.relation = AgentAssetResolutionRelation::Merged;
                    draft.resolution.winner = None;
                }
                6 => {
                    draft.state_proof.effective = AgentAssetEffectiveStateProofDraft::Intrinsic;
                    draft.resolution.terminal = None;
                    draft.resolution.control_source = None;
                    if let AgentAssetDetails::Mcp {
                        effective_availability,
                        ..
                    } = &mut draft.details
                    {
                        *effective_availability = AgentAssetEffectiveAvailability::ApprovalRequired;
                    }
                    draft.effective_state = AgentAssetState::Unknown;
                }
                _ => {}
            }
        }
        self.output.emit_draft(draft)
    }
}
fn resolve<const FAULT: u8>(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    claude_test_resolve(
        request,
        &mut CorruptDraft {
            output,
            fault: FAULT,
        },
    );
}
fn adapter(parser: AgentAssetParser, resolver: AgentAssetResolver) -> EnvironmentAdapter {
    EnvironmentAdapter::with_pipeline(
        claude_test_discover_contexts,
        claude_test_discover_sources,
        Some(claude_test_discover_follow_up_sources),
        "claude-proof-fixture",
        parser,
        resolver,
        definition(AgentCliKind::ClaudeCode)
            .environment()
            .state_assessor(),
    )
    .with_workspace_trust_authority(
        claude_test_discover_workspace_trust_sources,
        claude_test_resolve_workspace_trust,
    )
}
fn account(with_definition: bool) -> Vec<u8> {
    let mut value =
        serde_json::json!({"projects":{"__WORKSPACE__":{"hasTrustDialogAccepted":true}}});
    if with_definition {
        value["mcpServers"] = serde_json::json!({"alpha":{"command":"user-fixture-runner"}});
    }
    claude_json(value)
}
fn settings(value: serde_json::Value) -> Vec<u8> {
    let mut value = value;
    value["statusLine"] = serde_json::json!({"type":"command","command":"fixture-status-command"});
    claude_json(value)
}
fn build(
    config: serde_json::Value,
    with_user_definition: bool,
    parser: AgentAssetParser,
    resolver: AgentAssetResolver,
) -> (PathBuf, crate::models::AgentEnvironmentInventory) {
    let mut installation = fixture_installation();
    installation.agent_kind = AgentCliKind::ClaudeCode;
    let (installations, _) = FakeInstallationPort::new(vec![installation]);
    let (root, _, inventory, _) = build_claude_inventory_at_root_with_workspace_and_adapter(
        test_root("claude-state-proof"),
        [
            ("account", account(with_user_definition)),
            ("settings", settings(config)),
            (
                "workspace-mcp",
                claude_json(
                    serde_json::json!({"mcpServers":{"alpha":{"command":"fixture-runner","args":["fixture-arg"]}}}),
                ),
            ),
        ],
        [],
        [],
        ClaudeInventoryBuildOptions {
            installations: &installations,
            workspace_input: None,
            settings: None,
            test_adapter: adapter(parser, resolver),
            strict_snapshots: false,
            strict_missing_directories: &[],
        },
    );
    (root, inventory)
}
fn failures(inventory: &crate::models::AgentEnvironmentInventory) -> Vec<&AgentAssetDiagnostic> {
    inventory
        .diagnostics
        .iter()
        .chain(
            inventory
                .sources
                .iter()
                .flat_map(|source| &source.diagnostics),
        )
        .chain(
            inventory
                .declarations
                .iter()
                .flat_map(|declaration| &declaration.diagnostics),
        )
        .chain(inventory.assets.iter().flat_map(|asset| &asset.diagnostics))
        .filter(|diagnostic| {
            matches!(
                diagnostic,
                AgentAssetDiagnostic::InvalidProjection { .. }
                    | AgentAssetDiagnostic::InvalidResolution { .. }
            )
        })
        .collect()
}
fn only_status_survives(inventory: &crate::models::AgentEnvironmentInventory) {
    assert_eq!(inventory.assets.len(), 1, "{:?}", inventory.assets);
    assert_eq!(inventory.assets[0].category, AgentAssetCategory::StatusUi);
    assert_eq!(
        inventory.assets[0].effective_state,
        AgentAssetState::Enabled
    );
    assert!(inventory
        .declarations
        .iter()
        .any(|asset| asset.native_id == "alpha"
            && asset.role == AgentAssetDeclarationRole::Definition));
    assert!(!failures(inventory).is_empty());
}

#[test]
fn claude_counted_policy_completion_rejects_collector_drops_and_identity_faults() {
    let config =
        serde_json::json!({"allowedMcpServers":[{"serverName":"alpha"},{"serverName":"other"}]});
    let (root, baseline) = build(config.clone(), false, parse::<0>, resolve::<0>);
    assert_eq!(baseline.assets.len(), 2, "{:?}", failures(&baseline));
    assert_eq!(baseline.declarations.len(), 32);
    assert!(failures(&baseline).is_empty());
    assert!(matches!(
        claude_assets(&baseline, AgentAssetCategory::Mcp, "alpha")[0].details,
        AgentAssetDetails::Mcp {
            approval_state: AgentMcpApprovalState::Pending,
            effective_availability: AgentAssetEffectiveAvailability::ApprovalRequired,
            ..
        }
    ));
    remove_claude_fixture(root);
    for parser in [
        parse::<1> as AgentAssetParser,
        parse::<2>,
        parse::<3>,
        parse::<4>,
        parse::<5>,
        parse::<6>,
        parse::<7>,
        parse::<8>,
    ] {
        let (root, inventory) = build(config.clone(), false, parser, resolve::<0>);
        only_status_survives(&inventory);
        remove_claude_fixture(root);
    }
}

#[test]
fn claude_native_basis_rejects_self_consistent_approval_transport_and_relationship_changes() {
    let (root, baseline) = build(serde_json::json!({}), false, parse::<0>, resolve::<0>);
    assert_eq!(baseline.assets.len(), 2, "{:?}", failures(&baseline));
    assert_eq!(baseline.declarations.len(), 30);
    assert!(failures(&baseline).is_empty());
    remove_claude_fixture(root);
    for resolver in [
        resolve::<1> as AgentAssetResolver,
        resolve::<2>,
        resolve::<3>,
    ] {
        let (root, inventory) = build(serde_json::json!({}), false, parse::<0>, resolver);
        assert_eq!(inventory.declarations.len(), 30);
        only_status_survives(&inventory);
        remove_claude_fixture(root);
    }
}

#[test]
fn claude_complete_invalid_entities_and_native_terminal_route_are_required() {
    let config = serde_json::json!({"deniedMcpServers":[{"serverCommand":4}],"enableAllProjectMcpServers":"bad"});
    let (root, baseline) = build(config.clone(), false, parse::<0>, resolve::<0>);
    assert_eq!(baseline.assets.len(), 2, "{:?}", failures(&baseline));
    assert_eq!(baseline.declarations.len(), 31);
    let mcp = claude_assets(&baseline, AgentAssetCategory::Mcp, "alpha")[0];
    assert_eq!(mcp.effective_state, AgentAssetState::Unknown);
    assert!(mcp.resolution.control_source.is_none());
    assert!(failures(&baseline).is_empty());
    remove_claude_fixture(root);
    let (root, inventory) = build(config, false, parse::<0>, resolve::<4>);
    only_status_survives(&inventory);
    remove_claude_fixture(root);
    let config = serde_json::json!({"allowedMcpServers":[]});
    let (root, baseline) = build(config.clone(), false, parse::<0>, resolve::<0>);
    assert_eq!(baseline.assets.len(), 2, "{:?}", failures(&baseline));
    assert_eq!(
        claude_assets(&baseline, AgentAssetCategory::Mcp, "alpha")[0].effective_state,
        AgentAssetState::Blocked
    );
    assert!(failures(&baseline).is_empty());
    remove_claude_fixture(root);
    let (root, inventory) = build(config, false, parse::<0>, resolve::<6>);
    only_status_survives(&inventory);
    remove_claude_fixture(root);
}

#[test]
fn claude_native_replacement_cannot_be_relabelled_as_merge() {
    let (root, baseline) = build(serde_json::json!({}), true, parse::<0>, resolve::<0>);
    assert_eq!(baseline.assets.len(), 3, "{:?}", failures(&baseline));
    assert_eq!(baseline.declarations.len(), 31);
    let mcp = claude_assets(&baseline, AgentAssetCategory::Mcp, "alpha");
    assert_eq!(mcp.len(), 2);
    assert_eq!(
        mcp.iter()
            .filter(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
            .count(),
        1
    );
    assert!(failures(&baseline).is_empty());
    remove_claude_fixture(root);
    let (root, inventory) = build(serde_json::json!({}), true, parse::<0>, resolve::<5>);
    only_status_survives(&inventory);
    remove_claude_fixture(root);
}

#[test]
fn claude_plugin_global_failure_preserves_bool_declared_and_real_owner() {
    let mut installation = fixture_installation();
    installation.agent_kind = AgentCliKind::ClaudeCode;
    let (installations, _) = FakeInstallationPort::new(vec![installation]);
    for enabled in [true, false] {
        for malformed in [true, false] {
            let bad = if malformed {
                b"{".to_vec()
            } else {
                claude_json(serde_json::json!({"enabledPlugins":false}))
            };
            let (root, _, inventory, _) = build_claude_inventory_at_root_with_workspace_and_adapter(
                test_root("claude-plugin-proof"),
                [
                    ("account", account(false)),
                    (
                        "settings",
                        settings(serde_json::json!({"enabledPlugins":{"demo@fixture":enabled}})),
                    ),
                    (
                        "plugin-registry",
                        claude_json(
                            serde_json::json!({"version":2,"plugins":{"demo@fixture":[{"scope":"user","installPath":"/fixture/demo"}]}}),
                        ),
                    ),
                    ("managed-settings", bad),
                ],
                [],
                [],
                ClaudeInventoryBuildOptions {
                    installations: &installations,
                    workspace_input: None,
                    settings: None,
                    test_adapter: adapter(parse::<0>, resolve::<0>),
                    strict_snapshots: false,
                    strict_missing_directories: &[],
                },
            );
            assert_eq!(inventory.assets.len(), 2, "{:?}", failures(&inventory));
            // The registry now retains the observed missing package root as
            // one StateOverlay, independent of the configured plugin switch.
            assert_eq!(
                inventory
                    .declarations
                    .iter()
                    .filter(|declaration| {
                        declaration.declaration_key.starts_with("package.root:")
                            && declaration.role == AgentAssetDeclarationRole::StateOverlay
                    })
                    .count(),
                1
            );
            assert_eq!(
                inventory.declarations.len(),
                if malformed { 32 } else { 42 }
            );
            let plugin = claude_assets(&inventory, AgentAssetCategory::Plugin, "demo@fixture")[0];
            assert_eq!(
                plugin.declared_state,
                if enabled {
                    AgentAssetState::Enabled
                } else {
                    AgentAssetState::Disabled
                }
            );
            assert_eq!(plugin.effective_state, AgentAssetState::Unknown);
            assert!(matches!(
                plugin.resolution.control_source,
                Some(crate::models::AgentAssetPolicyReference::Declaration { .. })
            ));
            assert!(failures(&inventory).is_empty());
            remove_claude_fixture(root);
        }
    }
}

#[test]
fn claude_policy_ordinals_and_native_anchors_survive_complete_input_reversal() {
    let mut installation = fixture_installation();
    installation.agent_kind = AgentCliKind::ClaudeCode;
    let (installations, _) = FakeInstallationPort::new(vec![installation]);
    let files = vec![
        ("account", account(true)),
        (
            "settings",
            settings(
                serde_json::json!({"deniedMcpServers":[{"serverName":"alpha"},{"serverName":"alpha"}],"enabledPlugins":{"demo@fixture":true}}),
            ),
        ),
        (
            "workspace-mcp",
            claude_json(serde_json::json!({"mcpServers":{"alpha":{"command":"fixture-runner"}}})),
        ),
        (
            "plugin-registry",
            claude_json(
                serde_json::json!({"version":2,"plugins":{"demo@fixture":[{"scope":"user","installPath":"/fixture/demo"}]}}),
            ),
        ),
    ];
    let root = test_root("claude-proof-reversal");
    let (root, _, normal, _) = build_claude_inventory_at_root_with_workspace_and_adapter(
        root,
        files.clone(),
        [],
        [],
        ClaudeInventoryBuildOptions {
            installations: &installations,
            workspace_input: None,
            settings: None,
            test_adapter: adapter(parse::<0>, resolve::<0>),
            strict_snapshots: false,
            strict_missing_directories: &[],
        },
    );
    let reversed = claude_test_reversed_adapter();
    let (root, _, reversed, _) = build_claude_inventory_at_root_with_workspace_and_adapter(
        root,
        files,
        [],
        [],
        ClaudeInventoryBuildOptions {
            installations: &installations,
            workspace_input: None,
            settings: None,
            test_adapter: reversed,
            strict_snapshots: false,
            strict_missing_directories: &[],
        },
    );
    for inventory in [&normal, &reversed] {
        assert_eq!(inventory.assets.len(), 4, "{:?}", failures(inventory));
        assert_eq!(inventory.declarations.len(), 36);
        let roots = inventory
            .declarations
            .iter()
            .filter(|declaration| {
                declaration.declaration_key.starts_with("package.root:")
                    && declaration.role == AgentAssetDeclarationRole::StateOverlay
            })
            .collect::<Vec<_>>();
        assert_eq!(roots.len(), 1);
        assert_eq!(
            roots[0].presence,
            crate::models::AgentAssetPresence::Missing
        );
        let plugin = inventory
            .assets
            .iter()
            .find(|asset| {
                asset.category == AgentAssetCategory::Plugin && asset.native_id == "demo@fixture"
            })
            .expect("registered plugin stays visible when its package is missing");
        assert_eq!(plugin.declared_state, AgentAssetState::Enabled);
        assert_eq!(plugin.effective_state, AgentAssetState::NotInstalled);
        assert!(matches!(
            plugin.details,
            AgentAssetDetails::Plugin {
                install_state: AgentAssetInstallState::NotInstalled,
                ..
            }
        ));
        assert!(failures(inventory).is_empty());
    }
    assert!(
        claude_full_public_signature(&normal) == claude_full_public_signature(&reversed),
        "complete Claude public signature changed under reversed input"
    );
    remove_claude_fixture(root);
}
