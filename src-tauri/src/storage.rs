mod file_io;
mod migration;
mod validation;

use crate::{limits, models::AppData};
use file_io::{
    backup_file_path, data_file_path, replace_data_file, tmp_file_path, write_json_file_limited,
};
use migration::{read_app_data_file, BackupBeforeMigrate};
use std::{fs, path::Path};
use tauri::AppHandle;
use validation::{normalize_provider_cached_values, validate_app_data_schema};

#[cfg(test)]
use migration::{backup_legacy_file, migrate_app_data};
#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
const DATA_FILE_NAME: &str = "data.json";

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
    let normalized =
        normalize_provider_cached_values(&mut data) | limits::normalize_app_data(&mut data);
    if migrated || normalized {
        save_app_data(app, &data)?;
    }
    Ok(data)
}

pub fn save_app_data(app: &AppHandle, data: &AppData) -> Result<(), String> {
    validate_app_data_schema(data)?;
    let path = data_file_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建配置目录失败({}): {error}", parent.display()))?;
    }

    // 先写完整临时文件再替换目标文件，避免崩溃/断电把配置（含 API Key）截断成半个 JSON。
    let tmp_path = tmp_file_path(&path);
    write_json_file_limited(&tmp_path, data, limits::MAX_APP_DATA_FILE_BYTES, "保存配置")?;
    replace_data_file(&tmp_path, &path)
}

pub fn import_app_data(app: &AppHandle, source: &Path) -> Result<AppData, String> {
    // 导入不会改写来源文件，无需在其旁边留迁移备份。
    let (mut data, _migrated) = read_app_data_file(source, BackupBeforeMigrate::No)?;
    validate_app_data_schema(&data)?;
    normalize_provider_cached_values(&mut data);
    limits::normalize_app_data(&mut data);
    save_app_data(app, &data)?;
    Ok(data)
}

pub fn export_app_data(target: &Path, data: &AppData) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("创建导出目录失败({}): {error}", parent.display()))?;
        }
    }

    let mut export_data = data.clone();
    validate_app_data_schema(&export_data)?;
    normalize_provider_cached_values(&mut export_data);
    limits::normalize_app_data(&mut export_data);
    write_json_file_limited(
        target,
        &export_data,
        limits::MAX_APP_DATA_FILE_BYTES,
        "导出配置",
    )
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
                fs::rename(&candidate, path).map_err(|error| {
                    format!(
                        "恢复配置失败({} -> {}): {error}",
                        candidate.display(),
                        path.display()
                    )
                })?;
                return Ok(Some(recovered));
            }
            Err(error) => errors.push(error),
        }
    }

    if errors.is_empty() {
        Ok(None)
    } else {
        Err(format!("配置文件缺失，恢复配置失败：{}", errors.join("；")))
    }
}

#[cfg(test)]
mod tests;
