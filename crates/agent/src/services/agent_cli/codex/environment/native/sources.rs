use super::{
    plugin, snapshot_toml, PLUGIN_BASE_PREFIX, PLUGIN_MANIFEST_PREFIX, PLUGIN_MCP_PREFIX,
    PLUGIN_SKILLS_PREFIX, SKILL_PREFIX,
};
use crate::models::{
    AgentAssetCategory, AgentAssetInstallationOrigin, AgentAssetScope, AgentAssetSourceKind,
    AgentTrustState,
};
use crate::services::agent_cli::contracts::{
    AgentAssetSnapshot, AgentAssetSourcePathPolicy, AgentAssetSourceSpec,
    AgentFollowUpSourceDiscoveryRequest, AgentFollowUpSourceSpec, AgentFollowUpSourceTarget,
    AgentSourceDiscoveryRequest, FollowUpSourceOutput, InitialSourceOutput,
};
use crate::services::agent_cli::environment::{source, SourceInput};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(in crate::services::agent_cli::codex::environment) fn discover_sources(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    let system_root = super::super::system_paths::system_root();
    macro_rules! emit { ($($source:expr),+ $(,)?) => { $(if output.emit_initial($source).is_break() { return; })+ }; }
    emit!(
        super::super::system_paths::requirements_source(&system_root),
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "auth",
            label: "Codex 认证",
            path: root.join("auth.json"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[]
        }),
        skill_root(
            "shared-skills",
            "Codex 共享 Skills",
            request.home.join(".agents/skills"),
            request.home,
            AgentAssetScope::User,
            20,
            request.home
        ),
        skill_root(
            "codex-skills",
            "Codex Skills",
            root.join("skills"),
            root,
            AgentAssetScope::User,
            10,
            request.home
        ),
        skill_root(
            "system-skills",
            "Codex 内置 Skills",
            root.join("skills/.system"),
            root,
            AgentAssetScope::System,
            5,
            request.home
        ),
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::NativePackage,
            native_source_key: "plugins",
            label: "Codex Plugin 缓存",
            path: root.join("plugins/cache"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: false,
            source_kind: AgentAssetSourceKind::Directory,
            categories: &[AgentAssetCategory::Plugin]
        }),
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "hooks",
            label: "Codex Hooks",
            path: root.join("hooks.json"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[AgentAssetCategory::Hook]
        }),
    );
    if let Some(workspace) = request.workspace {
        emit!(
            skill_root(
                "workspace-skills",
                "Codex 工作区 Skills",
                workspace.join(".agents/skills"),
                workspace,
                AgentAssetScope::Workspace,
                20,
                request.home
            ),
            skill_root(
                "workspace-codex-skills",
                "Codex 工作区原生 Skills",
                workspace.join(".codex/skills"),
                workspace,
                AgentAssetScope::Workspace,
                20,
                request.home
            ),
            source(SourceInput {
                origin: AgentAssetInstallationOrigin::ConfigEntry,
                native_source_key: "workspace-hooks",
                label: "Codex 工作区 Hooks",
                path: workspace.join(".codex/hooks.json"),
                allowed_root: workspace,
                scope: AgentAssetScope::Workspace,
                precedence: 20,
                sensitive: true,
                source_kind: AgentAssetSourceKind::File,
                categories: &[AgentAssetCategory::Hook]
            }),
        );
    }
    #[cfg(unix)]
    emit!(skill_root(
        "admin-skills",
        "Codex 系统管理员 Skills",
        system_root.join("skills"),
        &system_root.join("skills"),
        AgentAssetScope::Managed,
        5,
        request.home
    ));
    let mut config_sources = vec![config_source(
        "config",
        "Codex 配置",
        root.join("config.toml"),
        root,
        AgentAssetScope::User,
        10,
    )];
    #[cfg(unix)]
    config_sources.push(config_source(
        "system-config",
        "Codex 系统配置",
        system_root.join("config.toml"),
        &system_root,
        AgentAssetScope::System,
        5,
    ));
    if let Some(workspace) = request.workspace {
        config_sources.push(config_source(
            "workspace-config",
            "Codex 工作区配置",
            workspace.join(".codex/config.toml"),
            workspace,
            AgentAssetScope::Workspace,
            20,
        ));
    }
    let mut ids = BTreeSet::new();
    for config in config_sources {
        let trusted = config.scope != AgentAssetScope::Workspace
            || request.context.trust_context == AgentTrustState::Trusted;
        let snapshot = match output.snapshot_initial(config) {
            std::ops::ControlFlow::Continue(Some(snapshot)) => snapshot,
            std::ops::ControlFlow::Continue(None) => continue,
            std::ops::ControlFlow::Break(_) => return,
        };
        if !trusted {
            continue;
        }
        if let Some(plugins) = snapshot_toml(&snapshot).and_then(|table| {
            table
                .get("plugins")
                .and_then(toml::Value::as_table)
                .cloned()
        }) {
            ids.extend(
                plugins
                    .keys()
                    .filter(|id| plugin::plugin_id_parts(id).is_some())
                    .cloned(),
            );
        }
    }
    for id in ids {
        if !discover_plugin(root, &id, output) {
            return;
        }
    }
}

fn skill_root(
    key: &str,
    label: &str,
    path: PathBuf,
    allowed_root: &Path,
    scope: AgentAssetScope,
    precedence: u32,
    native_home: &Path,
) -> AgentAssetSourceSpec {
    let origin = match key {
        "shared-skills" | "workspace-skills" => AgentAssetInstallationOrigin::SharedFiles,
        "system-skills" => AgentAssetInstallationOrigin::Bundled,
        _ => AgentAssetInstallationOrigin::LocalFiles,
    };
    let mut spec = source(SourceInput {
        origin,
        native_source_key: key,
        label,
        path,
        allowed_root,
        scope,
        precedence,
        sensitive: false,
        source_kind: AgentAssetSourceKind::Directory,
        categories: &[AgentAssetCategory::Skill],
    });
    if matches!(key, "codex-skills" | "workspace-codex-skills") {
        spec.path_policy = AgentAssetSourcePathPolicy::ReadonlySkillLinkRoot {
            shared_root: native_home.join(".agents/skills"),
        };
    }
    spec
}

fn config_source(
    key: &str,
    label: &str,
    path: PathBuf,
    allowed_root: &Path,
    scope: AgentAssetScope,
    precedence: u32,
) -> AgentAssetSourceSpec {
    source(SourceInput {
        origin: AgentAssetInstallationOrigin::ConfigEntry,
        native_source_key: key,
        label,
        path,
        allowed_root,
        scope,
        precedence,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: &[
            AgentAssetCategory::Mcp,
            AgentAssetCategory::Skill,
            AgentAssetCategory::Plugin,
            AgentAssetCategory::StatusUi,
            AgentAssetCategory::Hook,
        ],
    })
}

pub(super) fn plugin_source(
    root: &Path,
    key: &str,
    path: PathBuf,
    kind: AgentAssetSourceKind,
    categories: &[AgentAssetCategory],
) -> AgentAssetSourceSpec {
    let mut source = source(SourceInput {
        origin: AgentAssetInstallationOrigin::NativePackage,
        native_source_key: key,
        label: "Codex Plugin 本地清单",
        path,
        allowed_root: root,
        scope: AgentAssetScope::User,
        precedence: 10,
        sensitive: kind == AgentAssetSourceKind::File,
        source_kind: kind,
        categories,
    });
    // Package files are observed content. Native switches are written through
    // their user-config policies, never by editing an installed package.
    source.writable = false;
    source
}

fn discover_plugin(root: &Path, id: &str, output: &mut dyn InitialSourceOutput) -> bool {
    let Some((name, marketplace)) = plugin::plugin_id_parts(id) else {
        return true;
    };
    let base = root.join("plugins/cache").join(marketplace).join(name);
    let std::ops::ControlFlow::Continue(Some(snapshot)) = output.snapshot_initial(plugin_source(
        root,
        &format!("{PLUGIN_BASE_PREFIX}{id}"),
        base.clone(),
        AgentAssetSourceKind::Directory,
        &[AgentAssetCategory::Plugin],
    )) else {
        return false;
    };
    let AgentAssetSnapshot::DirectoryManifest {
        entries,
        complete: true,
        ..
    } = snapshot
    else {
        return true;
    };
    let versions = plugin::versions(&entries);
    let Some(version) = plugin::active_version(&versions) else {
        return true;
    };
    let installation = base.join(version);
    for (format, relative) in [
        ("agent", "plugin.json"),
        ("codex", ".codex-plugin/plugin.json"),
        ("claude", ".claude-plugin/plugin.json"),
        ("cursor", ".cursor-plugin/plugin.json"),
    ] {
        let key = format!("{PLUGIN_MANIFEST_PREFIX}{id}:{version}:{format}");
        let std::ops::ControlFlow::Continue(Some(snapshot)) =
            output.snapshot_initial(plugin_source(
                root,
                &key,
                installation.join(relative),
                AgentAssetSourceKind::File,
                &[
                    AgentAssetCategory::Plugin,
                    AgentAssetCategory::Mcp,
                    AgentAssetCategory::Hook,
                ],
            ))
        else {
            return false;
        };
        let bytes = match snapshot {
            AgentAssetSnapshot::Missing { .. } => continue,
            AgentAssetSnapshot::File { bytes, .. } => bytes,
            _ => return true,
        };
        if format == "agent" && !plugin::is_agent_manifest(&bytes) {
            continue;
        }
        let Some(manifest) = plugin::decode_manifest(&bytes, &installation, format == "agent")
        else {
            return true;
        };
        if !plugin::hooks::discover(
            root,
            id,
            version,
            format,
            &installation.join(relative),
            &manifest,
            output,
        ) {
            return false;
        }
        for (index, path) in manifest.skill_roots.iter().enumerate() {
            let key = format!(
                "{PLUGIN_SKILLS_PREFIX}{id}:{version}:{}:{index}:{}",
                URL_SAFE_NO_PAD.encode(&manifest.namespace),
                if format == "agent" { "agent" } else { "legacy" },
            );
            if output
                .emit_initial(plugin_source(
                    root,
                    &key,
                    path.clone(),
                    AgentAssetSourceKind::Directory,
                    &[AgentAssetCategory::Skill],
                ))
                .is_break()
            {
                return false;
            }
        }
        if let Some(path) = manifest.mcp_path {
            if output
                .emit_initial(plugin_source(
                    root,
                    &format!(
                        "{PLUGIN_MCP_PREFIX}{id}:{version}:{}",
                        if format == "agent" { "agent" } else { "legacy" }
                    ),
                    path,
                    AgentAssetSourceKind::File,
                    &[AgentAssetCategory::Mcp],
                ))
                .is_break()
            {
                return false;
            }
        }
        return true;
    }
    true
}

pub(in crate::services::agent_cli::codex) fn discover_follow_up_sources(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    let key = request.parent.native_source_key.as_str();
    if !matches!(
        key,
        "shared-skills"
            | "codex-skills"
            | "system-skills"
            | "workspace-skills"
            | "workspace-codex-skills"
            | "admin-skills"
    ) && !key.starts_with(PLUGIN_SKILLS_PREFIX)
    {
        return;
    }
    for entry in request.manifest {
        if entry.name.starts_with('.') {
            continue;
        }
        let target = if entry.is_symlink {
            if entry.name == "SKILL.md"
                || !matches!(
                    request.parent.path_policy,
                    AgentAssetSourcePathPolicy::ReadonlySkillLinkRoot { .. }
                )
            {
                continue;
            }
            AgentFollowUpSourceTarget::ReadonlySkillDirectoryLink {
                directory_entry_name: entry.name.clone(),
            }
        } else {
            match entry.source_kind {
                AgentAssetSourceKind::File if entry.name == "SKILL.md" => {
                    if key
                        .strip_prefix(PLUGIN_SKILLS_PREFIX)
                        .and_then(plugin_skill_source_parts)
                        .is_some_and(|(_, _, _, _, agent)| agent)
                    {
                        continue;
                    }
                    AgentFollowUpSourceTarget::ManifestFile {
                        entry_name: entry.name.clone(),
                    }
                }
                AgentAssetSourceKind::Directory => AgentFollowUpSourceTarget::Descendant {
                    directory_entry_name: entry.name.clone(),
                    relative_path: PathBuf::from("SKILL.md"),
                },
                _ => continue,
            }
        };
        if output
            .emit_follow_up(AgentFollowUpSourceSpec {
                parent_source_key: key.to_owned(),
                target,
                native_source_key: format!(
                    "{SKILL_PREFIX}{key}:{}",
                    URL_SAFE_NO_PAD.encode(&entry.name)
                ),
                label: "Codex Skill 清单".to_owned(),
                scope: request.parent.scope,
                precedence: request.parent.precedence,
                sensitive: true,
                source_kind: AgentAssetSourceKind::File,
                categories: vec![AgentAssetCategory::Skill],
            })
            .is_break()
        {
            return;
        }
    }
}

pub(in crate::services::agent_cli::codex::environment) fn plugin_skill_source_parts(
    value: &str,
) -> Option<(&str, &str, String, usize, bool)> {
    let mut parts = value.split(':');
    let id = parts.next()?;
    let version = parts.next()?;
    let namespace = String::from_utf8(URL_SAFE_NO_PAD.decode(parts.next()?).ok()?).ok()?;
    let index = parts.next()?.parse().ok()?;
    let format = parts.next()?;
    (parts.next().is_none()
        && plugin::plugin_id_parts(id).is_some()
        && matches!(format, "agent" | "legacy"))
    .then_some((id, version, namespace, index, format == "agent"))
}

pub(in crate::services::agent_cli::codex::environment) fn skill_source_parts(
    value: &str,
) -> Option<(&str, String)> {
    let (root, entry) = value.strip_prefix(SKILL_PREFIX)?.rsplit_once(':')?;
    let entry = String::from_utf8(URL_SAFE_NO_PAD.decode(entry).ok()?).ok()?;
    if entry.is_empty()
        || entry.starts_with('.')
        || Path::new(&entry).components().count() != 1
        || entry.contains(['/', '\\'])
    {
        return None;
    }
    Some((root, entry))
}

pub(in crate::services::agent_cli::codex::environment) fn plugin_manifest_parts(
    value: &str,
) -> Option<(&str, &str, &str)> {
    let mut parts = value.split(':');
    let id = parts.next()?;
    let version = parts.next()?;
    let format = parts.next()?;
    (parts.next().is_none()
        && plugin::plugin_id_parts(id).is_some()
        && matches!(format, "agent" | "codex" | "claude" | "cursor"))
    .then_some((id, version, format))
}

pub(in crate::services::agent_cli::codex::environment) fn plugin_mcp_source_parts(
    value: &str,
) -> Option<(&str, &str, bool)> {
    let mut parts = value.split(':');
    let id = parts.next()?;
    let version = parts.next()?;
    let format = parts.next()?;
    (parts.next().is_none()
        && plugin::plugin_id_parts(id).is_some()
        && matches!(format, "agent" | "legacy"))
    .then_some((id, version, format == "agent"))
}

pub(super) fn plugin_manifest_root<'a>(path: &'a Path, format: &str) -> Option<&'a Path> {
    if format == "agent" {
        path.parent()
    } else {
        path.parent()?.parent()
    }
}
