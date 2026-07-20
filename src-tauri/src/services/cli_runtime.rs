use crate::{
    models::{
        normalize_api_key, CliConfigSnapshot, CliRuntimeSnapshot, LivenessCliKind, Provider,
        TemporaryCliInstance, TemporaryCliInstanceStatus, TemporaryCliTerminalKind,
    },
    services::liveness::{anthropic_base_url, openai_base_url},
    util::unix_millis,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};

const RUNTIME_DIR_NAME: &str = "balancehub-cli-runtime-v1";
const INSTANCES_DIR_NAME: &str = "instances";
const METADATA_FILE_NAME: &str = "instance.json";
const STATUS_FILE_NAME: &str = "status.json";
const STARTING_TIMEOUT_MILLIS: u128 = 2 * 60 * 1000;
const UNKNOWN_PID_TIMEOUT_MILLIS: u128 = 24 * 60 * 60 * 1000;
const EXITED_RETENTION_MILLIS: u128 = 7 * 24 * 60 * 60 * 1000;
const MAX_INSTANCE_HISTORY: usize = 80;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredInstanceMetadata {
    id: String,
    provider_id: String,
    provider_name: String,
    cli_kind: LivenessCliKind,
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

#[derive(Clone, PartialEq, Eq)]
struct FileSignature {
    len: u64,
    modified: Option<SystemTime>,
}

struct StableFile {
    text: String,
    modified_at: Option<u128>,
}

pub fn snapshot(providers: &[Provider]) -> CliRuntimeSnapshot {
    CliRuntimeSnapshot {
        codex: codex_config_snapshot(providers),
        claude_code: claude_config_snapshot(providers),
        instances: load_instances(),
    }
}

pub fn register_instance(
    provider: &Provider,
    cli_kind: LivenessCliKind,
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

pub(crate) fn activation_target(id: &str) -> Result<CliTerminalLocator, String> {
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

pub fn instance_by_id(id: &str) -> Result<TemporaryCliInstance, String> {
    let instance_dir = validated_instance_dir(id)?;
    load_instance(&instance_dir).ok_or_else(|| "临时 CLI 实例不存在或记录已损坏".to_string())
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

    let now = unix_millis();
    let mut instances = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let instance = load_instance(&path)?;
            let ended_at = instance
                .ended_at
                .as_deref()
                .and_then(|value| value.parse::<u128>().ok());
            if instance.status == TemporaryCliInstanceStatus::Exited
                && ended_at.is_some_and(|ended| now.saturating_sub(ended) > EXITED_RETENTION_MILLIS)
            {
                let _ = fs::remove_dir_all(path);
                return None;
            }
            Some(instance)
        })
        .collect::<Vec<_>>();

    instances.sort_by(|left, right| {
        numeric_timestamp(&right.started_at).cmp(&numeric_timestamp(&left.started_at))
    });
    instances.truncate(MAX_INSTANCE_HISTORY);
    instances
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

fn codex_config_snapshot(providers: &[Provider]) -> CliConfigSnapshot {
    let Some(home) = home_dir() else {
        return config_error("无法定位用户目录");
    };
    let config_path = home.join(".codex").join("config.toml");
    let auth_path = home.join(".codex").join("auth.json");
    let config = match read_stable_optional(&config_path) {
        Ok(value) => value,
        Err(_) => return config_error("读取 Codex 配置文件失败"),
    };
    let auth = match read_stable_optional(&auth_path) {
        Ok(value) => value,
        Err(_) => return config_error("读取 Codex 认证文件失败"),
    };
    let modified_at = latest_modified_at([config.as_ref(), auth.as_ref()]);
    let (Some(config), Some(auth)) = (config, auth) else {
        return CliConfigSnapshot {
            modified_at,
            ..CliConfigSnapshot::default()
        };
    };

    match parse_codex_config(&config.text, &auth.text) {
        Ok(Some((base_url, api_key))) => CliConfigSnapshot {
            configured: true,
            provider_id: match_provider(providers, LivenessCliKind::Codex, &base_url, &api_key),
            modified_at,
            error_message: None,
        },
        Ok(None) => CliConfigSnapshot {
            modified_at,
            ..CliConfigSnapshot::default()
        },
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error("Codex 配置文件格式无效")
        },
    }
}

fn claude_config_snapshot(providers: &[Provider]) -> CliConfigSnapshot {
    let Some(home) = home_dir() else {
        return config_error("无法定位用户目录");
    };
    let settings_path = home.join(".claude").join("settings.json");
    let settings = match read_stable_optional(&settings_path) {
        Ok(value) => value,
        Err(_) => return config_error("读取 Claude Code 配置文件失败"),
    };
    let modified_at = latest_modified_at([settings.as_ref()]);
    let Some(settings) = settings else {
        return CliConfigSnapshot {
            modified_at,
            ..CliConfigSnapshot::default()
        };
    };

    match parse_claude_config(&settings.text) {
        Ok(Some((base_url, api_key))) => CliConfigSnapshot {
            configured: true,
            provider_id: match_provider(
                providers,
                LivenessCliKind::ClaudeCode,
                &base_url,
                &api_key,
            ),
            modified_at,
            error_message: None,
        },
        Ok(None) => CliConfigSnapshot {
            modified_at,
            ..CliConfigSnapshot::default()
        },
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error("Claude Code 配置文件格式无效")
        },
    }
}

fn parse_codex_config(config: &str, auth: &str) -> Result<Option<(String, String)>, ()> {
    let config = config.parse::<toml::Value>().map_err(|_| ())?;
    let auth = serde_json::from_str::<JsonValue>(auth).map_err(|_| ())?;
    let provider_name = config.get("model_provider").and_then(toml::Value::as_str);
    let base_url = provider_name
        .and_then(|name| config.get("model_providers")?.get(name))
        .and_then(|provider| provider.get("base_url"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let api_key = auth
        .get("OPENAI_API_KEY")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    Ok(match (base_url, api_key) {
        (Some(base_url), Some(api_key)) => Some((base_url.to_string(), api_key.to_string())),
        _ => None,
    })
}

fn parse_claude_config(settings: &str) -> Result<Option<(String, String)>, ()> {
    let settings = serde_json::from_str::<JsonValue>(settings).map_err(|_| ())?;
    let env = settings.get("env").and_then(JsonValue::as_object);
    let base_url = env
        .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let api_key = env
        .and_then(|env| {
            env.get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| env.get("ANTHROPIC_API_KEY"))
        })
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    Ok(match (base_url, api_key) {
        (Some(base_url), Some(api_key)) => Some((base_url.to_string(), api_key.to_string())),
        _ => None,
    })
}

fn match_provider(
    providers: &[Provider],
    cli_kind: LivenessCliKind,
    base_url: &str,
    api_key: &str,
) -> Option<String> {
    let expected_url = normalize_endpoint(base_url)?;
    let expected_key = normalize_api_key(api_key);
    providers
        .iter()
        .find(|provider| {
            let provider_url = match cli_kind {
                LivenessCliKind::Codex => openai_base_url(provider),
                LivenessCliKind::ClaudeCode => anthropic_base_url(provider),
            };
            normalize_endpoint(&provider_url).as_deref() == Some(expected_url.as_str())
                && normalize_api_key(&provider.auth.api_key) == expected_key
        })
        .map(|provider| provider.identity.id.clone())
}

fn normalize_endpoint(value: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(value.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(if path.is_empty() { "/" } else { &path });
    Some(url.as_str().trim_end_matches('/').to_string())
}

fn config_error(message: &str) -> CliConfigSnapshot {
    CliConfigSnapshot {
        error_message: Some(message.to_string()),
        ..CliConfigSnapshot::default()
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env::var_os("USERPROFILE")
            .or_else(|| {
                let mut home = env::var_os("HOMEDRIVE")?;
                home.push(env::var_os("HOMEPATH")?);
                Some(home)
            })
            .map(PathBuf::from)
    }

    #[cfg(not(target_os = "windows"))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

fn read_stable_optional(path: &Path) -> Result<Option<StableFile>, String> {
    for _ in 0..2 {
        let before = match file_signature(path) {
            Ok(Some(signature)) => signature,
            Ok(None) => return Ok(None),
            Err(err) => return Err(err),
        };
        let text = fs::read_to_string(path)
            .map_err(|err| format!("读取文件失败({}): {err}", path.display()))?;
        let after = match file_signature(path)? {
            Some(signature) => signature,
            None => continue,
        };
        if before == after {
            return Ok(Some(StableFile {
                text,
                modified_at: after.modified.and_then(system_time_millis),
            }));
        }
    }
    Err(format!("文件读取期间发生变化({})", path.display()))
}

fn file_signature(path: &Path) -> Result<Option<FileSignature>, String> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(FileSignature {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("读取文件元数据失败({}): {err}", path.display())),
    }
}

fn latest_modified_at<const N: usize>(files: [Option<&StableFile>; N]) -> Option<String> {
    files
        .into_iter()
        .flatten()
        .filter_map(|file| file.modified_at)
        .max()
        .map(|value| value.to_string())
}

fn system_time_millis(value: SystemTime) -> Option<u128> {
    value
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

fn instances_dir() -> PathBuf {
    env::temp_dir()
        .join(RUNTIME_DIR_NAME)
        .join(INSTANCES_DIR_NAME)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("读取临时 CLI 记录失败({}): {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|_| format!("临时 CLI 记录格式无效({})", path.display()))
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("创建目录失败({}): {err}", parent.display()))?;
    }
    let text = serde_json::to_string(value).map_err(|err| format!("序列化记录失败: {err}"))?;
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
        format!("更新临时 CLI 记录失败({}): {err}", target.display())
    })
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, target: &Path) -> Result<(), String> {
    if target.exists() {
        fs::remove_file(target)
            .map_err(|err| format!("替换临时 CLI 记录失败({}): {err}", target.display()))?;
    }
    fs::rename(source, target).map_err(|err| {
        let _ = fs::remove_file(source);
        format!("更新临时 CLI 记录失败({}): {err}", target.display())
    })
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
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .any(|value| value == pid.to_string())
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
    use crate::models::{AuthMode, ProviderInput};

    fn relay_provider() -> Provider {
        Provider::from_input(
            ProviderInput {
                identity: crate::models::ProviderIdentityInput {
                    name: "Relay".to_string(),
                    base_url: "https://relay.example.com".to_string(),
                },
                auth: crate::models::ProviderAuth {
                    mode: AuthMode::ApiKey,
                    api_key: "sk-test".to_string(),
                    ..ProviderInput::default().auth
                },
                ..ProviderInput::default()
            },
            "provider-test".to_string(),
        )
    }

    #[test]
    fn parses_selected_codex_provider_and_auth_file() {
        let parsed = parse_codex_config(
            r#"
model_provider = "relay"

[model_providers.relay]
base_url = "https://relay.example.com/v1"
"#,
            r#"{"OPENAI_API_KEY":"sk-test"}"#,
        )
        .expect("config should parse")
        .expect("config should be complete");

        assert_eq!(parsed.0, "https://relay.example.com/v1");
        assert_eq!(parsed.1, "sk-test");
    }

    #[test]
    fn parses_claude_settings_env() {
        let parsed = parse_claude_config(
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://relay.example.com","ANTHROPIC_AUTH_TOKEN":"sk-test"}}"#,
        )
        .expect("settings should parse")
        .expect("settings should be complete");

        assert_eq!(parsed.0, "https://relay.example.com");
        assert_eq!(parsed.1, "sk-test");
    }

    #[test]
    fn endpoint_normalization_ignores_trailing_slashes_and_url_case_rules() {
        assert_eq!(
            normalize_endpoint("HTTPS://Relay.Example.COM/v1/"),
            normalize_endpoint("https://relay.example.com/v1")
        );
    }

    #[test]
    fn provider_match_requires_the_effective_url_and_api_key() {
        let provider = relay_provider();

        assert_eq!(
            match_provider(
                std::slice::from_ref(&provider),
                LivenessCliKind::Codex,
                "https://relay.example.com/v1/",
                "sk-test",
            ),
            Some("provider-test".to_string())
        );
        assert_eq!(
            match_provider(
                std::slice::from_ref(&provider),
                LivenessCliKind::ClaudeCode,
                "https://relay.example.com",
                "sk-other",
            ),
            None
        );
    }
}
