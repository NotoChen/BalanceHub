use crate::models::{
    normalize_api_key_for_protocol, normalize_invite_link, normalize_provider_auth, AppData,
    CURRENT_SCHEMA_VERSION,
};
use crate::{limits, util::read_text_file_limited};
use serde::Serialize;
use std::{
    fs::{self, File},
    io::{BufWriter, Write},
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
            .map_err(|err| format!("创建配置目录失败({}): {err}", parent.display()))?;
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
                .map_err(|err| format!("创建导出目录失败({}): {err}", parent.display()))?;
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
    let text = read_text_file_limited(path, limits::MAX_APP_DATA_FILE_BYTES, "读取配置")?;
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
fn migrate_step(version: u32, data: &mut serde_json::Value) -> Result<(), String> {
    match version {
        // v1/v2 只存在于开发期，与 v3 的差异均为「新增带默认值的字段」，
        // 反序列化时 #[serde(default)] 即可兜底，无需 Value 级调整。
        1 | 2 => Ok(()),
        3 => {
            data["workspaces"] = serde_json::Value::Array(Vec::new());
            Ok(())
        }
        4 => {
            if let Some(providers) = data
                .get_mut("providers")
                .and_then(|value| value.as_array_mut())
            {
                for provider in providers {
                    let auth = provider
                        .as_object_mut()
                        .map(|object| {
                            object
                                .entry("auth")
                                .or_insert_with(|| serde_json::json!({}))
                        })
                        .and_then(serde_json::Value::as_object_mut);
                    if let Some(auth) = auth {
                        auth.entry("loginUsername")
                            .or_insert_with(|| serde_json::Value::String(String::new()));
                        auth.entry("loginPassword")
                            .or_insert_with(|| serde_json::Value::String(String::new()));
                    }
                }
            }
            Ok(())
        }
        5 => {
            if let Some(providers) = data
                .get_mut("providers")
                .and_then(|value| value.as_array_mut())
            {
                for provider in providers {
                    let auth = provider
                        .as_object_mut()
                        .map(|object| {
                            object
                                .entry("auth")
                                .or_insert_with(|| serde_json::json!({}))
                        })
                        .and_then(serde_json::Value::as_object_mut);
                    if let Some(auth) = auth {
                        auth.entry("apiKeyTokenId")
                            .or_insert_with(|| serde_json::Value::String(String::new()));
                        auth.entry("apiKeyOptions")
                            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                    }
                }
            }
            Ok(())
        }
        6 => {
            if let Some(settings) = data
                .get_mut("settings")
                .and_then(serde_json::Value::as_object_mut)
            {
                let removed_terminal_mode = settings
                    .get("temporaryCliTerminalKind")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|kind| matches!(kind, "auto" | "systemDefault" | "custom"));
                if removed_terminal_mode {
                    settings.insert(
                        "temporaryCliTerminalKind".to_string(),
                        serde_json::to_value(crate::models::TemporaryCliTerminalKind::default())
                            .map_err(|err| format!("生成默认终端配置失败: {err}"))?,
                    );
                }
                settings.remove("temporaryCliTerminalCommand");
            }
            if let Some(providers) = data
                .get_mut("providers")
                .and_then(|value| value.as_array_mut())
            {
                for provider in providers {
                    let Some(object) = provider.as_object_mut() else {
                        continue;
                    };
                    if let Some(identity) = object
                        .get_mut("identity")
                        .and_then(serde_json::Value::as_object_mut)
                    {
                        identity
                            .entry("protocol")
                            .or_insert_with(|| serde_json::Value::String("newApi".to_string()));
                    }
                    // 认证来源(source)成为一等字段：从旧 mode 推导 —— 账号密码是一种来源，
                    // 其余（会话 Cookie / 访问令牌 / API Key）都是「手动粘贴」。
                    if let Some(auth) = object
                        .get_mut("auth")
                        .and_then(serde_json::Value::as_object_mut)
                    {
                        let source = if auth.get("mode").and_then(serde_json::Value::as_str)
                            == Some("password")
                        {
                            "password"
                        } else {
                            "manual"
                        };
                        auth.entry("source")
                            .or_insert_with(|| serde_json::Value::String(source.to_string()));
                    }
                }
            }
            Ok(())
        }
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
    limits::validate_app_data_limits(data)
}

struct LimitedWriter<W> {
    inner: W,
    written: usize,
    limit: usize,
    exceeded: bool,
}

impl<W> LimitedWriter<W> {
    fn new(inner: W, limit: usize) -> Self {
        Self {
            inner,
            written: 0,
            limit,
            exceeded: false,
        }
    }
}

impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self.written.saturating_add(buffer.len()) > self.limit {
            self.exceeded = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "serialized data exceeds configured limit",
            ));
        }
        let written = self.inner.write(buffer)?;
        self.written = self.written.saturating_add(written);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn write_json_file_limited<T: Serialize>(
    path: &Path,
    value: &T,
    max_bytes: usize,
    context: &str,
) -> Result<(), String> {
    let file =
        File::create(path).map_err(|err| format!("{context}失败({}): {err}", path.display()))?;
    let mut writer = LimitedWriter::new(BufWriter::new(file), max_bytes);
    if let Err(err) = serde_json::to_writer_pretty(&mut writer, value) {
        let exceeded = writer.exceeded;
        drop(writer);
        let _ = fs::remove_file(path);
        return Err(if exceeded {
            format!(
                "{context}失败({})：序列化结果超过 {} MiB 上限",
                path.display(),
                max_bytes / 1024 / 1024
            )
        } else {
            format!("{context}失败({}): {err}", path.display())
        });
    }
    writer.flush().map_err(|err| {
        let _ = fs::remove_file(path);
        format!("{context}失败({}): {err}", path.display())
    })?;
    drop(writer);
    Ok(())
}

fn normalize_provider_cached_values(data: &mut AppData) -> bool {
    let mut changed = false;

    for provider in &mut data.providers {
        let current_auth = provider.auth.clone();
        provider.auth = normalize_provider_auth(current_auth.clone(), provider.identity.protocol);
        if provider.auth != current_auth {
            changed = true;
        }

        let normalized =
            normalize_api_key_for_protocol(&provider.auth.api_key, provider.identity.protocol);
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
mod tests;
