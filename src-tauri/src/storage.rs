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
    let mut data = if path.exists() {
        read_app_data_file(&path)?
    } else if let Some(data) = recover_missing_app_data_file(&path)? {
        data
    } else {
        return Ok(AppData::default());
    };
    validate_app_data_schema(&data)?;
    if normalize_provider_cached_values(&mut data) {
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
    let mut data = read_app_data_file(source)?;
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

fn read_app_data_file(path: &Path) -> Result<AppData, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("读取配置失败({}): {err}", path.display()))?;
    serde_json::from_str::<AppData>(&text)
        .map_err(|err| format!("解析配置失败({}): {err}", path.display()))
}

fn recover_missing_app_data_file(path: &Path) -> Result<Option<AppData>, String> {
    let candidates = [tmp_file_path(path), backup_file_path(path)];
    let mut errors = Vec::new();

    for candidate in candidates {
        if !candidate.exists() {
            continue;
        }
        match read_app_data_file(&candidate) {
            Ok(data) => {
                fs::rename(&candidate, path).map_err(|err| {
                    format!(
                        "恢复配置失败({} -> {}): {err}",
                        candidate.display(),
                        path.display()
                    )
                })?;
                return Ok(Some(data));
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
    fn rejects_app_data_when_schema_version_is_old() {
        let data = AppData {
            schema_version: CURRENT_SCHEMA_VERSION - 1,
            ..AppData::default()
        };

        let err = validate_app_data_schema(&data).expect_err("old schema should be rejected");
        assert!(err.contains("配置结构版本不兼容"));
        assert!(err.contains(&(CURRENT_SCHEMA_VERSION - 1).to_string()));
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

        assert_eq!(recovered.schema_version, CURRENT_SCHEMA_VERSION);
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
