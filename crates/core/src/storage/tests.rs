use super::*;
use crate::models::CURRENT_SCHEMA_VERSION;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn new_app_data_uses_current_schema_version() {
    assert_eq!(AppData::default().schema_version, CURRENT_SCHEMA_VERSION);
}

#[test]
fn schema_eleven_adds_accounts_without_inventing_bindings_or_losing_credentials() {
    let mut provider = crate::models::Provider::from_input(
        crate::models::ProviderInput::default(),
        "fixture".into(),
    );
    provider.auth.session_cookie = "session=fixture-cookie".into();
    let data = AppData::new_current(vec![provider], crate::models::AppSettings::default());
    let mut value = serde_json::to_value(data).unwrap();
    value["schemaVersion"] = serde_json::json!(11);
    value.as_object_mut().unwrap().remove("loginAccounts");
    let auth = value["providers"][0]["auth"].as_object_mut().unwrap();
    auth.remove("browserBinding");
    auth.remove("credentialRevision");
    auth.remove("sessionUpdatedAt");
    let loaded = migrate_app_data(&value.to_string(), 11).unwrap();
    assert_eq!(loaded.schema_version, 12);
    assert!(loaded.login_accounts.is_empty());
    assert!(loaded.providers[0].auth.browser_binding.is_none());
    assert_eq!(loaded.providers[0].auth.credential_revision, 0);
    assert_eq!(
        loaded.providers[0].auth.session_cookie,
        "session=fixture-cookie"
    );
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

    let loaded = read_app_data_file(&target, BackupBeforeMigrate::Yes)
        .expect("old data file should migrate");

    assert!(loaded.migrated);
    assert_eq!(loaded.data.schema_version, CURRENT_SCHEMA_VERSION);
    let backup = target.with_file_name(format!(
        "{DATA_FILE_NAME}.v{}.bak",
        CURRENT_SCHEMA_VERSION - 1
    ));
    assert!(backup.exists(), "original file should be backed up");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn reading_app_data_reports_the_legacy_session_index_root() {
    let dir = unique_test_dir("legacy-index-directory");
    let target = dir.join(DATA_FILE_NAME);
    let mut value = serde_json::to_value(AppData::default()).expect("app data should serialize");
    value["settings"]["sessionIndexDirectory"] = serde_json::json!("  /tmp/balancehub-index  ");
    fs::write(
        &target,
        serde_json::to_string_pretty(&value).expect("app data should serialize"),
    )
    .expect("data file should be writable");

    let loaded =
        read_app_data_file(&target, BackupBeforeMigrate::No).expect("current schema should load");

    assert_eq!(
        loaded.legacy_session_index_root,
        Some(PathBuf::from("/tmp/balancehub-index"))
    );

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
fn parse_errors_list_only_existing_automated_backups_newest_first() {
    let dir = unique_test_dir("backup-hint");
    let target = dir.join(DATA_FILE_NAME);
    fs::write(&target, "{").expect("corrupt data file should be writable");

    let error = read_app_data_file(&target, BackupBeforeMigrate::Yes)
        .expect_err("corrupt data file should not parse");
    assert_eq!(
        with_backup_hint(&target, error.clone()),
        error,
        "没有备份时不应附加空清单"
    );

    fs::write(dir.join("data.json.bak"), "{}").expect("generic backup");
    fs::write(dir.join("data.json.v9.bak"), "{}").expect("v9 backup");
    fs::write(dir.join("data.json.v11.bak"), "{}").expect("v11 backup");
    fs::write(dir.join("unrelated.bak"), "{}").expect("unrelated backup");
    let hinted = with_backup_hint(&target, error);

    let generic = hinted.find("data.json.bak").expect("应列出通用备份");
    let v11 = hinted.find("data.json.v11.bak").expect("应列出 v11 备份");
    let v9 = hinted.find("data.json.v9.bak").expect("应列出 v9 备份");
    assert!(generic < v11 && v11 < v9, "最近的一版应排在前面");
    assert!(!hinted.contains("unrelated.bak"), "只列应用自己的备份");
    assert!(hinted.contains("重新读取"));

    // 版本过新不是旧备份能解决的问题，不能引导用户回退。
    let newer = "配置结构版本过新：当前应用只支持 schemaVersion 12，检测到 13";
    assert_eq!(with_backup_hint(&target, newer.into()), newer);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn restore_latest_backup_migrates_the_backup_and_preserves_the_corrupt_file() {
    let dir = unique_test_dir("restore-backup");
    let target = dir.join(DATA_FILE_NAME);
    let backup = dir.join(format!("data.json.v{}.bak", CURRENT_SCHEMA_VERSION - 1));
    fs::write(&target, "{ broken").expect("corrupt data file should be writable");
    let old = AppData {
        schema_version: CURRENT_SCHEMA_VERSION - 1,
        ..AppData::default()
    };
    fs::write(
        &backup,
        serde_json::to_string_pretty(&old).expect("old data should serialize"),
    )
    .expect("backup should be writable");

    let (source, restored) =
        restore_latest_backup_at(&target).expect("parse damage should restore from backup");
    assert_eq!(source, backup);
    assert_eq!(restored.schema_version, CURRENT_SCHEMA_VERSION);

    // 旧版 data.json 恢复后立即迁移成拆分文件，不再回写旧格式。
    let restored_config =
        read_config_file(&paths_from_data_path(&target).config).expect("config.json 应可读取");
    let reloaded = restored_config
        .into_app_data(&AppData::default())
        .expect("恢复后的配置应能合并");
    assert_eq!(reloaded.schema_version, CURRENT_SCHEMA_VERSION);
    let preserved = fs::read_dir(&dir)
        .expect("test directory should be readable")
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .any(|name| name.starts_with("data.json.corrupt-"));
    assert!(preserved, "损坏的原文件必须保留");

    // 正式文件已经可读，再次恢复必须拒绝，避免用旧备份静默覆盖新数据。
    let err = restore_latest_backup_at(&target).expect_err("readable config must not be replaced");
    assert!(err.contains("仍可读取"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn restore_latest_backup_reports_a_missing_backup() {
    let dir = unique_test_dir("restore-no-backup");
    let target = dir.join(DATA_FILE_NAME);
    fs::write(&target, "{ broken").expect("corrupt data file should be writable");

    let err = restore_latest_backup_at(&target).expect_err("no backup should fail clearly");
    assert!(err.contains("没有可用的自动备份"));

    let _ = fs::remove_dir_all(dir);
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

    assert_eq!(recovered.data.schema_version, CURRENT_SCHEMA_VERSION);
    assert!(
        !recovered.migrated,
        "same-version recovery needs no migration"
    );
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

#[test]
fn legacy_data_json_migrates_into_split_files_and_keeps_credentials_and_endpoints() {
    let dir = unique_test_dir("legacy-split-migration");
    let target = dir.join(DATA_FILE_NAME);
    let paths = paths_from_data_path(&target);

    let mut input = crate::models::ProviderInput::default();
    input.identity.name = "Legacy".into();
    input.identity.base_url = "https://legacy.example".into();
    input.auth.mode = crate::models::AuthMode::Session;
    input.auth.session_cookie = "session=legacy".into();
    input.auth.api_user = "7".into();
    input.request.endpoints = vec![
        crate::models::provider_domain::endpoints::ProviderEndpointOverride {
            operation: "check_in".into(),
            path: "/api/user/daily-reward".into(),
            query: Vec::new(),
            headers: Vec::new(),
        },
    ];
    let mut provider = crate::models::Provider::from_input(input, "legacy-provider".into());
    provider.quota.available = 42.0;
    let data = AppData::new_current(vec![provider], crate::models::AppSettings::default());
    fs::write(
        &target,
        serde_json::to_string_pretty(&data).expect("legacy data should serialize"),
    )
    .expect("legacy data.json should be writable");

    let loaded = read_app_data_file(&target, BackupBeforeMigrate::No).expect("legacy file parses");
    let migrated = migrate_legacy_to_split(&paths, loaded).expect("migration should succeed");

    assert!(paths.config.exists());
    assert!(paths.state.exists());
    assert!(!target.exists(), "迁移成功后旧文件由备份替代");
    let has_migration_backup = fs::read_dir(&dir)
        .expect("test directory should be readable")
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .any(|name| {
            name.starts_with(&format!("data.json.v{CURRENT_SCHEMA_VERSION}"))
                && name.ends_with(".bak")
        });
    assert!(has_migration_backup, "迁移前应保留旧文件副本");

    let config = read_config_file(&paths.config).expect("config.json should parse");
    assert_eq!(config.providers.len(), 1);
    assert_eq!(config.providers[0].identity.name, "Legacy");
    assert_eq!(config.providers[0].auth.session_cookie, "session=legacy");
    assert_eq!(config.providers[0].auth.api_user, "7");
    assert_eq!(
        config.providers[0].request.endpoints[0].path,
        "/api/user/daily-reward"
    );
    assert_eq!(migrated.providers[0].quota.available, 42.0);
    let (reloaded, _, _) = super::split::load_data_from_files(&paths, &AppData::default())
        .expect("split files should load");
    assert_eq!(reloaded.providers[0].quota.available, 42.0);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn legacy_migration_keeps_a_backup_of_the_current_file_not_an_unrelated_one() {
    let dir = unique_test_dir("legacy-stale-backup");
    let target = dir.join(DATA_FILE_NAME);
    let stale = dir.join(format!("data.json.v{CURRENT_SCHEMA_VERSION}.bak"));
    let original = serde_json::to_string_pretty(&AppData::default()).expect("serialize");
    fs::write(&stale, "{\"stale\":true}").expect("stale backup should be writable");
    fs::write(&target, &original).expect("legacy data.json should be writable");

    let loaded = read_app_data_file(&target, BackupBeforeMigrate::No).expect("legacy file parses");
    migrate_legacy_to_split(&paths_from_data_path(&target), loaded).expect("migration succeeds");

    assert!(!target.exists());
    assert_eq!(
        fs::read_to_string(&stale).expect("stale backup is preserved"),
        "{\"stale\":true}"
    );
    let fresh = fs::read_dir(&dir)
        .expect("test directory should be readable")
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            (name.starts_with(&format!("data.json.v{CURRENT_SCHEMA_VERSION}-"))
                && name.ends_with(".bak"))
            .then(|| fs::read_to_string(entry.path()).ok())
        })
        .flatten()
        .any(|text| text == original);
    assert!(fresh, "必须为当前旧文件内容建立可恢复的备份");
    let candidates = backup_candidates(&target);
    let fresh_position = candidates
        .iter()
        .position(|path| fs::read_to_string(path).is_ok_and(|text| text == original))
        .expect("恢复列表应能识别带时间戳的迁移备份");
    let stale_position = candidates
        .iter()
        .position(|path| path == &stale)
        .expect("旧备份仍应可手动选择");
    assert!(
        fresh_position < stale_position,
        "同版本内必须先尝试与当前文件一致的新备份"
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn split_restore_from_a_legacy_backup_keeps_the_current_runtime_state() {
    let dir = unique_test_dir("split-restore-legacy-keeps-state");
    let target = dir.join(DATA_FILE_NAME);
    let paths = paths_from_data_path(&target);

    let mut input = crate::models::ProviderInput::default();
    input.identity.name = "Current".into();
    input.identity.base_url = "https://restore.example".into();
    let mut provider = crate::models::Provider::from_input(input, "restore-provider".into());
    provider.quota.available = 42.0;
    let current = AppData::new_current(vec![provider], crate::models::AppSettings::default());
    super::split::save_split(&paths, &current, super::split::SaveMode::Replace)
        .expect("current split files should be written");

    let mut backup_input = crate::models::ProviderInput::default();
    backup_input.identity.name = "FromBackup".into();
    backup_input.identity.base_url = "https://restore.example".into();
    let backup_provider =
        crate::models::Provider::from_input(backup_input, "restore-provider".into());
    let backup = AppData::new_current(vec![backup_provider], crate::models::AppSettings::default());
    fs::write(
        dir.join(format!("data.json.v{CURRENT_SCHEMA_VERSION}.bak")),
        serde_json::to_string_pretty(&backup).expect("backup should serialize"),
    )
    .expect("legacy backup should be writable");
    fs::write(&paths.config, "{ broken").expect("corrupt config should be writable");

    let (source, restored) =
        restore_latest_backup_at(&target).expect("legacy backup should restore split config");
    assert!(source.ends_with(format!("data.json.v{CURRENT_SCHEMA_VERSION}.bak")));
    assert_eq!(restored.providers[0].quota.available, 42.0);

    let config = read_config_file(&paths.config).expect("restored config should parse");
    assert_eq!(config.providers[0].identity.name, "FromBackup");
    let (loaded, _, _) = super::split::load_data_from_files(&paths, &AppData::default())
        .expect("restored split files should load");
    assert_eq!(loaded.providers[0].identity.name, "FromBackup");
    assert_eq!(loaded.providers[0].quota.available, 42.0);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn split_recovers_missing_files_from_interrupted_windows_replace() {
    let dir = unique_test_dir("split-windows-recovery");
    let paths = paths_from_data_path(&dir.join(DATA_FILE_NAME));
    super::split::save_split(&paths, &AppData::default(), super::split::SaveMode::Replace)
        .expect("split files should be written");

    let config_backup = paths.config.with_file_name("config.json.bak");
    let state_backup = paths.state.with_file_name("state.json.bak");
    fs::copy(&paths.config, &config_backup).expect("config backup should be writable");
    fs::copy(&paths.state, &state_backup).expect("state backup should be writable");
    fs::remove_file(&paths.config).expect("config should be removable");
    fs::remove_file(&paths.state).expect("state should be removable");

    recover_missing_split_file(&paths).expect("recovery should succeed");
    assert!(paths.config.exists());
    assert!(paths.state.exists());
    assert!(!config_backup.exists());
    assert!(!state_backup.exists());
    read_config_file(&paths.config).expect("recovered config should parse");

    let _ = fs::remove_dir_all(dir);
}
