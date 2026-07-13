use crate::models::{normalize_api_key, normalize_invite_link, AppData, CURRENT_SCHEMA_VERSION};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

const DATA_FILE_NAME: &str = "data.json";
const BACKUP_FILE_NAME: &str = "data.json.bak";
const TMP_FILE_NAME: &str = "data.json.tmp";

pub fn load_app_data(app: &AppHandle) -> Result<AppData, String> {
    let path = data_file_path(app)?;
    let (mut data, migrated) = if path.exists() {
        read_app_data_file(&path, BackupBeforeMigrate::Yes)?
    } else if let Some(recovered) = recover_missing_app_data_file(&path)? {
        recovered
    } else {
        return Ok(AppData::default());
    };
    validate_app_data_schema(&data)?;
    let normalized = normalize_provider_cached_values(&mut data);
    if migrated || normalized {
        save_app_data(app, &data)?;
    }
    Ok(data)
}

pub fn save_app_data(app: &AppHandle, data: &AppData) -> Result<(), String> {
    let path = data_file_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("创建配置目录失败({}): {err}", parent.display()))?;
    }

    let text =
        serde_json::to_string_pretty(data).map_err(|err| format!("序列化配置失败: {err}"))?;

    // 先写完整临时文件再替换目标文件，避免崩溃/断电把配置（含 API Key）截断成半个 JSON。
    let tmp_path = tmp_file_path(&path);
    fs::write(&tmp_path, text)
        .map_err(|err| format!("保存配置失败({}): {err}", tmp_path.display()))?;
    replace_data_file(&tmp_path, &path)
}

pub fn import_app_data(app: &AppHandle, source: &Path) -> Result<AppData, String> {
    // 导入不会改写来源文件，无需在其旁边留迁移备份。
    let (mut data, _migrated) = read_app_data_file(source, BackupBeforeMigrate::No)?;
    validate_app_data_schema(&data)?;
    normalize_provider_cached_values(&mut data);
    save_app_data(app, &data)?;
    Ok(data)
}

pub fn export_app_data(target: &Path, data: &AppData) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("创建导出目录失败({}): {err}", parent.display()))?;
        }
    }

    let mut export_data = data.clone();
    validate_app_data_schema(&export_data)?;
    normalize_provider_cached_values(&mut export_data);
    let text = serde_json::to_string_pretty(&export_data)
        .map_err(|err| format!("序列化导出配置失败: {err}"))?;
    fs::write(target, text).map_err(|err| format!("导出配置失败({}): {err}", target.display()))
}

fn data_file_path(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|err| format!("获取应用配置目录失败: {err}"))?;
    Ok(config_dir.join(DATA_FILE_NAME))
}

/// 迁移前是否备份原文件：主配置/恢复候选会被迁移结果覆盖，需要备份；
/// 导入的外部文件不会被改写，备份只会在来源目录留垃圾。
#[derive(Clone, Copy, PartialEq)]
enum BackupBeforeMigrate {
    Yes,
    No,
}

/// 读取并按需迁移一个配置文件。返回 `(数据, 是否发生了迁移)`，迁移过的数据
/// 由调用方决定何时落盘。
fn read_app_data_file(
    path: &Path,
    backup_mode: BackupBeforeMigrate,
) -> Result<(AppData, bool), String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("读取配置失败({}): {err}", path.display()))?;
    let stored_version = stored_schema_version(&text)
        .map_err(|err| format!("解析配置失败({}): {err}", path.display()))?;

    if stored_version == CURRENT_SCHEMA_VERSION {
        let data = serde_json::from_str::<AppData>(&text)
            .map_err(|err| format!("解析配置失败({}): {err}", path.display()))?;
        return Ok((data, false));
    }
    if stored_version > CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "配置结构版本过新：当前应用只支持 schemaVersion {CURRENT_SCHEMA_VERSION}，检测到 {stored_version}。请升级应用后再使用该配置。"
        ));
    }

    // 旧版本：先备份原文件（迁移只发生一次，这份备份就是用户数据的最后原始副本），
    // 再走逐级迁移；任何一步失败都不落盘，storage 保护态兜底，错误信息指向备份。
    let backup = if backup_mode == BackupBeforeMigrate::Yes {
        backup_legacy_file(path, stored_version, &text)
    } else {
        None
    };
    let backup_hint = backup
        .as_ref()
        .map(|backup_path| format!("，原文件已备份至 {}", backup_path.display()))
        .unwrap_or_default();
    let data = migrate_app_data(&text, stored_version).map_err(|err| {
        format!(
            "配置从 schemaVersion {stored_version} 迁移到 {CURRENT_SCHEMA_VERSION} 失败：{err}{backup_hint}"
        )
    })?;
    Ok((data, true))
}

/// 只解析 schemaVersion 字段，避免整体反序列化时 serde 默认值掩盖真实存储版本。
fn stored_schema_version(text: &str) -> Result<u32, String> {
    let value = serde_json::from_str::<serde_json::Value>(text).map_err(|err| err.to_string())?;
    Ok(value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32)
}

/// 逐级 schema 迁移。未来做破坏性结构变更时：`CURRENT_SCHEMA_VERSION += 1`，
/// 并在 [`migrate_step`] 里补一段对应旧版本的 Value 级结构调整。
fn migrate_app_data(text: &str, stored_version: u32) -> Result<AppData, String> {
    let mut value =
        serde_json::from_str::<serde_json::Value>(text).map_err(|err| err.to_string())?;
    for version in stored_version..CURRENT_SCHEMA_VERSION {
        migrate_step(version, &mut value)?;
    }
    value["schemaVersion"] = serde_json::Value::from(CURRENT_SCHEMA_VERSION);
    serde_json::from_value::<AppData>(value).map_err(|err| err.to_string())
}

/// 单级迁移：把 `version` 的结构调整为 `version + 1`。
fn migrate_step(version: u32, _data: &mut serde_json::Value) -> Result<(), String> {
    match version {
        // v1/v2 只存在于开发期，与 v3 的差异均为「新增带默认值的字段」，
        // 反序列化时 #[serde(default)] 即可兜底，无需 Value 级调整。
        1 | 2 => Ok(()),
        other => Err(format!(
            "没有从 schemaVersion {other} 出发的迁移路径，请重新初始化配置或导入新版配置"
        )),
    }
}

/// 把待迁移的原文件备份为 `data.json.v{N}.bak`。已存在同名备份时不覆盖 ——
/// 迁移失败重启会反复走到这里，第一份备份才是最原始的数据。备份失败不阻断迁移。
fn backup_legacy_file(path: &Path, version: u32, text: &str) -> Option<PathBuf> {
    let backup_path = path.with_file_name(format!("{DATA_FILE_NAME}.v{version}.bak"));
    if backup_path.exists() {
        return Some(backup_path);
    }
    fs::write(&backup_path, text).ok()?;
    Some(backup_path)
}

fn recover_missing_app_data_file(path: &Path) -> Result<Option<(AppData, bool)>, String> {
    let candidates = [tmp_file_path(path), backup_file_path(path)];
    let mut errors = Vec::new();

    for candidate in candidates {
        if !candidate.exists() {
            continue;
        }
        match read_app_data_file(&candidate, BackupBeforeMigrate::Yes) {
            Ok(recovered) => {
                fs::rename(&candidate, path).map_err(|err| {
                    format!(
                        "恢复配置失败({} -> {}): {err}",
                        candidate.display(),
                        path.display()
                    )
                })?;
                return Ok(Some(recovered));
            }
            Err(err) => errors.push(err),
        }
    }

    if errors.is_empty() {
        Ok(None)
    } else {
        Err(format!("配置文件缺失，恢复配置失败：{}", errors.join("；")))
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_data_file(tmp_path: &Path, path: &Path) -> Result<(), String> {
    fs::rename(tmp_path, path).map_err(|err| format!("写入配置失败({}): {err}", path.display()))
}

#[cfg(target_os = "windows")]
fn replace_data_file(tmp_path: &Path, path: &Path) -> Result<(), String> {
    let backup_path = backup_file_path(path);
    let had_target = path.exists();

    if had_target {
        fs::copy(path, &backup_path)
            .map_err(|err| format!("备份配置失败({}): {err}", backup_path.display()))?;
        fs::remove_file(path).map_err(|err| format!("替换配置失败({}): {err}", path.display()))?;
    }

    match fs::rename(tmp_path, path) {
        Ok(()) => {
            if had_target {
                let _ = fs::remove_file(&backup_path);
            }
            Ok(())
        }
        Err(err) => {
            if had_target && !path.exists() && backup_path.exists() {
                let _ = fs::rename(&backup_path, path);
            }
            Err(format!("写入配置失败({}): {err}", path.display()))
        }
    }
}

fn tmp_file_path(path: &Path) -> PathBuf {
    path.with_file_name(TMP_FILE_NAME)
}

fn backup_file_path(path: &Path) -> PathBuf {
    path.with_file_name(BACKUP_FILE_NAME)
}

fn validate_app_data_schema(data: &AppData) -> Result<(), String> {
    if data.schema_version != CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "配置结构版本不兼容：当前应用只支持 schemaVersion {}，检测到 {}。请重新初始化配置或导入新版配置。",
            CURRENT_SCHEMA_VERSION, data.schema_version
        ));
    }

    Ok(())
}

fn normalize_provider_cached_values(data: &mut AppData) -> bool {
    let mut changed = false;

    for provider in &mut data.providers {
        let normalized = normalize_api_key(&provider.auth.api_key);
        if normalized != provider.auth.api_key {
            provider.auth.api_key = normalized;
            changed = true;
        }

        let normalized_invite_link = normalize_invite_link(&provider.capabilities.invite_link);
        if normalized_invite_link != provider.capabilities.invite_link {
            provider.capabilities.invite_link = normalized_invite_link;
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
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

        let migrated = migrate_app_data(&text, CURRENT_SCHEMA_VERSION - 1)
            .expect("older schema should migrate");

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
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
}
