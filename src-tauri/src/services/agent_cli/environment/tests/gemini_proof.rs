//! Gemini 0.59 local policy identity and complete overlay evidence regressions.
use super::*;
use crate::models::{AgentAssetPolicyReference, AgentEnvironmentInventory};
use crate::services::agent_cli::contracts::{
    AgentAssetAssessmentRequest, AgentAssetAssessmentResult, AgentAssetAssessmentSubject,
    AgentAssetAssessmentTarget, AgentAssetDeclaredStateProofDraft,
    AgentAssetDeclaredUnknownCauseDraft, AgentAssetEffectiveStateProofDraft, AgentAssetParser,
    AgentAssetResolver, AgentAssetStateOverlayScopeDraft,
};

fn adapter(parser: AgentAssetParser, resolver: AgentAssetResolver) -> EnvironmentAdapter {
    EnvironmentAdapter::with_pipeline(
        |request, output| {
            definition(AgentCliKind::Gemini)
                .environment()
                .discover_contexts(request, output)
        },
        |request, output| {
            definition(AgentCliKind::Gemini)
                .environment()
                .discover_sources(request, output)
        },
        Some(|request, output| {
            definition(AgentCliKind::Gemini)
                .environment()
                .discover_follow_up_sources(request, output)
        }),
        "gemini-local-policy-fixture",
        parser,
        resolver,
        definition(AgentCliKind::Gemini)
            .environment()
            .state_assessor(),
    )
}

fn native_inventory(
    root: &Path,
    files: impl IntoIterator<Item = (&'static str, Vec<u8>)>,
    environment: EnvironmentAdapter,
) -> AgentEnvironmentInventory {
    let (snapshots, _) = ClaudeFixtureSnapshotPort::new(files, [], []);
    let mut installation = fixture_installation();
    installation.agent_kind = AgentCliKind::Gemini;
    let (installations, _) = FakeInstallationPort::new(vec![installation]);
    let definitions = [AgentCliDefinition {
        environment,
        ..*definition(AgentCliKind::Gemini)
    }];
    build_inventory_with(
        InventoryInput {
            home: root,
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
    .unwrap()
}

#[test]
fn gemini_native_mcp_ids_cannot_replace_policy_evidence() {
    let root = test_root("gemini-policy-field-identities");
    fs::create_dir_all(root.join(".gemini")).unwrap();
    let names = ["mcp.allowed", "mcp.excluded", "admin.root", "neighbor"];
    let servers = names
        .iter()
        .map(|name| {
            (
                (*name).to_owned(),
                serde_json::json!({"command": "fixture-runner"}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let inventory = native_inventory(
        &root,
        [(
            "settings",
            serde_json::to_vec(&serde_json::json!({
                "mcp": {"allowed": [], "excluded": []},
                "mcpServers": servers
            }))
            .unwrap(),
        )],
        *definition(AgentCliKind::Gemini).environment(),
    );
    assert_eq!(inventory.assets.len(), 4);
    assert_eq!(inventory.declarations.len(), 6);
    assert_eq!(
        inventory
            .assets
            .iter()
            .map(|asset| (asset.category, asset.native_id.as_str()))
            .collect::<BTreeSet<_>>(),
        names
            .into_iter()
            .map(|name| (AgentAssetCategory::Mcp, name))
            .collect()
    );
    assert_no_structural_projection_diagnostics(&inventory);
    let allowed = inventory
        .declarations
        .iter()
        .find(|declaration| {
            declaration.native_id == "mcp.allowed"
                && declaration.role == AgentAssetDeclarationRole::PolicyOverlay
        })
        .unwrap();
    assert_eq!(
        inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.role == AgentAssetDeclarationRole::Definition)
            .count(),
        4
    );
    assert_eq!(
        inventory
            .declarations
            .iter()
            .filter(|declaration| {
                declaration.native_id == "admin"
                    && declaration.role == AgentAssetDeclarationRole::PolicyOverlay
            })
            .count(),
        0
    );
    assert_eq!(
        inventory
            .declarations
            .iter()
            .map(|declaration| declaration.id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        6
    );
    for asset in &inventory.assets {
        assert_eq!(asset.declared_state, AgentAssetState::Enabled);
        assert_eq!(asset.effective_state, AgentAssetState::Blocked);
        assert_eq!(
            asset.resolution.terminal,
            Some(AgentAssetResolutionTerminal::PolicyBlocked)
        );
        assert_eq!(
            asset.resolution.control_source,
            Some(AgentAssetPolicyReference::Declaration {
                declaration_id: allowed.id.clone(),
            })
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn gemini_malformed_local_admin_never_creates_policy_or_server_rows() {
    let root = test_root("gemini-local-admin-ignored");
    fs::create_dir_all(root.join(".gemini")).unwrap();
    for field in ["config", "requiredConfig"] {
        let mut settings = serde_json::json!({
            "mcpServers": {"bad": {"command": "fixture-original"}},
            "admin": {"mcp": {"enabled": false}},
            "ui": {"footer": true}
        });
        settings["admin"]["mcp"][field] = serde_json::json!({
            "phantom": {"command": "ignored-command"},
            "bad": 42
        });
        let inventory = native_inventory(
            &root,
            [("settings", serde_json::to_vec(&settings).unwrap())],
            *definition(AgentCliKind::Gemini).environment(),
        );
        assert_eq!(inventory.assets.len(), 2, "{field}: {:?}", inventory.assets);
        assert_eq!(inventory.declarations.len(), 2, "{field}");
        assert_eq!(
            inventory
                .assets
                .iter()
                .map(|asset| (asset.category, asset.native_id.as_str()))
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                (AgentAssetCategory::Mcp, "bad"),
                (AgentAssetCategory::StatusUi, "footer"),
            ])
        );
        assert!(inventory
            .declarations
            .iter()
            .all(|declaration| declaration.role == AgentAssetDeclarationRole::Definition));
        for asset in &inventory.assets {
            assert_eq!(asset.declared_state, AgentAssetState::Enabled);
            assert_eq!(asset.effective_state, AgentAssetState::Enabled);
            assert!(asset.resolution.control_source.is_none());
            assert!(asset.resolution.terminal.is_none());
        }
        assert_no_structural_projection_diagnostics(&inventory);
    }
    fs::remove_dir_all(root).unwrap();
}

fn resolve_enablement_aliases<const CONFLICT: bool>(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    let overlays = request
        .declarations
        .iter()
        .filter(|declaration| declaration.role == AgentAssetDeclarationRole::StateOverlay)
        .collect::<Vec<_>>();
    assert_eq!(overlays.len(), 3);
    assert!(overlays.iter().all(|declaration| {
        declaration.category == AgentAssetCategory::Mcp
            && declaration.native_id == "server"
            && declaration.resolution_group_key == "server"
    }));
    let overlay_ids = overlays
        .iter()
        .map(|declaration| declaration.declaration_id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(overlay_ids.len(), 3);
    let targets = [AgentAssetAssessmentTarget {
        category: AgentAssetCategory::Mcp,
        resolution_group_key: "server".to_owned(),
        exact_native_id: "server".to_owned(),
        subject: AgentAssetAssessmentSubject::Bucket,
    }];
    let sources = request
        .sources
        .iter()
        .map(|source| source.spec.clone())
        .collect::<Vec<_>>();
    let native = definition(AgentCliKind::Gemini).environment();
    let assessed = (native.state_assessor())(AgentAssetAssessmentRequest {
        context: request.context,
        targets: &targets,
        declarations: request.declarations,
        sources: &sources,
    });
    let Some(AgentAssetAssessmentResult::Assessed { state, projection }) =
        assessed.get(&targets[0])
    else {
        panic!("complete alias peers must remain supported: {assessed:?}");
    };
    assert_eq!(
        state
            .declared_members
            .iter()
            .map(|member| member.declaration_id.clone())
            .collect::<BTreeSet<_>>(),
        overlay_ids
    );
    assert_eq!(
        projection.effective,
        AgentAssetEffectiveStateProofDraft::Intrinsic
    );
    assert!(state.control.is_none());
    if CONFLICT {
        assert_eq!(state.declared_state, AgentAssetDeclaredState::Unknown);
        assert!(matches!(
            state.declared,
            AgentAssetDeclaredStateProofDraft::Unknown {
                cause: AgentAssetDeclaredUnknownCauseDraft::OverlayConflict,
                ..
            }
        ));
    } else {
        assert_eq!(state.declared_state, AgentAssetDeclaredState::Disabled);
        assert!(matches!(
            state.declared,
            AgentAssetDeclaredStateProofDraft::Overlay {
                scope: AgentAssetStateOverlayScopeDraft::ResolutionGroup,
                outcome: AgentAssetDeclaredState::Disabled,
                ..
            }
        ));
    }
    native.resolve(request, output);
}

#[test]
fn gemini_enablement_alias_peers_survive_the_collector() {
    let root = test_root("gemini-enablement-field-identities");
    fs::create_dir_all(root.join(".gemini")).unwrap();
    for (conflict, resolver) in [
        (
            false,
            resolve_enablement_aliases::<false> as AgentAssetResolver,
        ),
        (true, resolve_enablement_aliases::<true>),
    ] {
        let inventory = native_inventory(
            &root,
            [
                (
                    "settings",
                    serde_json::to_vec(&serde_json::json!({
                        "mcpServers": {"server": {"command": "fixture-runner"}},
                        "ui": {"footer": true}
                    }))
                    .unwrap(),
                ),
                (
                    "mcp-enablement",
                    serde_json::to_vec(&serde_json::json!({
                        "Server": {"enabled": conflict},
                        "server": {"enabled": false},
                        " server ": {"enabled": false}
                    }))
                    .unwrap(),
                ),
            ],
            adapter(
                |request, output| {
                    definition(AgentCliKind::Gemini)
                        .environment()
                        .parse(request, output)
                },
                resolver,
            ),
        );
        assert_eq!(inventory.assets.len(), 2, "{:?}", inventory.assets);
        assert_eq!(inventory.declarations.len(), 5);
        assert_eq!(
            inventory
                .assets
                .iter()
                .map(|asset| (asset.category, asset.native_id.as_str()))
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                (AgentAssetCategory::Mcp, "server"),
                (AgentAssetCategory::StatusUi, "footer"),
            ])
        );
        let overlays = inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.role == AgentAssetDeclarationRole::StateOverlay)
            .collect::<Vec<_>>();
        assert_eq!(overlays.len(), 3);
        assert_eq!(
            overlays
                .iter()
                .map(|declaration| declaration.declaration_key.as_str())
                .collect::<BTreeSet<_>>()
                .len(),
            3
        );
        let mcp = inventory
            .assets
            .iter()
            .find(|asset| asset.category == AgentAssetCategory::Mcp)
            .unwrap();
        let expected = if conflict {
            AgentAssetState::Unknown
        } else {
            AgentAssetState::Disabled
        };
        assert_eq!(mcp.declared_state, expected);
        assert_eq!(mcp.effective_state, expected);
        // OverlayConflict is the complete declared proof; no outer native
        // control or structural terminal caused this Unknown state.
        assert_eq!(mcp.resolution.terminal, None);
        assert!(mcp.resolution.control_source.is_none());
        let footer = inventory
            .assets
            .iter()
            .find(|asset| asset.category == AgentAssetCategory::StatusUi)
            .unwrap();
        assert_eq!(footer.effective_state, AgentAssetState::Enabled);
        assert_no_structural_projection_diagnostics(&inventory);
    }
    fs::remove_dir_all(root).unwrap();
}
