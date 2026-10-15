//! Query the installed CLI's default feature switches once per environment.
//! The command only lists metadata. The in-process override removes file-level
//! feature choices without writing a configuration or starting a session.
use super::*;
use crate::{
    platform::{paths::compatible_path, process},
    services::agent_cli,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

type FeatureValues = BTreeMap<String, bool>;
#[derive(Default)]
struct Entry {
    checked_at: Option<Instant>,
    result: Option<Result<FeatureValues, String>>,
}
type Entries = BTreeMap<String, Arc<Mutex<Entry>>>;
static FEATURES: OnceLock<Mutex<Entries>> = OnceLock::new();

pub(in crate::services::agent_cli::settings) fn load(
    snapshot: &mut NativeSnapshot,
    cli_path: &str,
) {
    if snapshot.kind != AgentCliKind::Codex {
        return;
    }
    let result = if !snapshot.include_process_environment {
        Err("WSL 的默认功能开关由发行版内的 Codex 提供；此处仅展示文件中的设置。".into())
    } else {
        read(snapshot, cli_path)
    };
    match result {
        Ok(values) => snapshot.feature_defaults = values,
        Err(message) => snapshot.feature_defaults_notice = message,
    }
}

fn read(snapshot: &NativeSnapshot, cli_path: &str) -> Result<FeatureValues, String> {
    let mut digest = Sha256::new();
    digest.update(super::super::help::executable_key(cli_path)?);
    agent_cli::cache::observe_environment(&mut digest, AgentCliKind::Codex);
    digest.update(snapshot.root.as_os_str().as_encoded_bytes());
    if let Some(workspace) = &snapshot.workspace {
        digest.update(workspace.as_os_str().as_encoded_bytes());
    }
    for layer in &snapshot.layers {
        agent_cli::cache::observe_path(&mut digest, &layer.path);
    }
    for failure in &snapshot.read_failures {
        agent_cli::cache::observe_path(&mut digest, &failure.path);
    }
    let key = format!("{:x}", digest.finalize());
    let mut entries = FEATURES
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| "功能开关缓存不可用")?;
    if entries.len() >= 24 && !entries.contains_key(&key) {
        entries.retain(|_, entry| Arc::strong_count(entry) > 1);
    }
    let entry = entries.entry(key).or_default().clone();
    drop(entries);
    let mut entry = entry.lock().map_err(|_| "功能开关缓存不可用")?;
    if let (Some(result), Some(checked_at)) = (&entry.result, entry.checked_at) {
        let ttl = if result.is_ok() { 300 } else { 60 };
        if checked_at.elapsed() < Duration::from_secs(ttl) {
            return result.clone();
        }
    }
    let result = query(snapshot, cli_path);
    entry.checked_at = Some(Instant::now());
    entry.result = Some(result.clone());
    result
}

fn query(snapshot: &NativeSnapshot, cli_path: &str) -> Result<FeatureValues, String> {
    if !snapshot.root.is_dir() {
        return Err("配置目录尚未创建，暂未读取 CLI 默认开关。".into());
    }
    let path = Path::new(cli_path);
    let mut command = Command::new(compatible_path(path).as_ref());
    command
        .args(["features", "list", "-c", "features={}"])
        .env("CODEX_HOME", &snapshot.root)
        .env("NO_COLOR", "1")
        .current_dir(
            compatible_path(snapshot.workspace.as_deref().unwrap_or(&snapshot.root)).as_ref(),
        );
    if let Some(path) = agent_cli::runtime_path_for(path) {
        command.env("PATH", path);
    }
    let output =
        process::run_command_with_output_timeout(&mut command, Duration::from_secs(2), 128 * 1024)
            .map_err(|_| {
                "当前 CLI 未能提供默认功能开关；文件中未设置的项目仍使用 CLI 默认行为。"
            })?;
    if output.timed_out
        || output.stdout_truncated
        || !output.status.is_some_and(|status| status.success())
    {
        return Err(
            "当前 CLI 未能提供默认功能开关；文件中未设置的项目仍使用 CLI 默认行为。".into(),
        );
    }
    let values: FeatureValues = output
        .stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let key = parts.next()?;
            if !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'))
            {
                return None;
            }
            let value = match parts.last()? {
                "true" => true,
                "false" => false,
                _ => return None,
            };
            Some((key.into(), value))
        })
        .collect();
    if values.is_empty() {
        return Err("当前 CLI 没有返回可识别的功能开关。".into());
    }
    Ok(values)
}
