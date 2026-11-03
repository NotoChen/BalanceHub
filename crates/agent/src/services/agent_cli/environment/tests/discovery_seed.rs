//! Config-dependent discovery pays the same read budget and reuses its snapshot.
use super::*;
use std::ops::ControlFlow;

fn source(root: &Path, key: &str, name: &str) -> AgentAssetSourceSpec {
    AgentAssetSourceSpec {
        hook_definition_source: false,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: key.to_owned(),
        label: key.to_owned(),
        scope: AgentAssetScope::User,
        path: root.join(name),
        allowed_root: root.to_owned(),
        precedence: 1,
        writable: false,
        sensitive: false,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Skill],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 1,
            },
        ],
    }
}

fn discover(request: AgentSourceDiscoveryRequest<'_>, output: &mut dyn InitialSourceOutput) {
    let root = Path::new(&request.context.config_root);
    let ControlFlow::Continue(Some(AgentAssetSnapshot::File { bytes, .. })) =
        output.snapshot_initial(source(root, "seed", "a-seed.json"))
    else {
        return;
    };
    if bytes == b"seed" {
        let _ = output.emit_initial(source(root, "child", "z-child.json"));
    }
}

fn parse(request: AgentAssetParseRequest<'_>, output: &mut dyn AgentParseOutput) {
    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return;
    };
    assert_eq!(
        bytes.as_slice(),
        if request.source.native_source_key == "seed" {
            b"seed".as_slice()
        } else {
            b"child".as_slice()
        }
    );
    fixture_parse_source_key(request, output);
}

#[test]
fn discovery_seed_is_read_once_and_formally_parsed_from_the_same_snapshot() {
    let root = test_root("discovery-seed-reuse");
    fs::create_dir_all(root.join("fixture")).unwrap();
    let (snapshots, trace) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::file_bytes("seed", b"seed"),
        ExpectedSnapshot::file_bytes("child", b"child"),
    ]);
    let (installations, _) = FakeInstallationPort::new(vec![fixture_installation()]);
    let definition = fixture_definition_with_pipeline(discover, None, parse, fixture_resolve);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &[definition],
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();
    snapshots.assert_scripted_exhausted();
    let trace = trace.lock().unwrap();
    assert_eq!(trace.native_source_keys, ["seed", "child"]);
    assert_eq!(
        trace.charges.iter().map(|(_, bytes)| bytes).sum::<usize>(),
        9
    );
    assert_eq!(inventory.sources.len(), 2);
    assert_eq!(
        inventory
            .assets
            .iter()
            .map(|asset| asset.native_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["seed", "child"])
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discovery_seed_budget_exhaustion_prevents_dependent_reads_but_keeps_observed_seed() {
    let root = test_root("discovery-seed-budget");
    fs::create_dir_all(root.join("fixture")).unwrap();
    let (snapshots, trace) =
        FakeSnapshotPort::scripted([ExpectedSnapshot::file_bytes("seed", b"seed")]);
    let (installations, _) = FakeInstallationPort::new(vec![fixture_installation()]);
    let definition = fixture_definition_with_pipeline(discover, None, parse, fixture_resolve);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &[definition],
            limits: AgentAssetLimits {
                bytes_per_refresh: 4,
                ..AgentAssetLimits::DEFAULT
            },
            clock: Arc::new(ManualClock::new()),
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();
    snapshots.assert_scripted_exhausted();
    assert_eq!(trace.lock().unwrap().native_source_keys, ["seed"]);
    assert_eq!(inventory.sources.len(), 1);
    assert_eq!(inventory.assets.len(), 1);
    assert_eq!(inventory.assets[0].native_id, "seed");
    assert!(inventory.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::BytesPerRefresh,
            ..
        }
    )));
    fs::remove_dir_all(root).unwrap();
}
