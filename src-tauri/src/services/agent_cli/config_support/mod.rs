mod env_source;
mod json_source;

pub(crate) use env_source::{env_value, rewrite_env_values};
pub(crate) use json_source::rewrite_json_string_fields;

use crate::{
    limits,
    models::{
        normalize_api_key_for_protocol, AgentCliKind, CliConfigFile, CliConfigSnapshot, Provider,
    },
    services::agent_cli,
    util::read_text_file_limited,
};
use std::{
    fs,
    hash::{Hash, Hasher},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};

static CONFIG_WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, PartialEq, Eq)]
struct FileSignature {
    len: u64,
    modified: Option<SystemTime>,
}

pub(crate) struct StableFile {
    pub text: String,
    pub modified_at: Option<u128>,
}

pub(crate) fn read_stable_optional(path: &Path) -> Result<Option<StableFile>, String> {
    for _ in 0..2 {
        let before = match file_signature(path) {
            Ok(Some(signature)) => signature,
            Ok(None) => return Ok(None),
            Err(err) => return Err(err),
        };
        let text =
            read_text_file_limited(path, limits::MAX_CLI_CONFIG_FILE_BYTES, "读取 CLI 配置文件")?;
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

pub(crate) fn latest_modified_at<'a>(
    files: impl IntoIterator<Item = Option<&'a StableFile>>,
) -> Option<String> {
    files
        .into_iter()
        .flatten()
        .filter_map(|file| file.modified_at)
        .max()
        .map(|value| value.to_string())
}

pub(crate) fn read_cli_config(path: &Path, context: &str) -> Result<String, String> {
    read_text_file_limited(path, limits::MAX_CLI_CONFIG_FILE_BYTES, context)
}

pub(crate) fn read_optional_cli_config(path: &Path) -> Result<String, String> {
    Ok(read_stable_optional(path)?
        .map(|file| file.text)
        .unwrap_or_default())
}

pub(crate) fn restore_config_file(
    path: &Path,
    original: Option<&str>,
    label: &str,
) -> Result<(), String> {
    if let Some(original) = original {
        return write_config_text(path, original, label);
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!(
            "删除{label}产生的新文件失败({}): {err}",
            path.display()
        )),
    }
}

pub(crate) fn file_content<'a>(files: &'a [CliConfigFile], path: &Path) -> Result<&'a str, String> {
    let expected = path.to_string_lossy();
    let matches = files
        .iter()
        .filter(|file| file.file_path == expected)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!("缺少或重复提交 CLI 配置文件：{}", path.display()));
    }
    let content = matches[0].content.as_str();
    if content.len() > limits::MAX_CLI_CONFIG_FILE_BYTES {
        return Err(format!("CLI 配置文件过大：{}", path.display()));
    }
    Ok(content)
}

pub(crate) fn validate_file_set(
    files: &[CliConfigFile],
    expected_paths: &[&Path],
) -> Result<(), String> {
    if files.len() != expected_paths.len()
        || files.iter().any(|file| {
            !expected_paths
                .iter()
                .any(|path| file.file_path == path.to_string_lossy())
        })
    {
        return Err("只能提交当前 CLI 预览中的配置文件".to_string());
    }
    Ok(())
}

pub(crate) fn config_revision(parts: &[&str]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for part in parts {
        part.len().hash(&mut hasher);
        part.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

pub(crate) fn ensure_revision(expected: Option<&str>, actual: String) -> Result<(), String> {
    if let Some(expected) = expected.filter(|value| !value.trim().is_empty()) {
        if expected != actual {
            return Err("CLI 配置文件在预览后发生变化，请重新打开预览".to_string());
        }
    }
    Ok(())
}

pub(crate) fn cli_target(
    provider: &Provider,
    cli_kind: AgentCliKind,
) -> Result<(String, String), String> {
    let api_key =
        normalize_api_key_for_protocol(&provider.auth.api_key, provider.identity.protocol);
    if api_key.is_empty() {
        return Err("中转站缺少 API Key，无法切换 CLI 配置".to_string());
    }
    let base_url = agent_cli::provider_base_url(cli_kind, provider);
    if normalize_endpoint(&base_url).is_none() {
        return Err("中转站地址无效，无法切换 CLI 配置".to_string());
    }
    Ok((base_url, api_key))
}

pub(crate) fn match_provider(
    providers: &[Provider],
    cli_kind: AgentCliKind,
    base_url: &str,
    api_key: &str,
) -> Option<String> {
    let expected_url = normalize_endpoint(base_url)?;
    providers
        .iter()
        .find(|provider| {
            let provider_url = agent_cli::provider_base_url(cli_kind, provider);
            normalize_endpoint(&provider_url).as_deref() == Some(expected_url.as_str())
                && normalize_api_key_for_protocol(
                    &provider.auth.api_key,
                    provider.identity.protocol,
                ) == normalize_api_key_for_protocol(api_key, provider.identity.protocol)
        })
        .map(|provider| provider.identity.id.clone())
}

pub(crate) fn normalize_endpoint(value: &str) -> Option<String> {
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

pub(crate) fn config_error(cli_kind: AgentCliKind, message: &str) -> CliConfigSnapshot {
    CliConfigSnapshot {
        cli_kind,
        configured: false,
        provider_id: None,
        modified_at: None,
        error_message: Some(message.to_string()),
    }
}

pub(crate) fn write_config_text(path: &Path, text: &str, label: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("创建{label}目录失败({}): {err}", parent.display()))?;
    }
    let sequence = CONFIG_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = path.with_extension(format!("balancehub-{}-{sequence}.tmp", std::process::id()));
    fs::write(&tmp_path, text)
        .map_err(|err| format!("写入{label}临时文件失败({}): {err}", tmp_path.display()))?;
    if let Ok(metadata) = fs::metadata(path) {
        if let Err(err) = fs::set_permissions(&tmp_path, metadata.permissions()) {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("保留{label}文件权限失败: {err}"));
        }
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(err) = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600)) {
                let _ = fs::remove_file(&tmp_path);
                return Err(format!("设置{label}文件权限失败: {err}"));
            }
        }
    }
    replace_file(&tmp_path, path).map_err(|err| format!("更新{label}失败: {err}"))
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
    let sequence = CONFIG_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
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

fn system_time_millis(value: SystemTime) -> Option<u128> {
    value
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}
