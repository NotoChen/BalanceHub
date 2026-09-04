//! Shared, conservative primitives for user-scope managed hooks.
//!
//! Adapters own their Agent schema.  This module owns only the safety rules:
//! bounded JSON reads, revision checks, manifest persistence and event proof.

use super::codex::ownership;
use crate::models::{AgentCliKind, AgentHookOwnership};
use crate::services::agent_runtime::{
    hook::NormalizedHookEvent,
    repository::{MAX_RUNTIME_PROJECTION_BYTES, RUNTIME_REPOSITORY_SCHEMA_VERSION},
};
use serde_json::Value;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(crate) use ownership::{
    ensure_managed_parent, ensure_parent, home_dir, is_regular_file, is_safe_directory, now_millis,
    remove_manifest, revision_for_bytes, revision_for_missing, write_atomic, write_manifest,
};

#[derive(Debug, Clone)]
pub(crate) struct JsonSnapshot {
    pub(crate) value: Value,
    pub(crate) revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigStatus {
    Missing,
    Present,
    Unsupported,
    Unsafe,
}

#[derive(Debug, Clone)]
pub(crate) struct JsonRead {
    pub(crate) status: ConfigStatus,
    pub(crate) snapshot: Option<JsonSnapshot>,
    pub(crate) diagnostic: Option<String>,
}

pub(crate) fn read_json(path: &Path, label: &str) -> JsonRead {
    if !path.is_absolute() {
        return JsonRead {
            status: ConfigStatus::Unsafe,
            snapshot: None,
            diagnostic: Some(format!("{label} 配置路径必须是绝对路径")),
        };
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return JsonRead {
                status: ConfigStatus::Missing,
                snapshot: None,
                diagnostic: None,
            };
        }
        Err(error) => {
            return JsonRead {
                status: ConfigStatus::Unsafe,
                snapshot: None,
                diagnostic: Some(format!("无法读取 {label} 配置: {error}")),
            };
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return JsonRead {
            status: ConfigStatus::Unsafe,
            snapshot: None,
            diagnostic: Some(format!("{label} 配置不是普通文件，已拒绝访问")),
        };
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return JsonRead {
                status: ConfigStatus::Unsafe,
                snapshot: None,
                diagnostic: Some(format!("无法读取 {label} 配置: {error}")),
            }
        }
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(object)) => JsonRead {
            status: ConfigStatus::Present,
            snapshot: Some(JsonSnapshot {
                value: Value::Object(object),
                revision: revision_for_bytes(&bytes),
            }),
            diagnostic: None,
        },
        Ok(_) => JsonRead {
            status: ConfigStatus::Unsupported,
            snapshot: None,
            diagnostic: Some(format!("{label} 配置顶层必须是 JSON 对象")),
        },
        Err(error) => JsonRead {
            status: ConfigStatus::Unsupported,
            snapshot: None,
            diagnostic: Some(format!("{label} 配置 JSON 无法解析: {error}")),
        },
    }
}

pub(crate) fn encode_json(value: &Value, label: &str) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(value)
        .map(|mut bytes| {
            bytes.push(b'\n');
            bytes
        })
        .map_err(|error| format!("写入 {label} Hook 配置失败: {error}"))
}

pub(crate) fn read_manifest(path: &Path) -> Result<Option<AgentHookOwnership>, String> {
    ownership::read_manifest(path)
}

pub(crate) fn latest_event_after(
    root: &Path,
    agent: AgentCliKind,
    installed_at: i64,
) -> Option<i64> {
    let incoming = root.join("hook-spool").join("incoming");
    let incoming_latest = fs::read_dir(incoming)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return None;
            }
            let bytes = fs::read(path).ok()?;
            if bytes.len() > 256 * 1024 {
                return None;
            }
            let event = serde_json::from_slice::<NormalizedHookEvent>(&bytes).ok()?;
            (event.agent_kind == agent && event.received_at >= installed_at)
                .then_some(event.received_at)
        })
        .max();
    let projection = root.join("agent-runtime-v1").join("projection.json");
    let projected_latest = fs::symlink_metadata(&projection)
        .ok()
        .filter(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .and_then(|_| fs::read(projection).ok())
        .filter(|bytes| bytes.len() <= MAX_RUNTIME_PROJECTION_BYTES)
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| {
            (value.get("schemaVersion").and_then(Value::as_u64)
                == Some(u64::from(RUNTIME_REPOSITORY_SCHEMA_VERSION)))
            .then_some(value)
        })
        .and_then(|value| value.get("events").and_then(Value::as_array).cloned())
        .into_iter()
        .flatten()
        .filter_map(|event| {
            let event_agent = event.get("agentKind")?.as_str()?;
            let event_kind = event.get("kind")?.get("kind")?.as_str()?;
            let observed = event.get("observedAt")?.as_i64()?;
            (event_agent == agent.key()
                && event_kind.starts_with("hook_")
                && observed >= installed_at)
                .then_some(observed)
        })
        .max();
    incoming_latest.into_iter().chain(projected_latest).max()
}

pub(crate) fn config_path_for(home: &Path, agent: AgentCliKind) -> PathBuf {
    match agent {
        AgentCliKind::ClaudeCode => home.join(".claude/settings.json"),
        AgentCliKind::Gemini => home.join(".gemini/settings.json"),
        AgentCliKind::Grok => home.join(".grok/hooks/balancehub-runtime.json"),
        AgentCliKind::Codex => home.join(".codex/hooks.json"),
    }
}
