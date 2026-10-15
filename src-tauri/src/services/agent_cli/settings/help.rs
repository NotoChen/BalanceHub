use crate::{platform::process, services::agent_cli};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

struct CachedHelp {
    result: Option<Result<String, String>>,
    read_at: Instant,
}

type HelpEntries = BTreeMap<String, Arc<Mutex<CachedHelp>>>;
static HELP: OnceLock<Mutex<HelpEntries>> = OnceLock::new();

pub(crate) fn read_help(cli_path: &str) -> Result<String, String> {
    let key = executable_key(cli_path)?;
    let mut cache = HELP
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| "CLI 能力缓存不可用")?;
    if cache.len() >= 24 && !cache.contains_key(&key) {
        cache.clear();
    }
    let entry = Arc::clone(cache.entry(key).or_insert_with(|| {
        Arc::new(Mutex::new(CachedHelp {
            result: None,
            read_at: Instant::now(),
        }))
    }));
    drop(cache);
    // Rapid edits share one bounded inspection per executable. Failed help
    // probes are also cached briefly so every slider input does not retry it.
    let mut entry = entry.lock().map_err(|_| "CLI 能力缓存不可用")?;
    if let Some(result) = &entry.result {
        let lifetime = if result.is_ok() { 300 } else { 60 };
        if entry.read_at.elapsed() < Duration::from_secs(lifetime) {
            return result.clone();
        }
    }
    let result = inspect(cli_path);
    entry.read_at = Instant::now();
    entry.result = Some(result.clone());
    result
}

pub(super) fn executable_key(cli_path: &str) -> Result<String, String> {
    let path = Path::new(cli_path);
    if cli_path.is_empty() || !path.is_file() {
        return Err("所选 CLI 路径暂不可用".into());
    }
    let mut digest = Sha256::new();
    agent_cli::cache::observe_executable(&mut digest, path);
    if let Some(runtime_path) = agent_cli::runtime_path_for(path) {
        digest.update(runtime_path.as_encoded_bytes());
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn inspect(cli_path: &str) -> Result<String, String> {
    let mut command =
        Command::new(crate::platform::paths::compatible_path(Path::new(cli_path)).as_ref());
    command.arg("--help");
    if let Some(path) = agent_cli::runtime_path_for(Path::new(cli_path)) {
        command.env("PATH", path);
    }
    let output =
        process::run_command_with_output_timeout(&mut command, Duration::from_secs(5), 128 * 1024)
            .map_err(|_| "读取 CLI 启动选项失败")?;
    if output.timed_out || output.stdout_truncated || !output.status.is_some_and(|s| s.success()) {
        return Err("读取 CLI 启动选项失败或超时，可重试；仍可使用默认设置启动".into());
    }
    Ok(output.stdout)
}
