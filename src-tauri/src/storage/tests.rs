use super::*;
use crate::models::CURRENT_SCHEMA_VERSION;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn new_app_data_uses_current_schema_version() {
    assert_eq!(AppData::default().schema_version, CURRENT_SCHEMA_VERSION);
}

#[test]
fn rejects_app_data_when_schema_version_is_missing() {
    let data = serde_json::from_value::<AppData>(serde_json::json!({
        "providers": [],
        "settings": AppData::default().settings
    }))
    .expect("app data should deserialize");

    assert_eq!(data.schema_version, 0);
    let err = validate_app_data_schema(&data).expect_err("missing schema should be rejected");
    assert!(err.contains("schemaVersion"));
    assert!(err.contains(&CURRENT_SCHEMA_VERSION.to_string()));
    assert_eq!(data.schema_version, 0);
}

#[test]
fn migrates_app_data_from_older_schema_version() {
    let old = AppData {
        schema_version: CURRENT_SCHEMA_VERSION - 1,
        ..AppData::default()
    };
    let text = serde_json::to_string(&old).expect("app data should serialize");

    let migrated =
        migrate_app_data(&text, CURRENT_SCHEMA_VERSION - 1).expect("older schema should migrate");

    assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
    assert!(migrated.workspaces.is_empty());
    assert!(migrated.temporary_cli_preferences.is_empty());
    assert!(migrated.settings.session_index_enabled);
    assert_eq!(migrated.settings.session_index_max_size_mib, 64);
}

#[test]
fn schema_eight_migration_adds_session_index_settings() {
    let old = AppData {
        schema_version: 8,
        ..AppData::default()
    };
    let mut value = serde_json::to_value(old).expect("app data should serialize");
    let settings = value["settings"]
        .as_object_mut()
        .expect("settings should be an object");
    settings.remove("sessionIndexEnabled");
    settings.remove("sessionIndexDirectory");
    settings.remove("sessionIndexMaxSizeMiB");

    let migrated = migrate_app_data(
        &serde_json::to_string(&value).expect("legacy app data should serialize"),
        8,
    )
    .expect("schema eight should migrate");

    assert!(migrated.settings.session_index_enabled);
    assert!(migrated.settings.session_index_directory.is_empty());
    assert_eq!(migrated.settings.session_index_max_size_mib, 64);
}

#[test]
fn schema_nine_migration_adds_local_key_identity_and_moves_cli_preference() {
    let old = AppData {
        schema_version: 9,
        ..AppData::default()
    };
    let mut value = serde_json::to_value(old).expect("app data should serialize");
    let mut provider = crate::models::Provider::from_input(
        crate::models::ProviderInput::default(),
        "provider-test".to_string(),
    );
    provider.identity.name = "Test".to_string();
    provider.identity.base_url = "https://example.com".to_string();
    provider.auth.api_key = "sk-local".to_string();
    provider.auth.api_key_options = vec![crate::models::ProviderApiKeyOption {
        name: "备用".to_string(),
        key: "sk-local".to_string(),
        token_id: "legacy-token".to_string(),
        masked_key: "sk-l********ocal".to_string(),
        key_available: true,
        ..crate::models::ProviderApiKeyOption::default()
    }];
    let mut provider_value = serde_json::to_value(provider).expect("provider should serialize");
    provider_value["auth"]["apiKeyOptions"][0]
        .as_object_mut()
        .expect("api key option should be an object")
        .remove("localId");
    provider_value["auth"]["apiKeyOptions"][0]
        .as_object_mut()
        .expect("api key option should be an object")
        .remove("localName");
    value["providers"] = serde_json::json!([provider_value]);
    value["temporaryCliPreferences"] = serde_json::json!([{
        "providerId": "provider-test",
        "cliKind": "codex",
        "apiKeyTokenId": "legacy-token",
        "model": "",
        "workspacePath": "/tmp"
    }]);

    let mut migrated = migrate_app_data(
        &serde_json::to_string(&value).expect("legacy app data should serialize"),
        9,
    )
    .expect("schema nine should migrate");

    // Migration adds the fields; the normal load/import validation pass owns
    // deterministic local-id generation and legacy token-id preference repair.
    assert!(normalize_provider_cached_values(&mut migrated));

    assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(migrated.providers[0].auth.api_key_options.len(), 1);
    assert!(!migrated.providers[0].auth.api_key_options[0]
        .local_id
        .is_empty());
    assert_eq!(
        migrated.temporary_cli_preferences[0].api_key_local_id,
        migrated.providers[0].auth.api_key_options[0].local_id
    );
}

#[test]
fn schema_ten_migration_adds_an_empty_provider_remark() {
    let mut old = AppData {
        schema_version: 10,
        ..AppData::default()
    };
    old.providers.push(crate::models::Provider::from_input(
        crate::models::ProviderInput::default(),
        "provider-test".to_string(),
    ));
    let mut value = serde_json::to_value(old).expect("app data should serialize");
    value["providers"][0]["identity"]
        .as_object_mut()
        .expect("provider identity should be an object")
        .remove("remark");

    let migrated = migrate_app_data(
        &serde_json::to_string(&value).expect("legacy app data should serialize"),
        10,
    )
    .expect("schema ten should migrate");

    assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
    assert!(migrated.providers[0].identity.remark.is_empty());
}

#[test]
fn schema_four_migration_adds_password_login_fields() {
    let mut old = AppData {
        schema_version: 4,
        ..AppData::default()
    };
    old.providers.push(crate::models::Provider::from_input(
        crate::models::ProviderInput::default(),
        "provider-test".to_string(),
    ));
    let mut value = serde_json::to_value(old).expect("app data should serialize");
    let auth = value["providers"][0]["auth"]
        .as_object_mut()
        .expect("provider auth should be an object");
    auth.remove("loginUsername");
    auth.remove("loginPassword");

    let migrated = migrate_app_data(
        &serde_json::to_string(&value).expect("legacy app data should serialize"),
        4,
    )
    .expect("schema four should migrate");

    assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(migrated.providers[0].auth.login_username, "");
    assert_eq!(migrated.providers[0].auth.login_password, "");
}

#[test]
fn schema_five_migration_adds_api_key_cache_fields() {
    let mut old = AppData {
        schema_version: 5,
        ..AppData::default()
    };
    old.providers.push(crate::models::Provider::from_input(
        crate::models::ProviderInput::default(),
        "provider-test".to_string(),
    ));
    let mut value = serde_json::to_value(old).expect("app data should serialize");
    let auth = value["providers"][0]["auth"]
        .as_object_mut()
        .expect("provider auth should be an object");
    auth.remove("apiKeyTokenId");
    auth.remove("apiKeyOptions");

    let migrated = migrate_app_data(
        &serde_json::to_string(&value).expect("legacy app data should serialize"),
        5,
    )
    .expect("schema five should migrate");

    assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
    assert!(migrated.providers[0].auth.api_key_token_id.is_empty());
    assert!(migrated.providers[0].auth.api_key_options.is_empty());
}

#[test]
fn schema_six_migration_adds_newapi_protocol() {
    let mut old = AppData {
        schema_version: 6,
        ..AppData::default()
    };
    old.providers.push(crate::models::Provider::from_input(
        crate::models::ProviderInput::default(),
        "provider-test".to_string(),
    ));
    let mut value = serde_json::to_value(old).expect("app data should serialize");
    value["providers"][0]["identity"]
        .as_object_mut()
        .expect("provider identity should be an object")
        .remove("protocol");

    let migrated = migrate_app_data(
        &serde_json::to_string(&value).expect("legacy app data should serialize"),
        6,
    )
    .expect("schema six should migrate");

    assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(
        migrated.providers[0].identity.protocol,
        crate::models::ProviderProtocol::NewApi
    );
}

#[test]
fn schema_six_migration_replaces_removed_terminal_modes() {
    for removed_mode in ["auto", "systemDefault", "custom"] {
        let old = AppData {
            schema_version: 6,
            ..AppData::default()
        };
        let mut value = serde_json::to_value(old).expect("app data should serialize");
        value["settings"]["temporaryCliTerminalKind"] =
            serde_json::Value::String(removed_mode.to_string());
        value["settings"]["temporaryCliTerminalCommand"] =
            serde_json::Value::String("legacy terminal command".to_string());

        let migrated = migrate_app_data(
            &serde_json::to_string(&value).expect("legacy app data should serialize"),
            6,
        )
        .expect("removed terminal mode should migrate");

        assert_eq!(
            migrated.settings.temporary_cli_terminal_kind,
            crate::models::TemporaryCliTerminalKind::default()
        );
    }
}

#[test]
fn schema_seven_migration_moves_agent_cli_fields_into_dynamic_maps() {
    let mut old = AppData {
        schema_version: 7,
        ..AppData::default()
    };
    old.providers.push(crate::models::Provider::from_input(
        crate::models::ProviderInput::default(),
        "provider-test".to_string(),
    ));
    let mut value = serde_json::to_value(old).expect("app data should serialize");
    let settings = value["settings"]
        .as_object_mut()
        .expect("settings should be an object");
    settings.remove("agentCliPaths");
    settings.insert(
        "codexCliPath".to_string(),
        serde_json::Value::String("/opt/tools/codex".to_string()),
    );
    settings.insert(
        "claudeCliPath".to_string(),
        serde_json::Value::String("/opt/tools/claude".to_string()),
    );
    let liveness = value["providers"][0]["liveness"]
        .as_object_mut()
        .expect("provider liveness should be an object");
    liveness.remove("agentBaseUrls");
    liveness.insert(
        "openaiBaseUrl".to_string(),
        serde_json::Value::String("https://openai.example.com/v1".to_string()),
    );
    liveness.insert(
        "anthropicBaseUrl".to_string(),
        serde_json::Value::String("https://anthropic.example.com".to_string()),
    );

    let migrated = migrate_app_data(
        &serde_json::to_string(&value).expect("legacy app data should serialize"),
        7,
    )
    .expect("schema seven should migrate");

    assert_eq!(
        migrated
            .settings
            .agent_cli_paths
            .get(&crate::models::AgentCliKind::Codex)
            .map(String::as_str),
        Some("/opt/tools/codex")
    );
    assert_eq!(
        migrated
            .settings
            .agent_cli_paths
            .get(&crate::models::AgentCliKind::ClaudeCode)
            .map(String::as_str),
        Some("/opt/tools/claude")
    );
    assert_eq!(
        migrated.providers[0]
            .liveness
            .agent_base_urls
            .get(&crate::models::AgentCliKind::Codex)
            .map(String::as_str),
        Some("https://openai.example.com/v1")
    );
    assert_eq!(
        migrated.providers[0]
            .liveness
            .agent_base_urls
            .get(&crate::models::AgentCliKind::ClaudeCode)
            .map(String::as_str),
        Some("https://anthropic.example.com")
    );
}

#[test]
fn current_settings_serialize_agent_cli_paths_by_kind() {
    let mut data = AppData::default();
    data.settings.set_agent_cli_path(
        crate::models::AgentCliKind::ClaudeCode,
        "/opt/tools/claude".to_string(),
    );

    let value = serde_json::to_value(data).expect("app data should serialize");
    assert_eq!(
        value["settings"]["agentCliPaths"]["claudeCode"],
        "/opt/tools/claude"
    );
    assert!(value["settings"].get("codexCliPath").is_none());
    assert!(value["settings"].get("claudeCliPath").is_none());
}

#[test]
fn current_provider_serializes_agent_base_urls_by_kind() {
    let mut data = AppData::default();
    let mut provider = crate::models::Provider::from_input(
        crate::models::ProviderInput::default(),
        "provider-test".to_string(),
    );
    provider.liveness.agent_base_urls.insert(
        crate::models::AgentCliKind::Gemini,
        "https://gemini.example.com".to_string(),
    );
    data.providers.push(provider);

    let value = serde_json::to_value(data).expect("app data should serialize");
    assert_eq!(
        value["providers"][0]["liveness"]["agentBaseUrls"]["gemini"],
        "https://gemini.example.com"
    );
    assert!(value["providers"][0]["liveness"]
        .get("openaiBaseUrl")
        .is_none());
    assert!(value["providers"][0]["liveness"]
        .get("anthropicBaseUrl")
        .is_none());
}

#[test]
fn read_app_data_file_migrates_old_file_and_backs_up_original() {
    let dir = unique_test_dir("migrate-old");
    let target = dir.join(DATA_FILE_NAME);
    let old = AppData {
        schema_version: CURRENT_SCHEMA_VERSION - 1,
        ..AppData::default()
    };
    fs::write(
        &target,
        serde_json::to_string_pretty(&old).expect("app data should serialize"),
    )
    .expect("old data file should be writable");

    let (data, migrated) = read_app_data_file(&target, BackupBeforeMigrate::Yes)
        .expect("old data file should migrate");

    assert!(migrated);
    assert_eq!(data.schema_version, CURRENT_SCHEMA_VERSION);
    let backup = target.with_file_name(format!(
        "{DATA_FILE_NAME}.v{}.bak",
        CURRENT_SCHEMA_VERSION - 1
    ));
    assert!(backup.exists(), "original file should be backed up");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn read_app_data_file_rejects_newer_schema_version() {
    let dir = unique_test_dir("reject-newer");
    let target = dir.join(DATA_FILE_NAME);
    let newer = AppData {
        schema_version: CURRENT_SCHEMA_VERSION + 1,
        ..AppData::default()
    };
    fs::write(
        &target,
        serde_json::to_string_pretty(&newer).expect("app data should serialize"),
    )
    .expect("newer data file should be writable");

    let err = read_app_data_file(&target, BackupBeforeMigrate::Yes)
        .expect_err("newer schema should be rejected");
    assert!(err.contains("配置结构版本过新"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn migrate_rejects_unknown_version_zero() {
    let err =
        migrate_app_data("{\"providers\":[]}", 0).expect_err("version 0 has no migration path");
    assert!(err.contains("没有从 schemaVersion 0"));
}

#[test]
fn backup_legacy_file_does_not_overwrite_existing_backup() {
    let dir = unique_test_dir("backup-no-clobber");
    let target = dir.join(DATA_FILE_NAME);
    let first = backup_legacy_file(&target, 2, "original").expect("backup should be written");
    let second =
        backup_legacy_file(&target, 2, "migrated-again").expect("existing backup returned");

    assert_eq!(first, second);
    assert_eq!(
        fs::read_to_string(&first).expect("backup should exist"),
        "original"
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn replace_data_file_replaces_existing_target() {
    let dir = unique_test_dir("replace-existing");
    let target = dir.join(DATA_FILE_NAME);
    let tmp = tmp_file_path(&target);
    fs::write(&target, "old").expect("old target should be writable");
    fs::write(&tmp, "new").expect("tmp target should be writable");

    replace_data_file(&tmp, &target).expect("replace should succeed");

    assert_eq!(
        fs::read_to_string(&target).expect("target should exist"),
        "new"
    );
    assert!(!tmp.exists());

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn limited_json_writer_removes_oversized_partial_file() {
    let dir = unique_test_dir("limited-writer");
    let target = dir.join("oversized.json");
    let value = serde_json::json!({ "payload": "x".repeat(128) });

    let error = write_json_file_limited(&target, &value, 32, "写入测试配置")
        .expect_err("oversized JSON should be rejected");

    assert!(error.contains("上限"));
    assert!(!target.exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn recovers_missing_data_file_from_tmp_file() {
    let dir = unique_test_dir("recover-tmp");
    let target = dir.join(DATA_FILE_NAME);
    let tmp = tmp_file_path(&target);
    let data = AppData::default();
    fs::write(
        &tmp,
        serde_json::to_string_pretty(&data).expect("app data should serialize"),
    )
    .expect("tmp target should be writable");

    let recovered = recover_missing_app_data_file(&target)
        .expect("recovery should not fail")
        .expect("tmp file should be recovered");

    assert_eq!(recovered.0.schema_version, CURRENT_SCHEMA_VERSION);
    assert!(!recovered.1, "same-version recovery needs no migration");
    assert!(target.exists());
    assert!(!tmp.exists());

    let _ = fs::remove_dir_all(dir);
}

fn unique_test_dir(name: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "balancehub-storage-{name}-{}-{now}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("test dir should be created");
    path
}
