use crate::{
    models::{
        normalize_api_key, CliConfigChange, CliConfigPreview, CliConfigSnapshot,
        CliRuntimeSnapshot, LivenessCliKind, Provider, TemporaryCliInstance,
        TemporaryCliInstanceStatus, TemporaryCliTerminalKind,
    },
    services::liveness::{anthropic_base_url, openai_base_url},
    util::unix_millis,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{
    env, fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};
use toml_edit::{value as toml_value, Document as TomlDocument};

const RUNTIME_DIR_NAME: &str = "balancehub-cli-runtime-v1";
const INSTANCES_DIR_NAME: &str = "instances";
const METADATA_FILE_NAME: &str = "instance.json";
const STATUS_FILE_NAME: &str = "status.json";
const STARTING_TIMEOUT_MILLIS: u128 = 2 * 60 * 1000;
const UNKNOWN_PID_TIMEOUT_MILLIS: u128 = 24 * 60 * 60 * 1000;
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

pub fn active_instances() -> Vec<TemporaryCliInstance> {
    load_instances()
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

#[cfg(test)]
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

    let mut instances = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let instance = load_instance(&path)?;
            if instance.status == TemporaryCliInstanceStatus::Exited {
                let _ = fs::remove_dir_all(path);
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

pub fn preview_config(
    provider: &Provider,
    cli_kind: LivenessCliKind,
) -> Result<CliConfigPreview, String> {
    let (base_url, api_key) = cli_target(provider, cli_kind)?;
    let home = home_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let mut changes = Vec::new();

    let revision = match cli_kind {
        LivenessCliKind::Codex => {
            let config_path = home.join(".codex").join("config.toml");
            let auth_path = home.join(".codex").join("auth.json");
            let config_text = fs::read_to_string(&config_path).map_err(|err| {
                format!("读取 Codex 配置文件失败({}): {err}", config_path.display())
            })?;
            let auth_text = fs::read_to_string(&auth_path).map_err(|err| {
                format!("读取 Codex 认证文件失败({}): {err}", auth_path.display())
            })?;
            let provider_document = config_text
                .parse::<TomlDocument>()
                .map_err(|_| "Codex 配置文件格式无效".to_string())?;
            let provider_name = codex_provider_name(&provider_document)?;
            let before_url = provider_document
                .get("model_providers")
                .and_then(toml_edit::Item::as_table_like)
                .and_then(|providers| providers.get(&provider_name))
                .and_then(toml_edit::Item::as_table_like)
                .and_then(|selected| selected.get("base_url"))
                .and_then(toml_edit::Item::as_str);
            let auth_document = serde_json::from_str::<JsonValue>(&auth_text)
                .map_err(|_| "Codex 认证文件格式无效".to_string())?;
            let before_key = auth_document
                .get("OPENAI_API_KEY")
                .and_then(JsonValue::as_str);
            push_config_change(
                &mut changes,
                config_path.to_string_lossy().as_ref(),
                &format!("model_providers.{provider_name}.base_url"),
                before_url,
                Some(base_url.as_str()),
                false,
            );
            push_config_change(
                &mut changes,
                auth_path.to_string_lossy().as_ref(),
                "OPENAI_API_KEY",
                before_key,
                Some(api_key.as_str()),
                true,
            );
            config_revision(&[&config_text, &auth_text, &base_url, &api_key])
        }
        LivenessCliKind::ClaudeCode => {
            let settings_path = home.join(".claude").join("settings.json");
            let settings_text = fs::read_to_string(&settings_path).map_err(|err| {
                format!(
                    "读取 Claude Code 配置文件失败({}): {err}",
                    settings_path.display()
                )
            })?;
            let settings = serde_json::from_str::<JsonValue>(&settings_text)
                .map_err(|_| "Claude Code 配置文件格式无效".to_string())?;
            let env = settings.get("env").and_then(JsonValue::as_object);
            let next_settings = rewrite_claude_config(&settings_text, &base_url, &api_key)?;
            let next = serde_json::from_str::<JsonValue>(&next_settings)
                .map_err(|_| "Claude Code 配置文件格式无效".to_string())?;
            let next_env = next.get("env").and_then(JsonValue::as_object);
            for (field, sensitive) in [
                ("ANTHROPIC_BASE_URL", false),
                ("ANTHROPIC_AUTH_TOKEN", true),
                ("ANTHROPIC_API_KEY", true),
            ] {
                let before = env
                    .and_then(|values| values.get(field))
                    .and_then(JsonValue::as_str);
                let after = next_env
                    .and_then(|values| values.get(field))
                    .and_then(JsonValue::as_str);
                push_config_change(
                    &mut changes,
                    settings_path.to_string_lossy().as_ref(),
                    &format!("env.{field}"),
                    before,
                    after,
                    sensitive,
                );
            }
            config_revision(&[&settings_text, &base_url, &api_key])
        }
    };

    Ok(CliConfigPreview {
        provider_id: provider.identity.id.clone(),
        provider_name: provider.identity.name.clone(),
        cli_kind,
        revision,
        changes,
    })
}

pub fn switch_config(
    provider: &Provider,
    cli_kind: LivenessCliKind,
    expected_revision: Option<&str>,
) -> Result<(), String> {
    let (base_url, api_key) = cli_target(provider, cli_kind)?;
    match cli_kind {
        LivenessCliKind::Codex => switch_codex_config(&base_url, &api_key, expected_revision),
        LivenessCliKind::ClaudeCode => switch_claude_config(&base_url, &api_key, expected_revision),
    }
}

fn cli_target(provider: &Provider, cli_kind: LivenessCliKind) -> Result<(String, String), String> {
    let api_key = normalize_api_key(&provider.auth.api_key);
    if api_key.is_empty() {
        return Err("中转站缺少 API Key，无法切换 CLI 配置".to_string());
    }

    let base_url = match cli_kind {
        LivenessCliKind::Codex => openai_base_url(provider),
        LivenessCliKind::ClaudeCode => anthropic_base_url(provider),
    };
    if normalize_endpoint(&base_url).is_none() {
        return Err("中转站地址无效，无法切换 CLI 配置".to_string());
    }
    Ok((base_url, api_key))
}

fn switch_codex_config(
    base_url: &str,
    api_key: &str,
    expected_revision: Option<&str>,
) -> Result<(), String> {
    let home = home_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let config_path = home.join(".codex").join("config.toml");
    let auth_path = home.join(".codex").join("auth.json");
    let config_text = fs::read_to_string(&config_path)
        .map_err(|err| format!("读取 Codex 配置文件失败({}): {err}", config_path.display()))?;
    let auth_text = fs::read_to_string(&auth_path)
        .map_err(|err| format!("读取 Codex 认证文件失败({}): {err}", auth_path.display()))?;
    ensure_revision(
        expected_revision,
        config_revision(&[&config_text, &auth_text, base_url, api_key]),
    )?;
    let (next_config, next_auth) =
        rewrite_codex_config(&config_text, &auth_text, base_url, api_key)?;

    write_config_text(&config_path, &next_config, "Codex 配置")?;
    if let Err(err) = write_config_text(&auth_path, &next_auth, "Codex 认证") {
        let rollback_error = write_config_text(&config_path, &config_text, "Codex 配置回滚").err();
        return Err(match rollback_error {
            Some(rollback) => format!("{err}；{rollback}"),
            None => err,
        });
    }
    Ok(())
}

fn switch_claude_config(
    base_url: &str,
    api_key: &str,
    expected_revision: Option<&str>,
) -> Result<(), String> {
    let home = home_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let settings_path = home.join(".claude").join("settings.json");
    let settings_text = fs::read_to_string(&settings_path).map_err(|err| {
        format!(
            "读取 Claude Code 配置文件失败({}): {err}",
            settings_path.display()
        )
    })?;
    ensure_revision(
        expected_revision,
        config_revision(&[&settings_text, base_url, api_key]),
    )?;
    let next_settings = rewrite_claude_config(&settings_text, base_url, api_key)?;
    write_config_text(&settings_path, &next_settings, "Claude Code 配置")
}

fn codex_provider_name(document: &TomlDocument) -> Result<String, String> {
    document
        .get("model_provider")
        .and_then(toml_edit::Item::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Codex 配置缺少 model_provider，无法只更新中转站地址".to_string())
}

fn push_config_change(
    changes: &mut Vec<CliConfigChange>,
    file_path: &str,
    field_path: &str,
    before: Option<&str>,
    after: Option<&str>,
    sensitive: bool,
) {
    if before == after {
        return;
    }
    changes.push(CliConfigChange {
        file_path: file_path.to_string(),
        field_path: field_path.to_string(),
        before_value: before.map(str::to_string),
        after_value: after.map(str::to_string),
        sensitive,
    });
}

fn config_revision(parts: &[&str]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for part in parts {
        part.len().hash(&mut hasher);
        part.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn ensure_revision(expected: Option<&str>, actual: String) -> Result<(), String> {
    if let Some(expected) = expected.filter(|value| !value.trim().is_empty()) {
        if expected != actual {
            return Err("CLI 配置文件在预览后发生变化，请重新打开预览".to_string());
        }
    }
    Ok(())
}

fn rewrite_codex_config(
    config: &str,
    auth: &str,
    base_url: &str,
    api_key: &str,
) -> Result<(String, String), String> {
    let mut document = config
        .parse::<TomlDocument>()
        .map_err(|_| "Codex 配置文件格式无效".to_string())?;
    let provider_name = document
        .get("model_provider")
        .and_then(toml_edit::Item::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Codex 配置缺少 model_provider，无法只更新中转站地址".to_string())?;
    let providers = document
        .get_mut("model_providers")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| "Codex 配置缺少 model_providers".to_string())?;
    let selected = providers
        .get_mut(&provider_name)
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| format!("Codex 配置缺少当前 provider：{provider_name}"))?;
    selected.insert("base_url", toml_value(base_url.trim()));

    let mut auth = serde_json::from_str::<JsonValue>(auth)
        .map_err(|_| "Codex 认证文件格式无效".to_string())?;
    let auth = auth
        .as_object_mut()
        .ok_or_else(|| "Codex 认证文件格式无效".to_string())?;
    auth.insert(
        "OPENAI_API_KEY".to_string(),
        JsonValue::String(api_key.trim().to_string()),
    );
    let auth = serde_json::to_string_pretty(auth)
        .map_err(|err| format!("生成 Codex 认证配置失败: {err}"))?;

    Ok((document.to_string(), format!("{auth}\n")))
}

fn rewrite_claude_config(settings: &str, base_url: &str, api_key: &str) -> Result<String, String> {
    let mut settings = serde_json::from_str::<JsonValue>(settings)
        .map_err(|_| "Claude Code 配置文件格式无效".to_string())?;
    let settings = settings
        .as_object_mut()
        .ok_or_else(|| "Claude Code 配置文件格式无效".to_string())?;
    let env = settings
        .entry("env".to_string())
        .or_insert_with(|| JsonValue::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "Claude Code 配置中的 env 不是对象".to_string())?;
    env.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        JsonValue::String(base_url.trim().to_string()),
    );

    let has_auth_token = env.contains_key("ANTHROPIC_AUTH_TOKEN");
    let has_api_key = env.contains_key("ANTHROPIC_API_KEY");
    if has_auth_token || !has_api_key {
        env.insert(
            "ANTHROPIC_AUTH_TOKEN".to_string(),
            JsonValue::String(api_key.trim().to_string()),
        );
    }
    if has_api_key {
        env.insert(
            "ANTHROPIC_API_KEY".to_string(),
            JsonValue::String(api_key.trim().to_string()),
        );
    }

    serde_json::to_string_pretty(settings)
        .map(|text| format!("{text}\n"))
        .map_err(|err| format!("生成 Claude Code 配置失败: {err}"))
}

fn write_config_text(path: &Path, text: &str, label: &str) -> Result<(), String> {
    let sequence = INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = path.with_extension(format!("balancehub-{}-{sequence}.tmp", std::process::id()));
    fs::write(&tmp_path, text)
        .map_err(|err| format!("写入{label}临时文件失败({}): {err}", tmp_path.display()))?;
    if let Ok(metadata) = fs::metadata(path) {
        if let Err(err) = fs::set_permissions(&tmp_path, metadata.permissions()) {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("保留{label}文件权限失败: {err}"));
        }
    }
    replace_file(&tmp_path, path).map_err(|err| format!("更新{label}失败: {err}"))
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
                    ..crate::models::ProviderIdentityInput::default()
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
    fn codex_switch_only_updates_selected_provider_url_and_api_key() {
        let config = r#"model_provider = "relay"
model = "gpt-test"

[model_providers.relay]
name = "Relay"
base_url = "https://old.example.com/v1"
wire_api = "responses"

[mcp_servers.local]
command = "node"
"#;
        let auth = r#"{"OPENAI_API_KEY":"sk-old","tokens":{"access":"keep"}}"#;

        let (config, auth) =
            rewrite_codex_config(config, auth, "https://new.example.com/v1", "sk-new").unwrap();

        assert!(config.contains("base_url = \"https://new.example.com/v1\""));
        assert!(config.contains("model = \"gpt-test\""));
        assert!(config.contains("[mcp_servers.local]"));
        let auth = serde_json::from_str::<JsonValue>(&auth).unwrap();
        assert_eq!(auth["OPENAI_API_KEY"], "sk-new");
        assert_eq!(auth["tokens"]["access"], "keep");
    }

    #[test]
    fn claude_switch_preserves_other_settings_and_updates_existing_key_fields() {
        let settings = r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://old.example.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-old",
    "ANTHROPIC_API_KEY": "sk-old-api",
    "KEEP_ME": "yes"
  },
  "permissions": { "defaultMode": "bypassPermissions" }
}"#;

        let settings =
            rewrite_claude_config(settings, "https://new.example.com", "sk-new").unwrap();
        let settings = serde_json::from_str::<JsonValue>(&settings).unwrap();

        assert_eq!(
            settings["env"]["ANTHROPIC_BASE_URL"],
            "https://new.example.com"
        );
        assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-new");
        assert_eq!(settings["env"]["ANTHROPIC_API_KEY"], "sk-new");
        assert_eq!(settings["env"]["KEEP_ME"], "yes");
        assert_eq!(settings["permissions"]["defaultMode"], "bypassPermissions");
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
