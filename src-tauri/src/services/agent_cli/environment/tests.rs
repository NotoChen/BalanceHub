use super::{
    inventory::{asset_stable_id, native_environment, safe_relative_path},
    path_access::{
        read_resolved_asset, redact_preview, resolve_asset_in, validate_resolved_path,
        ResolvedAsset,
    },
    versioning::{failure_backoff, latest_stable_version, version_channel, version_state},
};
use crate::{
    models::{
        AgentAssetCategory, AgentAssetScope, AgentCliKind, AgentDiscoverySource, AgentInstallation,
        AgentInstallationAvailability, AgentInstallationChannel, AgentVersionSource,
        AgentVersionState,
    },
    services::agent_cli::{contracts::AgentAssetDeclaration, definition},
};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    time::SystemTime,
};

fn test_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "balancehub-agent-env-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn template_rejects_parent_traversal() {
    assert!(safe_relative_path(Path::new("/tmp/home"), "../outside").is_none());
    assert!(safe_relative_path(Path::new("/tmp/home"), "/outside").is_none());
}

#[test]
fn preview_redacts_secret_values_but_keeps_shape() {
    let value = redact_preview("baseUrl = 'https://example.test'\napiKey = 'secret-value'");
    assert!(value.contains("baseUrl = 'https://example.test'"));
    assert!(value.contains("apiKey = <已隐藏>"));
    assert!(!value.contains("secret-value"));
}

#[test]
fn version_response_accepts_only_stable_latest() {
    let stable = serde_json::json!({"dist-tags": {"latest": "1.2.3", "next": "1.3.0-rc.1"}});
    assert_eq!(latest_stable_version(&stable).unwrap(), "1.2.3");
    let prerelease = serde_json::json!({"dist-tags": {"latest": "1.2.3-beta.1"}});
    assert!(latest_stable_version(&prerelease).is_err());
    assert!(latest_stable_version(&serde_json::json!({})).is_err());
}

#[test]
fn version_comparison_keeps_prerelease_ahead_state() {
    assert_eq!(
        version_state(Some("1.2.3"), Some("1.2.3")),
        AgentVersionState::UpToDate
    );
    assert_eq!(
        version_state(Some("1.1.9"), Some("1.2.3")),
        AgentVersionState::UpdateAvailable
    );
    assert_eq!(
        version_state(Some("1.3.0-alpha.1"), Some("1.2.3")),
        AgentVersionState::AheadOfStable
    );
    assert_eq!(
        version_state(Some("agent 1.2.3-beta.1"), Some("1.2.3")),
        AgentVersionState::UpdateAvailable
    );
    assert_eq!(
        version_state(Some("not-a-version"), Some("1.2.3")),
        AgentVersionState::Unknown
    );
    assert_eq!(
        version_channel("gemini 1.2.3-nightly.4"),
        AgentInstallationChannel::Nightly
    );
}

#[test]
fn version_failure_backoff_is_bounded() {
    assert_eq!(failure_backoff(1), std::time::Duration::from_secs(30));
    assert_eq!(failure_backoff(2), std::time::Duration::from_secs(60));
    assert_eq!(failure_backoff(20), std::time::Duration::from_secs(30 * 60));
}

#[test]
fn installation_availability_and_platform_capabilities_are_typed() {
    assert_eq!(
        serde_json::to_value(AgentInstallationAvailability::Available).unwrap(),
        "available"
    );
    let environment = native_environment();
    let serialized = serde_json::to_value(environment).unwrap();
    assert!(matches!(
        serialized["hostPlatform"].as_str(),
        Some("macos" | "linux" | "windows")
    ));
    assert_eq!(
        serialized["capabilities"],
        serde_json::json!(["readOnlyInventory", "boundedPreview"])
    );
    let installation = AgentInstallation {
        id: "installation:test".to_string(),
        environment_id: "native:test".to_string(),
        agent_kind: AgentCliKind::Codex,
        label: definition(AgentCliKind::Codex).label.to_string(),
        availability: AgentInstallationAvailability::Unavailable,
        executable_path: None,
        installed_version: None,
        discovery_source: AgentDiscoverySource::Automatic,
        channel: AgentInstallationChannel::Unknown,
        installed_version_source: AgentVersionSource::Unknown,
        latest_stable_version: None,
        latest_version_source: AgentVersionSource::Unknown,
        version_state: AgentVersionState::Unknown,
        version_checked_at: None,
        diagnostic: Some("not installed".to_string()),
    };
    let serialized = serde_json::to_value(installation).unwrap();
    assert_eq!(serialized["label"], "Codex CLI");
    assert_eq!(serialized["availability"], "unavailable");
}

#[test]
fn shared_file_categories_have_distinct_asset_ids() {
    let environment = native_environment();
    let path = PathBuf::from("/tmp/settings.json");
    let declaration = |category, native_id| AgentAssetDeclaration {
        category,
        native_id,
        label: native_id,
        path: path.clone(),
        scope: AgentAssetScope::User,
        precedence: 20,
        writable: true,
        sensitive: true,
        is_directory: false,
    };
    let config = declaration(AgentAssetCategory::Config, "settings");
    let hooks = declaration(AgentAssetCategory::Hook, "hooks");
    assert_ne!(
        asset_stable_id(&environment, AgentCliKind::ClaudeCode, &config, &path),
        asset_stable_id(&environment, AgentCliKind::ClaudeCode, &hooks, &path)
    );
}

#[test]
fn agent_adapters_declare_documented_user_and_workspace_sources() {
    let home = Path::new("/home/tester");
    let workspace = Path::new("/work/project");
    let expected = [
        (
            AgentCliKind::Codex,
            vec![
                home.join(".codex/config.toml"),
                home.join(".codex/hooks.json"),
                workspace.join(".codex/config.toml"),
                workspace.join(".codex/hooks.json"),
            ],
        ),
        (
            AgentCliKind::ClaudeCode,
            vec![
                home.join(".claude/settings.json"),
                workspace.join(".claude/settings.json"),
                workspace.join(".claude/settings.local.json"),
                workspace.join(".mcp.json"),
            ],
        ),
        (
            AgentCliKind::Gemini,
            vec![
                home.join(".gemini/settings.json"),
                workspace.join(".gemini/settings.json"),
                workspace.join(".gemini/skills"),
            ],
        ),
        (
            AgentCliKind::Grok,
            vec![
                home.join(".grok/config.toml"),
                workspace.join(".grok/config.toml"),
                workspace.join(".grok/skills"),
            ],
        ),
    ];

    for (kind, expected_paths) in expected {
        let declarations = definition(kind)
            .environment()
            .discover(home, Some(workspace));
        for path in expected_paths {
            assert!(
                declarations.iter().any(|item| item.path == path),
                "{} does not declare {}",
                kind.key(),
                path.display()
            );
        }
    }

    let codex = definition(AgentCliKind::Codex)
        .environment()
        .discover(home, Some(workspace));
    assert!(!codex
        .iter()
        .any(|item| item.path == home.join(".codex/hooks") && item.is_directory));

    let claude = definition(AgentCliKind::ClaudeCode)
        .environment()
        .discover(home, Some(workspace));
    assert!(!claude
        .iter()
        .any(|item| item.path == home.join(".claude/settings.local.json")));
}

#[test]
fn directory_children_can_be_resolved_from_their_opaque_ids() {
    let original_home = test_root("child-resolution");
    fs::create_dir_all(&original_home).unwrap();
    let home = fs::canonicalize(&original_home).unwrap();
    let skill_dir = home.join(".codex/skills/example");
    fs::create_dir_all(&skill_dir).unwrap();
    let definition = definition(AgentCliKind::Codex);
    let declaration = definition
        .environment()
        .discover(&home, None)
        .into_iter()
        .find(|item| item.native_id == "codex-skills")
        .unwrap();
    let asset_id = asset_stable_id(
        &native_environment(),
        AgentCliKind::Codex,
        &declaration,
        &skill_dir,
    );

    let resolved = resolve_asset_in(&asset_id, &home, None).unwrap();
    assert_eq!(resolved.path, skill_dir);
    assert!(resolved.is_directory);
    let _ = fs::remove_dir_all(original_home);
}

#[test]
fn declaration_sensitivity_forces_metadata_only_preview() {
    let root = test_root("sensitive-preview");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("settings.json");
    fs::write(&path, r#"{"customCredential":"must-not-cross-ipc"}"#).unwrap();
    let result = read_resolved_asset(
        "asset:test",
        ResolvedAsset {
            path,
            sensitive: true,
            is_directory: false,
        },
    )
    .unwrap();
    assert!(result.metadata_only);
    assert!(result.content.is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn non_sensitive_preview_is_bounded_and_reports_truncation() {
    let root = test_root("bounded-preview");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("config.toml");
    fs::write(&path, "x".repeat(128 * 1024 + 1)).unwrap();
    let result = read_resolved_asset(
        "asset:test",
        ResolvedAsset {
            path,
            sensitive: false,
            is_directory: false,
        },
    )
    .unwrap();
    assert!(result.truncated);
    assert_eq!(result.content.unwrap().len(), 128 * 1024);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn symlink_is_not_read() {
    let root = test_root("symlink");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let target = root.join("target.json");
    let link = root.join("link.json");
    let mut file = File::create(&target).unwrap();
    writeln!(file, "{{\"ok\":true}}").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&target, &link).unwrap();
    assert!(validate_resolved_path(&link, false).is_err());
    let _ = fs::remove_dir_all(root);
}
