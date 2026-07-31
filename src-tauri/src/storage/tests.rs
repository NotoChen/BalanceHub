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
