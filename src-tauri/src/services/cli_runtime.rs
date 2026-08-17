mod app;
mod config;

use crate::{
    limits,
    models::{
        AgentCliKind, CliRuntimeSnapshot, Provider, TemporaryCliInstance,
        TemporaryCliInstanceStatus, TemporaryCliTerminalKind,
    },
    services::agent_cli,
    util::{read_text_file_limited, unix_millis},
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub(crate) use app::CliRuntimeService;
pub use config::{preview_config, switch_config};

const RUNTIME_DIR_NAME: &str = "balancehub-cli-runtime-v1";
const INSTANCES_DIR_NAME: &str = "instances";
const METADATA_FILE_NAME: &str = "instance.json";
const STATUS_FILE_NAME: &str = "status.json";
const STARTING_TIMEOUT_MILLIS: u128 = 2 * 60 * 1000;
const UNKNOWN_PID_TIMEOUT_MILLIS: u128 = 24 * 60 * 60 * 1000;
const EXITED_INSTANCE_RETENTION_MILLIS: u128 = 2 * 60 * 1000;
const MAX_ACTIVE_INSTANCES: usize = 80;

static INSTANCE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct RegisteredCliInstance {
    pub instance: TemporaryCliInstance,
    pub status_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum CliTerminalLocator {
    Ghostty { terminal_id: String },
}

pub(crate) struct CliTerminalActivationTarget {
    pub(crate) terminal_kind: TemporaryCliTerminalKind,
    pub(crate) locator: CliTerminalLocator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredInstanceMetadata {
    id: String,
    provider_id: String,
    provider_name: String,
    cli_kind: AgentCliKind,
    workdir: String,
    terminal_kind: TemporaryCliTerminalKind,
    terminal_locator: Option<CliTerminalLocator>,
    started_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredInstanceStatus {
    status: TemporaryCliInstanceStatus,
    pid: Option<u32>,
    ended_at: Option<u128>,
    exit_code: Option<i32>,
}

pub fn snapshot(providers: &[Provider]) -> CliRuntimeSnapshot {
    CliRuntimeSnapshot {
        configs: agent_cli::definitions()
            .iter()
            .filter(|definition| definition.default_config().is_some())
            .map(|definition| config::config_snapshot(providers, definition.kind))
            .collect(),
        instances: load_instances(),
    }
}

pub fn active_instances() -> Vec<TemporaryCliInstance> {
    load_instances()
}

pub fn instance(id: &str) -> Result<Option<TemporaryCliInstance>, String> {
    let instance_dir = validated_instance_dir(id)?;
    if !instance_dir.is_dir() {
        return Ok(None);
    }

    let metadata = read_json::<StoredInstanceMetadata>(&instance_dir.join(METADATA_FILE_NAME))?;
    let status_path = instance_dir.join(STATUS_FILE_NAME);
    let mut status = read_json::<StoredInstanceStatus>(&status_path)?;
    if reconcile_status(&metadata, &mut status) {
        write_json_atomic(&status_path, &status)?;
    }
    Ok(Some(merge_instance(metadata, status)))
}

pub fn register_instance(
    provider: &Provider,
    cli_kind: AgentCliKind,
    workdir: &Path,
    terminal_kind: TemporaryCliTerminalKind,
) -> Result<RegisteredCliInstance, String> {
    let started_at = unix_millis().to_string();
    let id = format!(
        "{:x}-{:x}-{:x}",
        unix_millis(),
        std::process::id(),
        INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let instance_dir = instances_dir().join(&id);
    fs::create_dir_all(&instance_dir).map_err(|err| {
        format!(
            "创建临时 CLI 实例目录失败({}): {err}",
            instance_dir.display()
        )
    })?;

    let metadata = StoredInstanceMetadata {
        id: id.clone(),
        provider_id: provider.identity.id.clone(),
        provider_name: provider.identity.name.clone(),
        cli_kind,
        workdir: workdir.to_string_lossy().to_string(),
        terminal_kind,
        terminal_locator: None,
        started_at: started_at.clone(),
    };
    let status = StoredInstanceStatus {
        status: TemporaryCliInstanceStatus::Starting,
        pid: None,
        ended_at: None,
        exit_code: None,
    };
    let metadata_path = instance_dir.join(METADATA_FILE_NAME);
    let status_path = instance_dir.join(STATUS_FILE_NAME);
    if let Err(err) = write_json_atomic(&metadata_path, &metadata)
        .and_then(|()| write_json_atomic(&status_path, &status))
    {
        let _ = fs::remove_dir_all(&instance_dir);
        return Err(err);
    }

    Ok(RegisteredCliInstance {
        instance: merge_instance(metadata, status),
        status_path,
    })
}

pub(crate) fn record_terminal_launch(
    id: &str,
    terminal_kind: TemporaryCliTerminalKind,
    terminal_locator: Option<CliTerminalLocator>,
) -> Result<TemporaryCliInstance, String> {
    let instance_dir = validated_instance_dir(id)?;
    let metadata_path = instance_dir.join(METADATA_FILE_NAME);
    let mut metadata = read_json::<StoredInstanceMetadata>(&metadata_path)?;
    metadata.terminal_kind = terminal_kind;
    metadata.terminal_locator = terminal_locator;
    write_json_atomic(&metadata_path, &metadata)?;

    let status = read_json::<StoredInstanceStatus>(&instance_dir.join(STATUS_FILE_NAME))?;
    Ok(merge_instance(metadata, status))
}

pub(crate) fn activation_target(id: &str) -> Result<CliTerminalActivationTarget, String> {
    let instance_dir = validated_instance_dir(id)?;
    let metadata = read_json::<StoredInstanceMetadata>(&instance_dir.join(METADATA_FILE_NAME))?;
    let status_path = instance_dir.join(STATUS_FILE_NAME);
    let mut status = read_json::<StoredInstanceStatus>(&status_path)?;
    if reconcile_status(&metadata, &mut status) {
        let _ = write_json_atomic(&status_path, &status);
    }
    if status.status == TemporaryCliInstanceStatus::Exited {
        return Err("临时 CLI 已退出，原终端窗口可能已经关闭".to_string());
    }

    metadata
        .terminal_locator
        .map(|locator| CliTerminalActivationTarget {
            terminal_kind: metadata.terminal_kind,
            locator,
        })
        .ok_or_else(|| "当前终端不支持精确定位临时 CLI 窗口".to_string())
}

pub fn mark_instance_exited(status_path: &Path, exit_code: Option<i32>) {
    let status = StoredInstanceStatus {
        status: TemporaryCliInstanceStatus::Exited,
        pid: None,
        ended_at: Some(unix_millis()),
        exit_code,
    };
    let _ = write_json_atomic(status_path, &status);
}

fn validated_instance_dir(id: &str) -> Result<PathBuf, String> {
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err("无效的临时 CLI 实例 ID".to_string());
    }
    Ok(instances_dir().join(id))
}

fn load_instances() -> Vec<TemporaryCliInstance> {
    let Ok(entries) = fs::read_dir(instances_dir()) else {
        return Vec::new();
    };

    let mut instances = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let instance = load_instance(&path)?;
            if instance.status == TemporaryCliInstanceStatus::Exited {
                if exited_instance_expired(&instance) {
                    let _ = fs::remove_dir_all(path);
                }
                return None;
            }
            Some(instance)
        })
        .collect::<Vec<_>>();

    instances.sort_by(|left, right| {
        numeric_timestamp(&right.started_at).cmp(&numeric_timestamp(&left.started_at))
    });
    instances.truncate(MAX_ACTIVE_INSTANCES);
    instances
}

fn exited_instance_expired(instance: &TemporaryCliInstance) -> bool {
    let ended_at = instance
        .ended_at
        .as_deref()
        .map(numeric_timestamp)
        .unwrap_or_else(|| numeric_timestamp(&instance.started_at));
    unix_millis().saturating_sub(ended_at) >= EXITED_INSTANCE_RETENTION_MILLIS
}

fn load_instance(instance_dir: &Path) -> Option<TemporaryCliInstance> {
    let metadata =
        read_json::<StoredInstanceMetadata>(&instance_dir.join(METADATA_FILE_NAME)).ok()?;
    let status_path = instance_dir.join(STATUS_FILE_NAME);
    let mut status = read_json::<StoredInstanceStatus>(&status_path).ok()?;
    if reconcile_status(&metadata, &mut status) {
        let _ = write_json_atomic(&status_path, &status);
    }
    Some(merge_instance(metadata, status))
}

fn reconcile_status(metadata: &StoredInstanceMetadata, status: &mut StoredInstanceStatus) -> bool {
    if status.status == TemporaryCliInstanceStatus::Exited {
        return false;
    }

    let now = unix_millis();
    let age = now.saturating_sub(numeric_timestamp(&metadata.started_at));
    let should_exit = match status.status {
        TemporaryCliInstanceStatus::Starting => age >= STARTING_TIMEOUT_MILLIS,
        TemporaryCliInstanceStatus::Running => match status.pid {
            Some(pid) => !process_is_alive(pid),
            None => age >= UNKNOWN_PID_TIMEOUT_MILLIS,
        },
        TemporaryCliInstanceStatus::Exited => false,
    };
    if !should_exit {
        return false;
    }

    status.status = TemporaryCliInstanceStatus::Exited;
    status.pid = None;
    status.ended_at = Some(now);
    true
}

fn merge_instance(
    metadata: StoredInstanceMetadata,
    status: StoredInstanceStatus,
) -> TemporaryCliInstance {
    let started_at = numeric_timestamp(&metadata.started_at);
    let can_activate =
        status.status != TemporaryCliInstanceStatus::Exited && metadata.terminal_locator.is_some();
    TemporaryCliInstance {
        id: metadata.id,
        provider_id: metadata.provider_id,
        provider_name: metadata.provider_name,
        cli_kind: metadata.cli_kind,
        workdir: metadata.workdir,
        terminal_kind: metadata.terminal_kind,
        terminal_name: metadata.terminal_kind.label().to_string(),
        started_at: metadata.started_at,
        ended_at: status
            .ended_at
            .map(|ended_at| ended_at.max(started_at).to_string()),
        pid: status.pid,
        status: status.status,
        exit_code: status.exit_code,
        can_activate,
    }
}

fn instances_dir() -> PathBuf {
    env::temp_dir()
        .join(RUNTIME_DIR_NAME)
        .join(INSTANCES_DIR_NAME)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = read_text_file_limited(
        path,
        limits::MAX_CLI_RUNTIME_FILE_BYTES,
        "读取临时 CLI 记录",
    )?;
    serde_json::from_str(&text).map_err(|_| format!("临时 CLI 记录格式无效({})", path.display()))
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("创建目录失败({}): {err}", parent.display()))?;
    }
    let text = serde_json::to_vec(value).map_err(|err| format!("序列化记录失败: {err}"))?;
    if text.len() > limits::MAX_CLI_RUNTIME_FILE_BYTES {
        return Err(format!(
            "临时 CLI 记录超过 {} KiB 上限({})",
            limits::MAX_CLI_RUNTIME_FILE_BYTES / 1024,
            path.display()
        ));
    }
    let sequence = INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = path.with_extension(format!("tmp-{}-{sequence}", std::process::id()));
    fs::write(&tmp_path, text)
        .map_err(|err| format!("写入临时记录失败({}): {err}", tmp_path.display()))?;
    replace_file(&tmp_path, path)
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, target: &Path) -> Result<(), String> {
    fs::rename(source, target).map_err(|err| {
        let _ = fs::remove_file(source);
        format!("更新文件失败({}): {err}", target.display())
    })
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, target: &Path) -> Result<(), String> {
    let sequence = INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let backup = target.with_extension(format!(
        "balancehub-replace-backup-{}-{sequence}",
        std::process::id()
    ));
    let had_target = target.exists();
    if had_target {
        if let Err(err) = fs::rename(target, &backup) {
            let _ = fs::remove_file(source);
            return Err(format!("备份待更新文件失败({}): {err}", target.display()));
        }
    }

    match fs::rename(source, target) {
        Ok(()) => {
            if had_target {
                let _ = fs::remove_file(backup);
            }
            Ok(())
        }
        Err(err) => {
            let restore_error = if had_target {
                fs::rename(&backup, target).err()
            } else {
                None
            };
            let _ = fs::remove_file(source);
            match restore_error {
                Some(restore) => Err(format!(
                    "更新文件失败({}): {err}；恢复原文件失败: {restore}",
                    target.display()
                )),
                None => Err(format!("更新文件失败({}): {err}", target.display())),
            }
        }
    }
}

fn numeric_timestamp(value: &str) -> u128 {
    value.trim().parse().unwrap_or_default()
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(target_os = "windows")]
fn process_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let mut command = std::process::Command::new("tasklist");
    let pid_text = pid.to_string();
    command.args(["/FI", &format!("PID eq {pid}"), "/NH"]);
    crate::platform::process::run_command_with_output_timeout(
        &mut command,
        std::time::Duration::from_secs(3),
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    )
    .map(|output| {
        !output.timed_out
            && output.status.is_some_and(|status| status.success())
            && output
                .stdout
                .split_whitespace()
                .any(|value| value == pid_text)
    })
    .unwrap_or(false)
}

#[cfg(not(any(unix, target_os = "windows")))]
fn process_is_alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_instance_query_preserves_recent_exit_details() {
        let id = format!(
            "test-{:x}-{:x}",
            std::process::id(),
            INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let instance_dir = instances_dir().join(&id);
        let now = unix_millis();
        let metadata = StoredInstanceMetadata {
            id: id.clone(),
            provider_id: "provider-test".to_string(),
            provider_name: "Relay".to_string(),
            cli_kind: AgentCliKind::Codex,
            workdir: "/workspace".to_string(),
            terminal_kind: TemporaryCliTerminalKind::Terminal,
            terminal_locator: None,
            started_at: now.saturating_sub(1).to_string(),
        };
        let status = StoredInstanceStatus {
            status: TemporaryCliInstanceStatus::Exited,
            pid: None,
            ended_at: Some(now),
            exit_code: Some(17),
        };
        write_json_atomic(&instance_dir.join(METADATA_FILE_NAME), &metadata).unwrap();
        write_json_atomic(&instance_dir.join(STATUS_FILE_NAME), &status).unwrap();

        let loaded = instance(&id).unwrap().unwrap();
        assert_eq!(loaded.status, TemporaryCliInstanceStatus::Exited);
        assert_eq!(loaded.exit_code, Some(17));
        assert!(instance_dir.is_dir());

        let _ = fs::remove_dir_all(instance_dir);
    }

    #[test]
    fn single_instance_query_rejects_path_traversal() {
        assert!(instance("../status").is_err());
    }
}
