mod file_io;
mod legacy_session_index;
mod migration;
mod split;
mod validation;

use crate::{
    limits,
    models::{AppData, CURRENT_SCHEMA_VERSION},
    state::CoreState,
};
use file_io::{
    backup_file_path, data_file_path, lock_data_directory, tmp_file_path, write_json_file_limited,
    DATA_FILE_NAME,
};
use migration::{read_app_data_file, BackupBeforeMigrate, LoadedAppData};
use split::{
    paths_from_data_path, read_config_file, read_state_file, save_split, ConfigData, SaveMode,
    SplitPaths,
};
use std::{
    fs,
    path::{Path, PathBuf},
};
use validation::{normalize_provider_cached_values, validate_app_data_schema};

#[cfg(test)]
use file_io::replace_data_file;
#[cfg(test)]
use migration::{backup_legacy_file, migrate_app_data};

/// 冷启动读取本地配置。优先读取拆分后的 `config.json` + `state.json`；
/// 只有旧安装才走 `data.json` 迁移，迁移成功后把旧文件改名保留。
pub fn load_app_data() -> Result<AppData, String> {
    let data_path = data_file_path()?;
    lock_data_directory(&data_path)?;
    let paths = paths_from_data_path(&data_path);

    recover_missing_split_file(&paths)?;

    if paths.config.exists() {
        return load_split_app_data(&paths);
    }
    if paths.state.exists() {
        split::mark_config_missing(&paths.root);
        return Err(with_backup_hint(
            &paths.config,
            format!(
                "配置文件缺失：{} 不存在，但运行状态 {} 仍在。请从备份恢复 config.json，或确认要清空配置后手动删除 state.json 再启动。",
                paths.config.display(),
                paths.state.display()
            ),
        ));
    }
    if paths.legacy.exists() {
        let loaded = read_app_data_file(&paths.legacy, BackupBeforeMigrate::Yes)
            .map_err(|error| with_backup_hint(&paths.legacy, error))?;
        return migrate_legacy_to_split(&paths, loaded);
    }
    if let Some(recovered) = recover_missing_app_data_file(&paths.legacy)? {
        return migrate_legacy_to_split(&paths, recovered);
    }

    // 全新目录：立即建立两个文件，用户启动后就能在配置目录看到并编辑
    // config.json；写失败不影响本次启动，后续保存会继续尝试。
    let mut data = AppData::default();
    merge_dev_sample_on_load(&mut data);
    split::mark_config_fresh(&paths.root);
    if let Err(error) = save_split(&paths, &data, SaveMode::Replace) {
        log::warn!("首次创建本地配置文件失败：{error}");
    }
    Ok(data)
}

fn load_split_app_data(paths: &SplitPaths) -> Result<AppData, String> {
    let (mut data, normalized, state_warning) =
        split::load_data_from_files(paths, &AppData::default())
            .map_err(|error| with_backup_hint(&paths.config, error))?;
    let merged_sample = merge_dev_sample_on_load(&mut data);
    if let Some(warning) = state_warning {
        log::warn!("{warning}");
        // 运行状态损坏时保留一份证据，再按默认状态启动；不要把它伪装成
        // 配置损坏去阻断整个应用。
        if paths.state.exists() {
            if let Err(error) = preserve_corrupt_file(&paths.state) {
                log::warn!("保留损坏的 state.json 失败：{error}");
            }
        }
    }
    if normalized || merged_sample || !paths.state.exists() {
        if let Err(error) = save_split(paths, &data, SaveMode::RespectExternalEdits) {
            log::warn!("启动时写回规范化配置或运行状态失败：{error}");
        }
    }
    Ok(data)
}

fn migrate_legacy_to_split(paths: &SplitPaths, loaded: LoadedAppData) -> Result<AppData, String> {
    let mut data = loaded.data;
    merge_dev_sample_on_load(&mut data);
    if loaded.migrated {
        log::info!(
            "旧配置已从 schemaVersion 升级到 {CURRENT_SCHEMA_VERSION}，并拆分到 config.json/state.json"
        );
    }
    validate_app_data_schema(&data)?;
    normalize_provider_cached_values(&mut data);
    limits::normalize_app_data(&mut data);
    validate_app_data_schema(&data)?;

    let backup_ready = ensure_legacy_backup(&paths.legacy);
    split::mark_config_fresh(&paths.root);
    save_split(paths, &data, SaveMode::Replace)?;
    if backup_ready {
        if let Err(error) = fs::remove_file(&paths.legacy) {
            log::warn!(
                "配置已拆分到 {}，但旧文件 {} 无法删除：{error}",
                paths.config.display(),
                paths.legacy.display()
            );
        }
    } else {
        log::warn!(
            "配置已拆分到 {}，但无法为旧文件 {} 建立备份，已保留原文件",
            paths.config.display(),
            paths.legacy.display()
        );
    }

    if !cfg!(debug_assertions) {
        cleanup_legacy_session_index(loaded.legacy_session_index_root.as_deref());
    }
    Ok(data)
}

/// 拆分迁移前至少保留一份与当前旧文件内容一致的副本。此前留下的同名
/// 备份可能属于更早的 data.json，不能仅凭“存在备份”就删除当前文件。
fn ensure_legacy_backup(path: &Path) -> bool {
    let source = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            log::warn!("读取旧配置失败({}): {error}", path.display());
            return false;
        }
    };
    let mut candidates = Vec::new();
    append_backup_candidates(path, &mut candidates);
    if candidates
        .iter()
        .any(|candidate| fs::read(candidate).is_ok_and(|bytes| bytes == source))
    {
        return true;
    }

    let stamp = crate::util::unix_millis();
    let mut target = path.with_file_name(format!(
        "{DATA_FILE_NAME}.v{CURRENT_SCHEMA_VERSION}-{stamp}.bak"
    ));
    let mut suffix = 1;
    while target.exists() {
        target = path.with_file_name(format!(
            "{DATA_FILE_NAME}.v{CURRENT_SCHEMA_VERSION}-{stamp}-{suffix}.bak"
        ));
        suffix += 1;
    }
    match fs::write(&target, &source) {
        Ok(_) => true,
        Err(error) => {
            log::warn!(
                "备份旧配置失败({} -> {}): {error}",
                path.display(),
                target.display()
            );
            false
        }
    }
}

fn recover_missing_split_file(paths: &SplitPaths) -> Result<(), String> {
    if !paths.config.exists() {
        for candidate in [
            split::config_tmp_path(&paths.config),
            backup_file_path(&paths.config),
        ] {
            if !candidate.exists() {
                continue;
            }
            match read_config_file(&candidate) {
                Ok(_) => {
                    restore_split_source(&candidate, &paths.config, "配置")?;
                    break;
                }
                Err(error) => log::warn!("无法从 {} 恢复配置：{error}", candidate.display()),
            }
        }
    }
    if !paths.state.exists() {
        for candidate in [
            split::config_tmp_path(&paths.state),
            backup_file_path(&paths.state),
        ] {
            if !candidate.exists() {
                continue;
            }
            let (state, warning) = read_state_file(&candidate);
            if state.is_some() {
                restore_split_source(&candidate, &paths.state, "运行状态")?;
                break;
            } else if let Some(warning) = warning {
                log::warn!("无法从 {} 恢复运行状态：{warning}", candidate.display());
            }
        }
    }
    Ok(())
}

/// 替换中断后留下 `.tmp`，或 Windows 替换前留下的 `.bak`。两者都可能是
/// 唯一完整的副本；正式文件存在时不会走到这里。
fn restore_split_source(source: &Path, target: &Path, label: &str) -> Result<(), String> {
    fs::rename(source, target).map_err(|error| {
        format!(
            "恢复{label}失败({} -> {}): {error}",
            source.display(),
            target.display()
        )
    })?;
    log::warn!("已从 {} 恢复未完成的{label}写入", source.display());
    Ok(())
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

/// 普通业务保存。外部已经改坏/改动的 `config.json` 不会被覆盖；调用方
/// 会收到明确错误，前端可以提示用户等待热重载。
pub fn save_app_data(data: &AppData) -> Result<(), String> {
    validate_app_data_schema(data)?;
    let paths = paths_from_data_path(&data_file_path()?);
    save_split(&paths, data, SaveMode::RespectExternalEdits)
}

/// 云同步恢复、导入、备份恢复等明确要求整体替换的入口。
pub(crate) fn save_app_data_in_dir(root: &Path, data: &AppData) -> Result<(), String> {
    validate_app_data_schema(data)?;
    save_split(&split::paths_in_dir(root), data, SaveMode::Replace)
}

/// 云同步启动恢复用：不触发迁移、不写盘，只读当前配置与运行状态。
pub(crate) fn load_app_data_in_dir(root: &Path) -> Result<Option<AppData>, String> {
    let paths = split::paths_in_dir(root);
    recover_missing_split_file(&paths)?;
    if paths.config.exists() {
        let (data, _, warning) = split::load_data_from_files(&paths, &AppData::default())?;
        if let Some(warning) = warning {
            log::warn!("{warning}");
        }
        return Ok(Some(data));
    }
    if paths.legacy.exists() {
        return Ok(Some(
            read_app_data_file(&paths.legacy, BackupBeforeMigrate::No)?.data,
        ));
    }
    Ok(None)
}

pub(crate) fn config_snapshot_of(data: &AppData) -> Result<Vec<u8>, String> {
    split::config_bytes(data)
}

/// 启动读取失败后的重试依据：配置文件（或尚未迁移的旧 data.json）是否
/// 发生了可见变化。监听线程用它避免每 800ms 重复读取同一个损坏文件。
pub(crate) fn startup_source_signature() -> Result<String, String> {
    let paths = paths_from_data_path(&data_file_path()?);
    Ok(split::retry_signature(&paths))
}

/// 外部配置热重载：在事务锁内以磁盘配置为基准合并内存运行状态。返回
/// `Some((provider_count, settings_changed))` 表示内存已更新。
pub(crate) fn reload_config_from_disk(core: &CoreState) -> Result<Option<(usize, bool)>, String> {
    let paths = paths_from_data_path(&data_file_path()?);
    if !split::config_file_changed(&paths) {
        return Ok(None);
    }
    let _transaction = core
        .mutation_gate
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let current = core
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let config = read_config_file(&paths.config)?;
    let next = split::reload_data_from_files(&paths, &current, config)?;
    if split::config_bytes(&current)? == split::config_bytes(&next)? {
        return Ok(None);
    }
    let settings_changed =
        serde_json::to_value(&current.settings).ok() != serde_json::to_value(&next.settings).ok();
    let provider_count = next.providers.len();
    let mut next = next;
    let revision = core.next_revision();
    next.revision = revision;
    for provider in &mut next.providers {
        provider.revision = revision;
    }
    // 运行状态写失败不应让已经合法的新配置无法生效；内存仍然采用新配置，
    // 下一次普通保存会再次尝试写 state.json。
    if let Err(error) = split::save_state_only(&paths, &next) {
        log::warn!("外部配置已重载，但保存运行状态失败：{error}");
    }
    crate::host::app_data_changed(&current, &next);
    {
        let mut data = core.data.write().unwrap_or_else(|error| error.into_inner());
        *data = next;
    }
    Ok(Some((provider_count, settings_changed)))
}

/// 当前 data.json 已损坏时，用同目录最近的自动备份恢复。只接受解析失败的
/// 主文件：仍可读取的配置（包括版本过新）必须走显式导入，不能静默回退。
pub fn restore_latest_backup() -> Result<(std::path::PathBuf, AppData), String> {
    let path = data_file_path()?;
    lock_data_directory(&path)?;
    restore_latest_backup_at(&path)
}

fn restore_latest_backup_at(path: &Path) -> Result<(std::path::PathBuf, AppData), String> {
    let paths = paths_from_data_path(path);
    if paths.config.exists() || paths.state.exists() {
        return restore_split_backup_at(&paths);
    }
    restore_legacy_backup_at(path)
}

fn restore_split_backup_at(paths: &SplitPaths) -> Result<(PathBuf, AppData), String> {
    if paths.config.exists() {
        match read_config_file(&paths.config) {
            Ok(_) => {
                return Err(
                    "当前 config.json 仍可读取，请通过「从备份恢复」选择要导入的备份".to_string(),
                )
            }
            Err(error) if !error.contains("解析") => {
                return Err(format!(
                    "当前 config.json 不是解析损坏，无法自动恢复：{error}"
                ));
            }
            Err(_) => {}
        }
    }
    let candidates = backup_candidates(&paths.config);
    if candidates.is_empty() {
        return Err("没有可用的自动备份".to_string());
    }
    let mut errors = Vec::new();
    for candidate in candidates {
        let restored = match restore_candidate(paths, &candidate) {
            Ok(data) => data,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        if let Err(error) = validate_app_data_schema(&restored) {
            errors.push(error);
            continue;
        }
        preserve_corrupt_file(&paths.config)?;
        save_split(paths, &restored, SaveMode::Replace)?;
        return Ok((candidate, restored));
    }
    Err(format!("自动备份均无法读取：{}", errors.join("；")))
}

fn restore_candidate(paths: &SplitPaths, candidate: &Path) -> Result<AppData, String> {
    let is_config = candidate
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(split::CONFIG_FILE_NAME));
    if is_config {
        let config = read_config_file(candidate)?;
        let (state, _) = read_state_file(&paths.state);
        split::merge_config(&AppData::default(), config, state).map(|(data, _)| data)
    } else {
        let mut data = read_app_data_file(candidate, BackupBeforeMigrate::No)?.data;
        validate_app_data_schema(&data)?;
        normalize_provider_cached_values(&mut data);
        limits::normalize_app_data(&mut data);
        // 旧格式备份里也带有当时的运行状态；当前 state.json 若仍可读，
        // 优先保留较新的额度与记录，只把旧备份当作配置来源。
        let (state, _) = read_state_file(&paths.state);
        if state.is_some() {
            let config = split::ConfigData::from_app_data(&data);
            return split::merge_config(&AppData::default(), config, state).map(|(data, _)| data);
        }
        Ok(data)
    }
}

fn restore_legacy_backup_at(path: &Path) -> Result<(PathBuf, AppData), String> {
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
        save_split(&paths_from_data_path(path), &data, SaveMode::Replace)?;
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
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| DATA_FILE_NAME.to_string());
    let stamp = crate::util::unix_millis();
    let mut target = path.with_file_name(format!("{file_name}.corrupt-{stamp}"));
    let mut suffix = 1;
    while target.exists() {
        target = path.with_file_name(format!("{file_name}.corrupt-{stamp}-{suffix}"));
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
    let mut data = read_app_data_for_import(source)?;
    validate_app_data_schema(&data)?;
    normalize_provider_cached_values(&mut data);
    limits::normalize_app_data(&mut data);
    save_split(
        &paths_from_data_path(&data_file_path()?),
        &data,
        SaveMode::Replace,
    )?;
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
    let mut data = read_app_data_text_for_import(text, label)?;
    validate_app_data_schema(&data)?;
    normalize_provider_cached_values(&mut data);
    limits::normalize_app_data(&mut data);
    save_split(
        &paths_from_data_path(&data_file_path()?),
        &data,
        SaveMode::Replace,
    )?;
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

/// 导入既接受完整备份（`data.json`/导出文件），也接受拆分后的
/// `config.json`；后者与当前运行状态合并后导入。
fn read_app_data_for_import(source: &Path) -> Result<AppData, String> {
    match read_app_data_file(source, BackupBeforeMigrate::No) {
        Ok(loaded) => Ok(loaded.data),
        Err(full_error) => {
            let config = read_config_file(source).map_err(|_| full_error)?;
            let paths = paths_from_data_path(&data_file_path()?);
            let (state, _) = read_state_file(&paths.state);
            split::merge_config(&AppData::default(), config, state).map(|(data, _)| data)
        }
    }
}

fn read_app_data_text_for_import(text: &str, label: &Path) -> Result<AppData, String> {
    match migration::read_app_data_text(text, label, BackupBeforeMigrate::No) {
        Ok(loaded) => Ok(loaded.data),
        Err(full_error) => {
            let value =
                serde_json::from_str::<serde_json::Value>(text).map_err(|_| full_error.clone())?;
            let stored_version = value
                .get("schemaVersion")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(u64::from(CURRENT_SCHEMA_VERSION))
                as u32;
            if stored_version != CURRENT_SCHEMA_VERSION {
                return Err(full_error);
            }
            let config =
                serde_json::from_value::<ConfigData>(value).map_err(|_| full_error.clone())?;
            config.validate()?;
            let paths = paths_from_data_path(&data_file_path()?);
            let (state, _) = read_state_file(&paths.state);
            split::merge_config(&AppData::default(), config, state).map(|(data, _)| data)
        }
    }
}

/// 开发版演示卡片：只在开发脚本开启 `BALANCEHUB_DEV_SAMPLE`、且盘上还没有演示卡片
/// 时并入一次，之后和普通配置一样只读不写。
#[cfg(debug_assertions)]
fn merge_dev_sample_on_load(data: &mut AppData) -> bool {
    crate::dev_sample::merge_on_load(data).is_some()
}

#[cfg(not(debug_assertions))]
fn merge_dev_sample_on_load(_data: &mut AppData) -> bool {
    false
}

/// 解析失败时附带同目录的自动备份清单。只有解析类错误才提示备份：
/// 结构版本过新或权限问题不是旧备份能解决的，列出来只会误导用户。
fn with_backup_hint(path: &Path, error: String) -> String {
    if !error.contains("解析") {
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
        "{error}。同目录下检测到自动备份：{list}；可直接点击「自动备份恢复」，或通过「从备份恢复」选择文件，也可以修复或覆盖 config.json 后重新读取"
    )
}

/// 应用自己写出的备份。`config.json.bak`/`state.json.bak` 是 Windows 替换
/// 失败时留下的上一版；`.v{N}.bak` 是迁移前的原文件。先列配置，再列旧版
/// 单文件，均按版本从新到旧。
fn backup_candidates(path: &Path) -> Vec<PathBuf> {
    let paths = paths_from_data_path(path);
    let mut result = Vec::new();
    append_backup_candidates(&paths.config, &mut result);
    append_backup_candidates(&paths.legacy, &mut result);
    result
}

fn append_backup_candidates(path: &Path, result: &mut Vec<PathBuf>) {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let generic = path.with_file_name(format!("{file_name}.bak"));
    if generic.exists() {
        result.push(generic);
    }
    let Some(directory) = path.parent() else {
        return;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let prefix = format!("{file_name}.v");
    let mut versioned: Vec<(u32, u64, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let rest = name.to_str()?.strip_prefix(&prefix)?.strip_suffix(".bak")?;
            // 允许 `v12.bak` 与 `v12-时间戳.bak`：后者用于保留内容不同的
            // 历史迁移备份，同时仍能被恢复列表识别。
            let mut parts = rest.split('-');
            let version = parts.next()?.parse::<u32>().ok()?;
            let stamp = parts.next().and_then(|value| value.parse::<u64>().ok());
            Some((version, stamp.unwrap_or(0), entry.path()))
        })
        .collect();
    // 同版本内新备份优先：旧的 `v12.bak` 不能抢在刚写下的
    // `v12-时间戳.bak` 前面被自动恢复。
    versioned.sort_by_key(|(version, stamp, _)| {
        (std::cmp::Reverse(*version), std::cmp::Reverse(*stamp))
    });
    result.extend(versioned.into_iter().map(|(_, _, path)| path));
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
