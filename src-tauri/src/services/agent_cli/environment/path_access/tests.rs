use super::*;
use crate::services::agent_cli::environment::access_registry::test_support::{
    access_id, asset_request, source_request, AccessFixture,
};
use std::{cell::Cell, fs};

#[test]
fn real_inventory_preview_uses_one_target_bound_published_snapshot_without_execution() {
    let fixture = AccessFixture::new("real-preview");
    let sentinel = fixture.root.join("must-not-execute");
    fs::write(&fixture.file, format!(
        "[mcp_servers.fixture]\ncommand='node'\nargs=['server.js','--port','3000','--api-key','fixture-key']\nurl='https://example.invalid/mcp?transport=stdio'\n[mcp_servers.fixture.env]\nACCESS='fixture-key'\n[hooks]\ncommand='touch {}'\n", sentinel.display()
    )).unwrap();
    let registry = AgentAssetAccessRegistry::default();
    let inventory = fixture.publish(&registry, "main");
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.native_id == "fixture"
                && asset.category == crate::models::AgentAssetCategory::Mcp)
            .count(),
        1
    );
    let request = asset_request(&inventory, "main");
    let result = read_asset(&registry, request).unwrap();
    assert_eq!(result.access_id, request.access_id);
    let content = result.content.unwrap();
    assert!(content.contains("node"));
    assert!(content.contains("server.js"));
    assert!(content.contains("3000"));
    assert!(content.contains("transport=stdio"));
    assert!(!content.contains("fixture-key"));
    assert!(!content.contains("touch "));
    assert!(!sentinel.exists());
    assert_eq!(
        result.source_revision.identity,
        registry
            .resolve(request)
            .unwrap()
            .verified
            .revision()
            .identity
    );
    let source = read_source(&registry, source_request(&inventory, "main")).unwrap();
    assert_eq!(
        source.source_revision.identity,
        result.source_revision.identity
    );
    assert!(read_source(&registry, request).is_err());
}

#[test]
fn preview_rejects_same_bytes_new_source_object() {
    let fixture = AccessFixture::new("source-replaced");
    let registry = AgentAssetAccessRegistry::default();
    let inventory = fixture.publish(&registry, "main");
    let bytes = fs::read(&fixture.file).unwrap();
    fs::rename(&fixture.file, fixture.root.join("old-config.toml")).unwrap();
    fs::write(&fixture.file, &bytes).unwrap();
    let error = read_asset(&registry, asset_request(&inventory, "main")).unwrap_err();
    assert_eq!(error.kind, AgentAssetAccessErrorKind::SourceChanged);
}

#[test]
fn preview_rejects_same_bytes_new_allowed_root_object() {
    let fixture = AccessFixture::new("root-replaced");
    let registry = AgentAssetAccessRegistry::default();
    let inventory = fixture.publish(&registry, "main");
    let bytes = fs::read(&fixture.file).unwrap();
    fs::rename(fixture.root.join(".codex"), fixture.root.join(".codex-old")).unwrap();
    fs::create_dir(fixture.root.join(".codex")).unwrap();
    fs::write(&fixture.file, &bytes).unwrap();
    let error = read_asset(&registry, asset_request(&inventory, "main")).unwrap_err();
    assert_eq!(error.kind, AgentAssetAccessErrorKind::RootChanged);
}

#[test]
fn in_place_revision_drift_and_failed_open_never_return_raw_values() {
    let fixture = AccessFixture::new("source-modified");
    let registry = AgentAssetAccessRegistry::default();
    let inventory = fixture.publish(&registry, "main");
    fs::write(
        &fixture.file,
        "[mcp_servers.fixture]\ncommand='private-replacement-value'\n",
    )
    .unwrap();
    let error = read_asset(&registry, asset_request(&inventory, "main")).unwrap_err();
    assert_eq!(error.kind, AgentAssetAccessErrorKind::SourceChanged);
    assert!(!serde_json::to_string(&error)
        .unwrap()
        .contains("private-replacement-value"));
}

#[cfg(unix)]
#[test]
fn final_and_ancestor_symlinks_are_rejected_before_read_or_open() {
    use std::os::unix::fs::symlink;
    for ancestor in [false, true] {
        let fixture = AccessFixture::new("symlink");
        let registry = AgentAssetAccessRegistry::default();
        let inventory = fixture.publish(&registry, "main");
        if ancestor {
            fs::rename(fixture.root.join(".codex"), fixture.root.join("actual")).unwrap();
            symlink(fixture.root.join("actual"), fixture.root.join(".codex")).unwrap();
        } else {
            fs::rename(&fixture.file, fixture.root.join("actual.toml")).unwrap();
            symlink(fixture.root.join("actual.toml"), &fixture.file).unwrap();
        }
        let request = asset_request(&inventory, "main");
        assert_eq!(
            read_asset(&registry, request).unwrap_err().kind,
            AgentAssetAccessErrorKind::SymlinkRejected
        );
        let anchor = registry.resolve(request).unwrap();
        let risks = anchor
            .require_action(AgentAssetActionKind::Open)
            .unwrap()
            .risks
            .clone();
        let calls = Cell::new(0);
        let error = open_asset(
            &registry,
            request,
            AgentAssetOpenTarget::Asset,
            &risks,
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, AgentAssetAccessErrorKind::SymlinkRejected);
        assert_eq!(calls.get(), 0);
    }
}

#[test]
fn each_external_action_requires_exact_risks_and_a_current_target() {
    let fixture = AccessFixture::new("open-confirmation");
    let registry = AgentAssetAccessRegistry::default();
    let inventory = fixture.publish(&registry, "main");
    let request = source_request(&inventory, "main");
    let anchor = registry.resolve(request).unwrap();
    let required = anchor
        .require_action(AgentAssetActionKind::Open)
        .unwrap()
        .risks
        .clone();
    assert!(required.contains(&AgentAssetAccessRisk::ExternalPathnameRace));
    assert!(required.contains(&AgentAssetAccessRisk::RawSensitiveContent));
    let calls = Cell::new(0);
    for accepted in [
        Vec::new(),
        vec![AgentAssetAccessRisk::ExternalPathnameRace],
        vec![required[0], required[0]],
    ] {
        let error = open_source(
            &registry,
            request,
            AgentAssetOpenTarget::Asset,
            &accepted,
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, AgentAssetAccessErrorKind::ConfirmationRequired);
    }
    let wrong = AgentAssetAccessRequest {
        target_id: "wrong-source",
        ..request
    };
    assert_eq!(
        open_source(
            &registry,
            wrong,
            AgentAssetOpenTarget::Asset,
            &required,
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            }
        )
        .unwrap_err()
        .kind,
        AgentAssetAccessErrorKind::TargetMismatch
    );
    assert_eq!(calls.get(), 0);
    for target in [AgentAssetOpenTarget::Asset, AgentAssetOpenTarget::Reveal] {
        open_source(&registry, request, target, &required, |path| {
            assert_eq!(path, fixture.file);
            calls.set(calls.get() + 1);
            Ok(())
        })
        .unwrap();
    }
    assert_eq!(calls.get(), 2);
    assert_eq!(
        open_source(
            &registry,
            request,
            AgentAssetOpenTarget::Asset,
            &required,
            |_| Err("private-opener-output".to_string())
        )
        .unwrap_err()
        .kind,
        AgentAssetAccessErrorKind::ExternalOpenFailed
    );
}

#[test]
fn external_callback_runs_while_the_verified_source_handle_is_held() {
    let fixture = AccessFixture::new("open-guard");
    let registry = AgentAssetAccessRegistry::default();
    let inventory = fixture.publish(&registry, "main");
    let anchor = registry
        .resolve(source_request(&inventory, "main"))
        .unwrap();
    with_verified_external_path(&anchor, |guard, path| {
        assert_eq!(path, fixture.file);
        let bytes = guard.read_file_bounded(4096).unwrap();
        assert!(anchor.verified.matches_bytes(&bytes));
        guard.revalidate().unwrap();
        #[cfg(windows)]
        assert!(fs::rename(path, fixture.root.join("held.toml")).is_err());
        Ok(())
    })
    .unwrap();
    fs::rename(&fixture.file, fixture.root.join("released.toml")).unwrap();
}

#[test]
fn malformed_or_unknown_source_preview_is_metadata_only() {
    let fixture = AccessFixture::new("metadata-preview");
    let registry = AgentAssetAccessRegistry::default();
    for text in [
        "not a TOML document: private-secret",
        "token = 'private-secret'\nunterminated = [",
    ] {
        fs::write(&fixture.file, text).unwrap();
        let inventory = fixture.publish(&registry, "main");
        let source = inventory
            .sources
            .iter()
            .find(|source| Path::new(&source.path) == fixture.file)
            .unwrap();
        let request = AgentAssetAccessRequest {
            actor: "main",
            environment_id: &inventory.environment.id,
            workspace: None,
            target_id: &source.id,
            access_id: access_id(&source.access),
            target_kind: AgentAssetAccessTargetKind::Source,
        };
        let result = read_source(&registry, request).unwrap();
        assert!(result.metadata_only);
        assert!(result.content.is_none());
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("private-secret"));
        assert_eq!(
            result.diagnostics,
            vec![AgentAssetReadDiagnostic::InvalidDocumentMetadataOnly]
        );
    }
}
