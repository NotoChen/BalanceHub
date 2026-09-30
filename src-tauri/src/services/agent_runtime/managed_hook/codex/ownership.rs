use super::MANIFEST_SCHEMA_VERSION;
use crate::models::{AgentCliKind, AgentHookOwnership};
use crate::services::agent_runtime::{
    hook::NormalizedHookEvent,
    repository::{MAX_RUNTIME_PROJECTION_BYTES, RUNTIME_REPOSITORY_SCHEMA_VERSION},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManifestFile {
    pub(crate) schema_version: u16,
    pub(crate) ownership: AgentHookOwnership,
    /// Historical receipt retained after the user explicitly takes over a
    /// callback through global Hook management; it no longer grants ownership.
    #[serde(default)]
    pub(crate) catalog_detached: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigSnapshot {
    pub(crate) value: Value,
    pub(crate) revision: String,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigStatus {
    Missing,
    Present,
    Unsupported,
    Unsafe,
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigRead {
    pub(crate) status: ConfigStatus,
    pub(crate) snapshot: Option<ConfigSnapshot>,
    pub(crate) diagnostic: Option<String>,
}

pub(crate) fn read_config(path: &Path) -> ConfigRead {
    if !path.is_absolute() {
        return ConfigRead {
            status: ConfigStatus::Unsafe,
            snapshot: None,
            diagnostic: Some("Codex Hook 配置路径必须是绝对路径".to_string()),
        };
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return ConfigRead {
                status: ConfigStatus::Missing,
                snapshot: None,
                diagnostic: None,
            }
        }
        Err(error) => {
            return ConfigRead {
                status: ConfigStatus::Unsafe,
                snapshot: None,
                diagnostic: Some(format!("无法读取 Codex Hook 配置: {error}")),
            }
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return ConfigRead {
            status: ConfigStatus::Unsafe,
            snapshot: None,
            diagnostic: Some("Codex Hook 配置不是普通文件，已拒绝访问".to_string()),
        };
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return ConfigRead {
                status: ConfigStatus::Unsafe,
                snapshot: None,
                diagnostic: Some(format!("无法读取 Codex Hook 配置: {error}")),
            }
        }
    };
    let value = match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(object)) => Value::Object(object),
        Ok(_) => {
            return ConfigRead {
                status: ConfigStatus::Unsupported,
                snapshot: None,
                diagnostic: Some("Codex hooks.json 顶层必须是 JSON 对象".to_string()),
            }
        }
        Err(error) => {
            return ConfigRead {
                status: ConfigStatus::Unsupported,
                snapshot: None,
                diagnostic: Some(format!("Codex hooks.json JSON 无法解析: {error}")),
            }
        }
    };
    ConfigRead {
        status: ConfigStatus::Present,
        snapshot: Some(ConfigSnapshot {
            revision: revision_for_bytes(&bytes),
            text: String::from_utf8_lossy(&bytes).into_owned(),
            value,
        }),
        diagnostic: None,
    }
}

pub(crate) fn read_manifest(path: &Path) -> Result<Option<AgentHookOwnership>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err("ownership manifest 不是普通文件".to_string())
        }
        Ok(_) => {
            let bytes = fs::read(path).map_err(|error| error.to_string())?;
            let manifest = serde_json::from_slice::<ManifestFile>(&bytes)
                .map_err(|error| format!("ownership manifest 格式无效: {error}"))?;
            if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
                return Err("ownership manifest 版本不受支持".to_string());
            }
            if manifest.catalog_detached {
                return Err(
                    "会话接入已转由全局 Hook 管理，专用控制器不会覆盖或自动恢复这些规则".to_owned(),
                );
            }
            Ok(Some(manifest.ownership))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn latest_event_after(root: &Path, installed_at: i64) -> Option<i64> {
    latest_incoming_event_after(root, installed_at)
        .into_iter()
        .chain(latest_projected_event_after(root, installed_at))
        .max()
}

fn latest_incoming_event_after(root: &Path, installed_at: i64) -> Option<i64> {
    let incoming = root.join("hook-spool").join("incoming");
    let entries = fs::read_dir(incoming).ok()?;
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return None;
            }
            let bytes = fs::read(&path).ok()?;
            if bytes.len() > 256 * 1024 {
                return None;
            }
            let event = serde_json::from_slice::<NormalizedHookEvent>(&bytes).ok()?;
            (event.agent_kind == AgentCliKind::Codex && event.received_at >= installed_at)
                .then_some(event.received_at)
        })
        .max()
}

/// A consumed Hook no longer exists in `incoming`; the runtime repository is
/// the durable source of the same event after projection commit and ack.
fn latest_projected_event_after(root: &Path, installed_at: i64) -> Option<i64> {
    let path = root.join("agent-runtime-v1").join("projection.json");
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    let bytes = fs::read(&path).ok()?;
    if bytes.len() > MAX_RUNTIME_PROJECTION_BYTES {
        return None;
    }
    let projection = serde_json::from_slice::<Value>(&bytes).ok()?;
    if projection.get("schemaVersion")?.as_u64()? != u64::from(RUNTIME_REPOSITORY_SCHEMA_VERSION) {
        return None;
    }
    projection
        .get("events")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|event| {
            let agent_kind = event.get("agentKind")?.as_str()?;
            let event_kind = event.get("kind")?.get("kind")?.as_str()?;
            let observed_at = event.get("observedAt")?.as_i64()?;
            (agent_kind == "codex"
                && event_kind.starts_with("hook_")
                && observed_at >= installed_at)
                .then_some(observed_at)
        })
        .max()
}

pub(crate) fn write_manifest(path: &Path, ownership: &AgentHookOwnership) -> Result<(), String> {
    let manifest = ManifestFile {
        schema_version: MANIFEST_SCHEMA_VERSION,
        ownership: ownership.clone(),
        catalog_detached: false,
    };
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    write_atomic(path, &bytes)
}

pub(crate) fn remove_manifest(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err("ownership manifest 不是普通文件，未删除".to_string())
        }
        Ok(_) => {
            fs::remove_file(path).map_err(|error| format!("删除 ownership manifest 失败: {error}"))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("读取 ownership manifest 失败: {error}")),
    }
}

pub(crate) fn encode_config(value: &Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(value)
        .map(|mut bytes| {
            bytes.push(b'\n');
            bytes
        })
        .map_err(|error| format!("写入 Codex Hook 配置失败: {error}"))
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "目标文件缺少父目录".to_string())?;
    ensure_parent(path)?;
    let nonce = now_millis();
    let temporary = parent.join(format!(".balancehub-hook-{nonce}.tmp"));
    let existing_permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = options
            .open(&temporary)
            .map_err(|error| format!("创建临时 Hook 文件失败: {error}"))?;
        restrict_file(&file).map_err(|error| format!("设置 Hook 文件权限失败: {error}"))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("写入 Hook 文件失败: {error}"))?;
        drop(file);
        if let Some(permissions) = existing_permissions {
            fs::set_permissions(&temporary, permissions)
                .map_err(|error| format!("保留 Hook 文件权限失败: {error}"))?;
        }
        fs::rename(&temporary, path).map_err(|error| format!("替换 Hook 文件失败: {error}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn ensure_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "目标文件缺少父目录".to_string())?;
    if !parent.is_absolute() {
        return Err("Hook 目录必须是绝对路径".to_string());
    }
    if !parent.is_dir() {
        return Err(format!("Hook 目录不存在: {}", parent.display()));
    }
    let metadata = fs::symlink_metadata(parent).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("Hook 目录不能是符号链接".to_string());
    }
    Ok(())
}

pub(crate) fn restrict_file(_file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = _file.metadata()?.permissions();
        permissions.set_mode(0o600);
        _file.set_permissions(permissions)?;
    }
    Ok(())
}

pub(crate) fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

pub(crate) fn is_safe_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

pub(crate) fn revision_for_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

pub(crate) fn revision_for_missing() -> String {
    "missing".to_string()
}

pub(crate) fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

pub(crate) fn ensure_managed_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "目标文件缺少父目录".to_string())?;
    if !parent.is_absolute() {
        return Err("Hook 目录必须是绝对路径".to_string());
    }
    if !parent.exists() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建 BalanceHub Hook 目录失败: {error}"))?;
    }
    ensure_parent(path)
}
