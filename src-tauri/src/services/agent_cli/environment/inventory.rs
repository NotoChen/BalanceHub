use super::versioning::version_channel;
use crate::services::agent_cli::{contracts::AgentAssetDeclaration, definitions};
use crate::{
    models::{
        AgentAssetCapability, AgentAssetCategory, AgentAssetScope, AgentAssetSource,
        AgentAssetState, AgentCliKind, AgentDiscoverySource, AgentEnvironmentCapability,
        AgentEnvironmentDescriptor, AgentEnvironmentInventory, AgentEnvironmentKind,
        AgentHostPlatform, AgentInstallation, AgentInstallationAvailability,
        AgentInstallationChannel, AgentVersionSource, AgentVersionState,
    },
    services::{agent_cli, cli_paths::user_home},
};
use chrono::{DateTime, Local};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    time::SystemTime,
};
pub(crate) const MAX_INVENTORY_CHILDREN: usize = 128;

#[derive(Clone, Copy)]
pub(crate) enum AssetRoot {
    User,
    Workspace,
}

#[derive(Clone, Copy)]
pub(crate) struct AssetTemplate {
    pub category: AgentAssetCategory,
    pub native_id: &'static str,
    pub label: &'static str,
    pub root: AssetRoot,
    pub relative_path: &'static str,
    pub scope: AgentAssetScope,
    pub precedence: u32,
    pub sensitive: bool,
    pub is_directory: bool,
}

pub(crate) fn from_templates(
    home: &Path,
    workspace: Option<&Path>,
    templates: &[AssetTemplate],
) -> Vec<AgentAssetDeclaration> {
    templates
        .iter()
        .filter_map(|template| {
            let root = match template.root {
                AssetRoot::User => home,
                AssetRoot::Workspace => workspace?,
            };
            let path = safe_relative_path(root, template.relative_path)?;
            Some(AgentAssetDeclaration {
                category: template.category,
                native_id: template.native_id,
                label: template.label,
                path,
                scope: template.scope,
                precedence: template.precedence,
                writable: matches!(
                    template.scope,
                    AgentAssetScope::User | AgentAssetScope::Workspace | AgentAssetScope::Local
                ),
                sensitive: template.sensitive,
                is_directory: template.is_directory,
            })
        })
        .collect()
}

pub(super) fn safe_relative_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    Some(root.join(relative_path))
}

pub(crate) fn inventory(
    settings: &crate::models::AppSettings,
    workspace: Option<&Path>,
) -> Result<AgentEnvironmentInventory, String> {
    let home = user_home().ok_or_else(|| "无法定位用户目录".to_string())?;
    let workspace = normalize_optional_workspace(workspace)?;
    let environment = native_environment();
    let workspace_string = workspace.as_ref().map(|path| path.display().to_string());
    let mut installations = Vec::new();
    let mut sources = BTreeMap::new();
    let mut assets = Vec::new();
    let mut capabilities = Vec::new();

    let discovered = std::thread::scope(|scope| {
        let handles = definitions()
            .iter()
            .map(|registered| {
                scope.spawn(move || {
                    (
                        registered,
                        agent_cli::find(settings, registered.kind, false),
                    )
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .zip(definitions())
            .map(|(handle, registered)| {
                handle.join().unwrap_or_else(|_| {
                    (
                        registered,
                        Err(format!("{} CLI 自动检测异常", registered.label)),
                    )
                })
            })
            .collect::<Vec<_>>()
    });

    for (registered, executable) in discovered {
        let kind = registered.kind;
        let (executable_path, installed_version, diagnostic) = match executable {
            Ok(value) => (Some(value.path), Some(value.version), None),
            Err(error) => (None, None, Some(error)),
        };
        let channel = installed_version
            .as_deref()
            .map(version_channel)
            .unwrap_or(AgentInstallationChannel::Unknown);
        let installed_version_source = if installed_version.is_some() {
            AgentVersionSource::LocalExecutable
        } else {
            AgentVersionSource::Unknown
        };
        let installation_id = stable_id("installation", &[&environment.id, kind.key()]);
        installations.push(AgentInstallation {
            id: installation_id,
            environment_id: environment.id.clone(),
            agent_kind: kind,
            label: registered.label.to_string(),
            availability: if executable_path.is_some() {
                AgentInstallationAvailability::Available
            } else {
                AgentInstallationAvailability::Unavailable
            },
            executable_path,
            installed_version,
            discovery_source: if settings.agent_cli_path(kind).trim().is_empty() {
                AgentDiscoverySource::Automatic
            } else {
                AgentDiscoverySource::Configured
            },
            channel,
            installed_version_source,
            latest_stable_version: None,
            latest_version_source: AgentVersionSource::Unknown,
            version_state: AgentVersionState::Unknown,
            version_checked_at: None,
            diagnostic,
        });
        let declarations = registered
            .environment()
            .discover(&home, workspace.as_deref());
        capabilities.push(crate::models::AgentCapabilities {
            agent_kind: kind,
            assets: capabilities_for(&declarations),
        });
        for declaration in declarations {
            let declaration_workspace = matches!(
                declaration.scope,
                AgentAssetScope::Workspace | AgentAssetScope::Local
            )
            .then(|| workspace_string.clone())
            .flatten();
            let source_id = stable_id(
                "source",
                &[
                    environment.id.as_str(),
                    kind.key(),
                    category_key(declaration.category),
                    declaration.scope_key(),
                    declaration.native_id,
                    &declaration.path.to_string_lossy(),
                ],
            );
            sources
                .entry(source_id.clone())
                .or_insert(AgentAssetSource {
                    id: source_id.clone(),
                    scope: declaration.scope,
                    environment_id: environment.id.clone(),
                    workspace_id: declaration_workspace.clone(),
                    path: declaration.path.to_string_lossy().into_owned(),
                    precedence: declaration.precedence,
                    writable: declaration.writable,
                });
            assets.extend(expand_declaration(
                kind,
                &environment,
                declaration_workspace.as_deref(),
                declaration,
                &source_id,
            ));
        }
    }
    assets.sort_by(|left, right| left.stable_id.cmp(&right.stable_id));
    Ok(AgentEnvironmentInventory {
        environment,
        installations,
        sources: sources.into_values().collect(),
        capabilities,
        assets,
        scanned_at: now_string(),
        workspace: workspace_string,
    })
}

pub(super) fn native_environment() -> AgentEnvironmentDescriptor {
    let platform = native_host_platform();
    let platform_label = match platform {
        AgentHostPlatform::Macos => "macOS",
        AgentHostPlatform::Linux => "Linux",
        AgentHostPlatform::Windows => "Windows",
    };
    AgentEnvironmentDescriptor {
        id: format!("native:{}", env::consts::OS),
        kind: AgentEnvironmentKind::Native,
        host_platform: platform,
        guest_platform: None,
        display_name: format!("本机 ({platform_label})"),
        capabilities: vec![
            AgentEnvironmentCapability::ReadOnlyInventory,
            AgentEnvironmentCapability::BoundedPreview,
        ],
    }
}

fn native_host_platform() -> AgentHostPlatform {
    #[cfg(target_os = "macos")]
    return AgentHostPlatform::Macos;
    #[cfg(target_os = "linux")]
    return AgentHostPlatform::Linux;
    #[cfg(target_os = "windows")]
    return AgentHostPlatform::Windows;
}

fn capabilities_for(declarations: &[AgentAssetDeclaration]) -> Vec<AgentAssetCapability> {
    let mut categories =
        BTreeMap::<&'static str, (AgentAssetCategory, BTreeSet<&'static str>)>::new();
    for declaration in declarations {
        categories
            .entry(category_key(declaration.category))
            .or_insert_with(|| (declaration.category, BTreeSet::new()))
            .1
            .insert(scope_key(declaration.scope));
    }
    categories
        .into_values()
        .map(|(category, scopes)| AgentAssetCapability {
            category,
            discovery: scopes.into_iter().filter_map(scope_from_key).collect(),
            mutation: crate::models::AgentAssetMutation::ReadOnly,
            requires_restart: false,
            // Trust is source- and Agent-specific. Until an adapter parses the relevant
            // trust store, claiming that it is required would be as misleading as claiming
            // that the asset is trusted.
            requires_trust: false,
        })
        .collect()
}

fn expand_declaration(
    kind: AgentCliKind,
    environment: &AgentEnvironmentDescriptor,
    workspace_id: Option<&str>,
    declaration: AgentAssetDeclaration,
    source_id: &str,
) -> Vec<crate::models::AgentAssetRecord> {
    let context = AssetRecordContext {
        kind,
        environment,
        workspace_id,
    };
    declaration_paths(&declaration)
        .into_iter()
        .map(|resolved| {
            asset_record(
                &context,
                &declaration,
                source_id,
                resolved.path,
                resolved.is_directory,
                resolved.revision,
            )
        })
        .collect()
}

pub(super) struct DeclarationPath {
    pub(super) path: PathBuf,
    pub(super) is_directory: bool,
    pub(super) revision: Option<String>,
}

pub(super) fn declaration_paths(declaration: &AgentAssetDeclaration) -> Vec<DeclarationPath> {
    let metadata = fs::symlink_metadata(&declaration.path).ok();
    let mut paths = vec![DeclarationPath {
        path: declaration.path.clone(),
        is_directory: declaration.is_directory,
        revision: metadata.as_ref().map(file_revision),
    }];
    if !declaration.is_directory || !metadata.as_ref().is_some_and(|value| value.is_dir()) {
        return paths;
    }
    let Ok(entries) = fs::read_dir(&declaration.path) else {
        return paths;
    };
    for entry in entries.flatten().take(MAX_INVENTORY_CHILDREN) {
        let Ok(entry_type) = entry.file_type() else {
            continue;
        };
        if entry_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        paths.push(DeclarationPath {
            revision: fs::symlink_metadata(&path).ok().as_ref().map(file_revision),
            path,
            is_directory: entry_type.is_dir(),
        });
    }
    paths
}

fn asset_record(
    context: &AssetRecordContext<'_>,
    declaration: &AgentAssetDeclaration,
    source_id: &str,
    path: PathBuf,
    is_directory: bool,
    revision: Option<String>,
) -> crate::models::AgentAssetRecord {
    let metadata = fs::symlink_metadata(&path).ok();
    let exists = metadata.is_some();
    let is_symlink = metadata
        .as_ref()
        .is_some_and(|value| value.file_type().is_symlink());
    let mut diagnostics = Vec::new();
    if !exists {
        diagnostics.push("资源不存在".to_string());
    }
    if is_symlink {
        diagnostics.push("为符号链接，出于安全原因不读取其内容".to_string());
    }
    let state = if is_symlink {
        AgentAssetState::Blocked
    } else if !exists {
        AgentAssetState::Unknown
    } else {
        // Presence alone cannot establish native enablement, trust or precedence.
        AgentAssetState::Unknown
    };
    let path_text = path.to_string_lossy().into_owned();
    let is_child = path != declaration.path;
    let native_id = if is_child {
        format!(
            "{}:{}",
            declaration.native_id,
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("child")
        )
    } else {
        declaration.native_id.to_string()
    };
    let label = if is_child {
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(declaration.label)
            .to_string()
    } else {
        declaration.label.to_string()
    };
    crate::models::AgentAssetRecord {
        stable_id: asset_stable_id(context.environment, context.kind, declaration, &path),
        agent_kind: context.kind,
        category: declaration.category,
        native_id,
        label,
        source_id: source_id.to_string(),
        scope: declaration.scope,
        environment_id: context.environment.id.clone(),
        workspace_id: context.workspace_id.map(str::to_string),
        path: Some(path_text),
        precedence: declaration.precedence,
        writable: declaration.writable,
        declared_state: state,
        effective_state: state,
        trust_state: None,
        diagnostics,
        revision,
        sensitive: declaration.sensitive,
        is_directory,
    }
}

struct AssetRecordContext<'a> {
    kind: AgentCliKind,
    environment: &'a AgentEnvironmentDescriptor,
    workspace_id: Option<&'a str>,
}

fn stable_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.len().to_le_bytes());
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    let encoded = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{prefix}:{encoded}")
}

pub(super) fn asset_stable_id(
    environment: &AgentEnvironmentDescriptor,
    kind: AgentCliKind,
    declaration: &AgentAssetDeclaration,
    path: &Path,
) -> String {
    stable_id(
        "asset",
        &[
            environment.id.as_str(),
            kind.key(),
            category_key(declaration.category),
            scope_key(declaration.scope),
            declaration.native_id,
            &path.to_string_lossy(),
        ],
    )
}

pub(super) fn normalize_optional_workspace(path: Option<&Path>) -> Result<Option<PathBuf>, String> {
    path.map(normalize_workspace).transpose()
}

fn normalize_workspace(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("工作区路径必须是绝对路径".to_string());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| format!("工作区不存在: {error}"))?;
    if metadata.file_type().is_symlink() || path_has_symlink_component(path) {
        return Err("出于安全原因不支持符号链接工作区".to_string());
    }
    if !metadata.is_dir() {
        return Err("工作区路径不是目录".to_string());
    }
    fs::canonicalize(path).map_err(|error| format!("无法解析工作区路径: {error}"))
}

pub(super) fn path_has_symlink_component(path: &Path) -> bool {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if fs::symlink_metadata(&current)
            .ok()
            .is_some_and(|metadata| metadata.file_type().is_symlink())
        {
            return true;
        }
    }
    false
}

fn file_revision(metadata: &fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(system_time_millis)
        .unwrap_or_default();
    format!("{}:{modified}", metadata.len())
}

pub(super) fn system_time_millis(value: SystemTime) -> Option<u128> {
    value
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

pub(super) fn now_string() -> String {
    DateTime::<Local>::from(SystemTime::now()).to_rfc3339()
}

fn scope_key(scope: AgentAssetScope) -> &'static str {
    match scope {
        AgentAssetScope::User => "user",
        AgentAssetScope::Workspace => "workspace",
        AgentAssetScope::Local => "local",
        AgentAssetScope::System => "system",
        AgentAssetScope::Managed => "managed",
    }
}

fn scope_from_key(value: &str) -> Option<AgentAssetScope> {
    match value {
        "user" => Some(AgentAssetScope::User),
        "workspace" => Some(AgentAssetScope::Workspace),
        "local" => Some(AgentAssetScope::Local),
        "system" => Some(AgentAssetScope::System),
        "managed" => Some(AgentAssetScope::Managed),
        _ => None,
    }
}

fn category_key(category: AgentAssetCategory) -> &'static str {
    match category {
        AgentAssetCategory::Config => "config",
        AgentAssetCategory::Skill => "skill",
        AgentAssetCategory::Plugin => "plugin",
        AgentAssetCategory::Extension => "extension",
        AgentAssetCategory::Mcp => "mcp",
        AgentAssetCategory::Hook => "hook",
        AgentAssetCategory::StatusUi => "status-ui",
    }
}

trait DeclarationScope {
    fn scope_key(&self) -> &'static str;
}
impl DeclarationScope for AgentAssetDeclaration {
    fn scope_key(&self) -> &'static str {
        scope_key(self.scope)
    }
}
