use super::*;
use test_support::{asset_request, source_request, AccessFixture};

#[test]
fn current_generation_rejects_wrong_actor_target_environment_and_workspace() {
    let fixture = AccessFixture::new("bindings");
    let registry = AgentAssetAccessRegistry::default();
    let inventory = fixture.publish(&registry, "main");
    let request = asset_request(&inventory, "main");
    assert!(registry.resolve(request).is_ok());
    let cases = [
        (
            AgentAssetAccessRequest {
                actor: "other-window",
                ..request
            },
            AgentAssetAccessErrorKind::ActorMismatch,
        ),
        (
            AgentAssetAccessRequest {
                target_id: "other-asset",
                ..request
            },
            AgentAssetAccessErrorKind::TargetMismatch,
        ),
        (
            AgentAssetAccessRequest {
                target_kind: AgentAssetAccessTargetKind::Source,
                ..request
            },
            AgentAssetAccessErrorKind::TargetMismatch,
        ),
        (
            AgentAssetAccessRequest {
                environment_id: "other-environment",
                ..request
            },
            AgentAssetAccessErrorKind::EnvironmentMismatch,
        ),
        (
            AgentAssetAccessRequest {
                workspace: Some(&fixture.root),
                ..request
            },
            AgentAssetAccessErrorKind::WorkspaceMismatch,
        ),
        (
            AgentAssetAccessRequest {
                access_id: fixture.file.to_str().unwrap(),
                ..request
            },
            AgentAssetAccessErrorKind::AccessExpired,
        ),
    ];
    for (request, expected) in cases {
        assert_eq!(registry.resolve(request).unwrap_err().kind, expected);
    }
    let source = source_request(&inventory, "main");
    assert_ne!(source.access_id, request.access_id);
    assert_eq!(
        registry
            .resolve(AgentAssetAccessRequest {
                access_id: source.access_id,
                ..request
            })
            .unwrap_err()
            .kind,
        AgentAssetAccessErrorKind::TargetMismatch
    );
}

#[test]
fn successful_refresh_expires_old_ids_but_admitted_anchor_survives() {
    let fixture = AccessFixture::new("generations");
    let registry = AgentAssetAccessRegistry::default();
    let old = fixture.publish(&registry, "main");
    let old_request = asset_request(&old, "main");
    let admitted = registry.resolve(old_request).unwrap();
    let latest = fixture.publish(&registry, "main");
    assert_ne!(
        old_request.access_id,
        asset_request(&latest, "main").access_id
    );
    assert_eq!(
        registry.resolve(old_request).unwrap_err().kind,
        AgentAssetAccessErrorKind::AccessExpired
    );
    assert!(super::super::verified_path::reopen_verified_path(&admitted.verified).is_ok());
    assert!(registry.resolve(asset_request(&latest, "main")).is_ok());
}

#[test]
fn late_old_scan_cannot_publish_over_new_scan_and_failed_scan_keeps_current_access() {
    let fixture = AccessFixture::new("publish-order");
    let registry = AgentAssetAccessRegistry::default();
    let current = fixture.publish(&registry, "main");
    let old_ticket = registry.begin_publish("main", None).unwrap();
    let newer_ticket = registry.begin_publish("main", None).unwrap();
    // The newer scan fails before publication; the already-displayed inventory remains usable.
    drop(newer_ticket);
    assert!(registry.resolve(asset_request(&current, "main")).is_ok());
    let (mut stale_inventory, evidence) = fixture.build();
    assert_eq!(
        registry
            .publish_ticket(old_ticket, &mut stale_inventory, evidence)
            .unwrap_err()
            .kind,
        AgentAssetAccessErrorKind::AccessExpired
    );
    assert!(registry.resolve(asset_request(&current, "main")).is_ok());
    let ticket = registry.begin_publish("main", None).unwrap();
    let (mut latest, evidence) = fixture.build();
    registry
        .publish_ticket(ticket, &mut latest, evidence)
        .unwrap();
    assert!(registry.resolve(asset_request(&latest, "main")).is_ok());
}

#[test]
fn window_cleanup_invalidates_published_and_in_flight_generations() {
    let fixture = AccessFixture::new("window-close");
    let registry = AgentAssetAccessRegistry::default();
    let current = fixture.publish(&registry, "main");
    let ticket = registry.begin_publish("main", None).unwrap();
    registry.remove_actor("main");
    assert_eq!(
        registry
            .resolve(asset_request(&current, "main"))
            .unwrap_err()
            .kind,
        AgentAssetAccessErrorKind::AccessExpired
    );
    let (mut inventory, evidence) = fixture.build();
    assert_eq!(
        registry
            .publish_ticket(ticket, &mut inventory, evidence)
            .unwrap_err()
            .kind,
        AgentAssetAccessErrorKind::AccessExpired
    );
}

#[test]
fn missing_or_mismatched_snapshot_evidence_never_creates_ready_access() {
    let fixture = AccessFixture::new("missing-evidence");
    let registry = AgentAssetAccessRegistry::default();
    let (mut inventory, mut evidence) = fixture.build();
    let source_id = inventory
        .assets
        .iter()
        .find(|asset| asset.native_id == "fixture")
        .unwrap()
        .inspection_source_id
        .clone();
    evidence.retain(|evidence| evidence.source_id != source_id);
    registry.publish("main", &mut inventory, evidence).unwrap();
    let source = inventory
        .sources
        .iter()
        .find(|source| source.id == source_id)
        .unwrap();
    assert!(matches!(
        source.access,
        AgentAssetAccess::Unavailable {
            reason: AgentAssetAccessUnavailableReason::SnapshotUnavailable
        }
    ));
    assert!(source
        .actions
        .iter()
        .filter(|action| matches!(
            action.action,
            AgentAssetActionKind::Preview
                | AgentAssetActionKind::Open
                | AgentAssetActionKind::Reveal
        ))
        .all(|action| !action.available));
}

#[test]
fn repeated_publication_retains_only_bounded_current_generations_and_shared_stamps() {
    let fixture = AccessFixture::new("bounded-registry");
    let registry = AgentAssetAccessRegistry::default();
    let (inventory, evidence) = fixture.build();
    for index in 0..64 {
        let mut current = inventory.clone();
        registry
            .publish(&format!("window-{index}"), &mut current, evidence.clone())
            .unwrap();
    }
    let state = registry.state.lock().unwrap();
    assert!(state.generations.len() <= MAX_GENERATIONS);
    assert!(
        state
            .generations
            .values()
            .map(|generation| generation.entries.len())
            .sum::<usize>()
            <= MAX_REGISTERED_TARGETS
    );
    assert!(state.requested.is_empty());
    let latest = state
        .generations
        .values()
        .max_by_key(|generation| generation.sequence)
        .unwrap();
    let asset = latest
        .entries
        .values()
        .find(|anchor| anchor.binding.target_kind == AgentAssetAccessTargetKind::Asset)
        .unwrap();
    let source = latest
        .entries
        .values()
        .find(|anchor| anchor.binding.target_id == asset.binding.inspection_source_id)
        .unwrap();
    assert!(Arc::ptr_eq(&asset.verified, &source.verified));
}
