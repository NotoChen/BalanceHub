mod file_io;
mod legacy_session_index;
mod migration;
mod validation;

use crate::{limits, models::AppData};
use file_io::{
    backup_file_path, data_file_path, lock_data_directory, replace_data_file, tmp_file_path,
    write_json_file_limited, DATA_FILE_NAME,
};
use migration::{read_app_data_file, BackupBeforeMigrate, LoadedAppData};
use std::{fs, path::Path};
use validation::{normalize_provider_cached_values, validate_app_data_schema};

#[cfg(test)]
use migration::{backup_legacy_file, migrate_app_data};
#[cfg(test)]
use std::path::PathBuf;

pub fn load_app_data() -> Result<AppData, String> {
    let path = data_file_path()?;
    lock_data_directory(&path)?;
    let LoadedAppData {
        mut data,
        migrated,
        legacy_session_index_root,
    } = if path.exists() {
        match read_app_data_file(&path, BackupBeforeMigrate::Yes) {
            Ok(loaded) => loaded,
            // 解析失败通常意味着文件被截断或被手工改坏；把同目录下应用
            // 自己留下的备份列出来，用户可以直接导入或覆盖后重新读取。
            Err(error) => return Err(with_backup_hint(&path, error)),
        }
    } else {
        // 干净目录同样走演示数据路径：开发脚本可以只用隔离根目录启动，
        // 不依赖预先复制一份安装版配置。
        recover_missing_app_data_file(&path)?.unwrap_or_default()
    };
    // 只在已有配置的安装上升级清理；全新目录不扫描缓存。开发版与安装版
    // 共用宿主缓存目录，清理又只在升级后执行一次，因此跳过调试构建，
    // 避免 dev 启动时删除安装版仍在使用的派生索引。
    if path.exists() && !cfg!(debug_assertions) {
        cleanup_legacy_session_index(legacy_session_index_root.as_deref());
    }
    validate_app_data_schema(&data)?;
    let mut normalized =
        normalize_provider_cached_values(&mut data) | limits::normalize_app_data(&mut data);
    if !migrated && !normalized {
        normalized = merge_dev_sample_on_load(&path, &mut data)?;
    }
    if migrated || normalized {
        save_app_data(&data)?;
    }
    Ok(data)
}

/// 旧版本把会话正文索引写在 `<缓存目录>/session-index` 或旧设置指定的
/// 自定义目录。索引已改为按需读取，升级后按旧命名清理这些派生文件；
/// 清理失败只影响磁盘占用，不应阻断配置加载。
fn cleanup_legacy_session_index(custom_directory: Option<&Path>) {
    let cache_directory = match crate::host::cache_dir() {
        Ok(directory) => directory,
        Err(error) => {
            log::warn!("获取缓存目录失败，跳过旧版会话检索索引清理：{error}");
            return;
        }
    };
    match legacy_session_index::cleanup(&cache_directory, custom_directory) {
        Ok(true) => log::info!("已清理旧版会话检索索引"),
        Ok(false) => {}
        Err(error) => log::warn!("旧版会话检索索引清理未完成：{error}"),
    }
}

pub fn save_app_data(data: &AppData) -> Result<(), String> {
    validate_app_data_schema(data)?;
    save_app_data_at(&data_file_path()?, data)
}

fn save_app_data_at(path: &Path, data: &AppData) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建配置目录失败({}): {error}", parent.display()))?;
    }

    // 先写完整临时文件再替换目标文件，避免崩溃/断电把配置（含 API Key）截断成半个 JSON。
    let tmp_path = tmp_file_path(path);
    write_json_file_limited(&tmp_path, data, limits::MAX_APP_DATA_FILE_BYTES, "保存配置")?;
    replace_data_file(&tmp_path, path)
}

/// 当前 data.json 已损坏时，用同目录最近的自动备份恢复。只接受解析失败的
/// 主文件：仍可读取的配置（包括版本过新）必须走显式导入，不能静默回退。
/// 原文件保留为 `data.json.corrupt-<时间戳>`，恢复成功后才写入正式文件。
pub fn restore_latest_backup() -> Result<(std::path::PathBuf, AppData), String> {
    let path = data_file_path()?;
    lock_data_directory(&path)?;
    restore_latest_backup_at(&path)
}

fn restore_latest_backup_at(path: &Path) -> Result<(std::path::PathBuf, AppData), String> {
    match read_app_data_file(path, BackupBeforeMigrate::No) {
        Ok(_) => {
            return Err("当前 data.json 仍可读取，请通过「从备份恢复」选择要导入的备份".to_string())
        }
        // 文件缺失可以由备份补回；其它读取错误（权限、版本过新）不是旧备份能解决的。
        Err(error) if path.exists() && !error.contains("解析配置失败") => {
            return Err(format!(
                "当前 data.json 不是解析损坏，无法自动恢复：{error}"
            ));
        }
        Err(_) => {}
    }
    let candidates = backup_candidates(path);
    if candidates.is_empty() {
        return Err("没有可用的自动备份".to_string());
    }
    let mut errors = Vec::new();
    for candidate in candidates {
        let loaded = match read_app_data_file(&candidate, BackupBeforeMigrate::No) {
            Ok(loaded) => loaded,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let mut data = loaded.data;
        if let Err(error) = validate_app_data_schema(&data) {
            errors.push(error);
            continue;
        }
        normalize_provider_cached_values(&mut data);
        limits::normalize_app_data(&mut data);
        preserve_corrupt_file(path)?;
        save_app_data_at(path, &data)?;
        return Ok((candidate, data));
    }
    Err(format!("自动备份均无法读取：{}", errors.join("；")))
}

/// 把损坏的正式文件改名保留，绝不直接删除。重名时追加序号，避免同一
/// 毫秒内连续恢复时覆盖上一份证据。
fn preserve_corrupt_file(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let stamp = crate::util::unix_millis();
    let mut target = path.with_file_name(format!("{DATA_FILE_NAME}.corrupt-{stamp}"));
    let mut suffix = 1;
    while target.exists() {
        target = path.with_file_name(format!("{DATA_FILE_NAME}.corrupt-{stamp}-{suffix}"));
        suffix += 1;
    }
    fs::rename(path, &target).map_err(|error| {
        format!(
            "保留损坏配置失败({} -> {}): {error}",
            path.display(),
            target.display()
        )
    })
}

pub fn import_app_data(source: &Path) -> Result<AppData, String> {
    // 导入不会改写来源文件，无需在其旁边留迁移备份。
    let mut data = read_app_data_file(source, BackupBeforeMigrate::No)?.data;
    validate_app_data_schema(&data)?;
    normalize_provider_cached_values(&mut data);
    limits::normalize_app_data(&mut data);
    save_app_data(&data)?;
    Ok(data)
}

/// 浏览器版导出：生成与文件导出完全一致的内容，交给前端下载。
pub fn export_app_data_json(data: &AppData) -> Result<String, String> {
    let mut export_data = data.clone();
    validate_app_data_schema(&export_data)?;
    normalize_provider_cached_values(&mut export_data);
    limits::normalize_app_data(&mut export_data);
    serde_json::to_string_pretty(&export_data).map_err(|error| format!("导出配置失败: {error}"))
}

/// 浏览器版导入：接收上传的配置文本，走与文件导入相同的迁移和校验。
pub fn import_app_data_json(text: &str) -> Result<AppData, String> {
    let label = Path::new("上传的配置");
    let mut data = migration::read_app_data_text(text, label, BackupBeforeMigrate::No)?.data;
    validate_app_data_schema(&data)?;
    normalize_provider_cached_values(&mut data);
    limits::normalize_app_data(&mut data);
    save_app_data(&data)?;
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

/// 开发版演示卡片：只在开发脚本开启 `BALANCEHUB_DEV_SAMPLE`、且盘上还没有演示卡片
/// 时并入一次，之后和普通配置一样只读不写。
#[cfg(debug_assertions)]
fn merge_dev_sample_on_load(path: &Path, data: &mut AppData) -> Result<bool, String> {
    if crate::dev_sample::samples_present(path) {
        return Ok(false);
    }
    Ok(crate::dev_sample::merge_on_load(data).is_some())
}

#[cfg(not(debug_assertions))]
fn merge_dev_sample_on_load(_path: &Path, _data: &mut AppData) -> Result<bool, String> {
    Ok(false)
}

/// 解析失败时附带同目录的自动备份清单。只有解析类错误才提示备份：
/// 结构版本过新或权限问题不是旧备份能解决的，列出来只会误导用户。
fn with_backup_hint(path: &Path, error: String) -> String {
    if !error.contains("解析配置失败") {
        return error;
    }
    let backups = backup_candidates(path);
    if backups.is_empty() {
        return error;
    }
    let list = backups
        .iter()
        .map(|backup| backup.display().to_string())
        .collect::<Vec<_>>()
        .join("、");
    format!(
        "{error}。同目录下检测到自动备份：{list}；可直接点击「自动备份恢复」，或通过「从备份恢复」选择文件，也可以覆盖 data.json 后重新读取"
    )
}

/// 应用自己写出的备份：`data.json.bak` 是 Windows 替换失败时留下的上一版，
/// `data.json.v{N}.bak` 是迁移前的原文件。优先列最近的一版。
fn backup_candidates(path: &Path) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    let generic = backup_file_path(path);
    if generic.exists() {
        result.push(generic);
    }
    let Some(directory) = path.parent() else {
        return result;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return result;
    };
    let mut versioned: Vec<(u32, std::path::PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let version = name
                .to_str()?
                .strip_prefix("data.json.v")?
                .strip_suffix(".bak")?
                .parse::<u32>()
                .ok()?;
            Some((version, entry.path()))
        })
        .collect();
    versioned.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    result.extend(versioned.into_iter().map(|(_, path)| path));
    result
}

fn recover_missing_app_data_file(path: &Path) -> Result<Option<LoadedAppData>, String> {
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
