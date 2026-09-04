use super::inventory::{
    asset_stable_id, declaration_paths, native_environment, normalize_optional_workspace,
    path_has_symlink_component, system_time_millis,
};
use crate::{
    limits,
    models::{AgentAssetOpenTarget, AgentAssetReadResult},
    services::{agent_cli::definitions, cli_paths::user_home},
};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

const MAX_PREVIEW_BYTES: usize = 128 * 1024;

#[derive(Debug)]
pub(super) struct ResolvedAsset {
    pub(super) path: PathBuf,
    pub(super) sensitive: bool,
    pub(super) is_directory: bool,
}

fn resolve_asset(asset_id: &str, workspace: Option<&Path>) -> Result<ResolvedAsset, String> {
    let home = user_home().ok_or_else(|| "无法定位用户目录".to_string())?;
    let workspace = normalize_optional_workspace(workspace)?;
    resolve_asset_in(asset_id, &home, workspace.as_deref())
}

pub(super) fn resolve_asset_in(
    asset_id: &str,
    home: &Path,
    workspace: Option<&Path>,
) -> Result<ResolvedAsset, String> {
    let environment = native_environment();
    for registered in definitions() {
        for declaration in registered.environment().discover(home, workspace) {
            for resolved in declaration_paths(&declaration) {
                if asset_stable_id(&environment, registered.kind, &declaration, &resolved.path)
                    != asset_id
                {
                    continue;
                }
                return Ok(ResolvedAsset {
                    path: resolved.path,
                    sensitive: declaration.sensitive,
                    is_directory: resolved.is_directory,
                });
            }
        }
    }
    Err("资产标识无效或已过期，请重新扫描 Agent 环境".to_string())
}

pub(crate) fn open_asset_path(
    asset_id: &str,
    workspace: Option<&Path>,
    target: AgentAssetOpenTarget,
) -> Result<PathBuf, String> {
    let resolved = resolve_asset(asset_id, workspace)?;
    match target {
        AgentAssetOpenTarget::Asset => {
            validate_resolved_path(&resolved.path, resolved.is_directory)?;
            Ok(resolved.path)
        }
        AgentAssetOpenTarget::ParentDirectory => {
            let parent = resolved
                .path
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| "该资产没有可打开的上级目录".to_string())?;
            validate_resolved_path(&parent, true)?;
            Ok(parent)
        }
    }
}

pub(crate) fn read_asset(
    asset_id: &str,
    workspace: Option<&Path>,
) -> Result<AgentAssetReadResult, String> {
    let resolved = resolve_asset(asset_id, workspace)?;
    validate_resolved_path(&resolved.path, resolved.is_directory)?;
    read_resolved_asset(asset_id, resolved)
}

pub(super) fn read_resolved_asset(
    asset_id: &str,
    resolved: ResolvedAsset,
) -> Result<AgentAssetReadResult, String> {
    let path = resolved.path;
    let metadata =
        fs::symlink_metadata(&path).map_err(|err| format!("读取资产元数据失败: {err}"))?;
    if metadata.file_type().is_symlink() || resolved.is_directory {
        return Ok(AgentAssetReadResult {
            stable_id: asset_id.to_string(),
            path: path.display().to_string(),
            content: None,
            size_bytes: metadata.len(),
            modified_at: metadata
                .modified()
                .ok()
                .and_then(system_time_millis)
                .map(|v| v.to_string()),
            truncated: false,
            metadata_only: true,
            diagnostic: Some("目录或符号链接仅提供元数据".to_string()),
        });
    }
    if metadata.len() > limits::MAX_CLI_CONFIG_FILE_BYTES as u64 {
        return Err("配置文件超过允许读取大小".to_string());
    }
    let metadata_only = resolved.sensitive || is_secret_path(&path);
    if metadata_only {
        return Ok(AgentAssetReadResult {
            stable_id: asset_id.to_string(),
            path: path.display().to_string(),
            content: None,
            size_bytes: metadata.len(),
            modified_at: metadata
                .modified()
                .ok()
                .and_then(system_time_millis)
                .map(|v| v.to_string()),
            truncated: false,
            metadata_only: true,
            diagnostic: Some("敏感凭据文件仅提供元数据".to_string()),
        });
    }
    let (text, truncated) = read_preview_limited(&path)?;
    Ok(AgentAssetReadResult {
        stable_id: asset_id.to_string(),
        path: path.display().to_string(),
        content: Some(redact_preview(&text)),
        size_bytes: metadata.len(),
        modified_at: metadata
            .modified()
            .ok()
            .and_then(system_time_millis)
            .map(|v| v.to_string()),
        truncated,
        metadata_only: false,
        diagnostic: None,
    })
}

fn read_preview_limited(path: &Path) -> Result<(String, bool), String> {
    let file = File::open(path)
        .map_err(|error| format!("读取 Agent 配置预览失败({}): {error}", path.display()))?;
    let mut bytes = Vec::with_capacity(MAX_PREVIEW_BYTES.min(64 * 1024));
    file.take(MAX_PREVIEW_BYTES.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("读取 Agent 配置预览失败({}): {error}", path.display()))?;
    let truncated = bytes.len() > MAX_PREVIEW_BYTES;
    if truncated {
        bytes.truncate(MAX_PREVIEW_BYTES);
        while let Err(error) = std::str::from_utf8(&bytes) {
            if error.error_len().is_some() {
                return Err(format!(
                    "读取 Agent 配置预览失败({})：文件不是有效 UTF-8",
                    path.display()
                ));
            }
            bytes.truncate(error.valid_up_to());
        }
    }
    let text = String::from_utf8(bytes).map_err(|error| {
        format!(
            "读取 Agent 配置预览失败({})：文件不是有效 UTF-8：{error}",
            path.display()
        )
    })?;
    Ok((text, truncated))
}

pub(super) fn validate_resolved_path(path: &Path, is_directory: bool) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|err| format!("资产不存在: {err}"))?;
    if metadata.file_type().is_symlink() || path_has_symlink_component(path) {
        return Err("出于安全原因不支持打开符号链接资产".to_string());
    }
    if is_directory != metadata.is_dir() {
        return Err("资产类型已发生变化，请重新扫描 Agent 环境".to_string());
    }
    Ok(())
}

fn is_secret_path(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    name == ".env"
        || name.contains("auth")
        || name.contains("credential")
        || name.contains("secret")
}

pub(super) fn redact_preview(text: &str) -> String {
    text.lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            let sensitive = [
                "api_key",
                "apikey",
                "token",
                "cookie",
                "password",
                "secret",
                "authorization",
            ]
            .iter()
            .any(|key| lower.contains(key));
            if !sensitive {
                return line.to_string();
            }
            if let Some(index) = line.find(':').or_else(|| line.find('=')) {
                format!("{} <已隐藏>", &line[..=index])
            } else {
                "<已隐藏敏感配置行>".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
