use super::super::config;

use super::*;
use crate::{
    models::*,
    services::agent_cli::{contracts::*, environment::*},
};
use std::path::{Path, PathBuf};

pub(super) fn managed_root() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/GeminiCli")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(r"C:\ProgramData\gemini-cli")
    } else {
        PathBuf::from("/etc/gemini-cli")
    }
}

fn configured_system_file(name: &str, default: PathBuf, workdir: &Path) -> PathBuf {
    crate::services::cli_paths::configured_path(name)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                workdir.join(path)
            }
        })
        .unwrap_or(default)
}

pub(super) fn mcp_participation(
    request: AgentAssetParseRequest<'_>,
) -> crate::models::AgentAssetResolutionParticipation {
    if request.context.workspace_id.is_some()
        && request.context.trust_context != AgentTrustState::Trusted
    {
        crate::models::AgentAssetResolutionParticipation::Suppressed {
            reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
        }
    } else {
        crate::models::AgentAssetResolutionParticipation::Participates
    }
}

pub(super) fn participation(
    request: AgentAssetParseRequest<'_>,
    category: AgentAssetCategory,
) -> crate::models::AgentAssetResolutionParticipation {
    if category == AgentAssetCategory::Mcp {
        return mcp_participation(request);
    }
    if workspace_source(request.source)
        && request.context.workspace_id.is_some()
        && request.context.trust_context != AgentTrustState::Trusted
    {
        crate::models::AgentAssetResolutionParticipation::Suppressed {
            reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
        }
    } else {
        crate::models::AgentAssetResolutionParticipation::Participates
    }
}

pub(super) fn workspace_source(source: &AgentAssetSourceSpec) -> bool {
    matches!(
        source.native_source_key.as_str(),
        "workspace-settings" | "workspace-skills" | "workspace-shared-skills"
    ) || source
        .native_source_key
        .strip_prefix(SKILL_MANIFEST_PREFIX)
        .is_some_and(|source| {
            source.starts_with("workspace-skills:")
                || source.starts_with("workspace-shared-skills:")
        })
}

pub(super) fn discover_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    _output: &mut dyn AgentDiagnosticOutput,
) -> Vec<crate::models::AgentConfigurationContext> {
    let root = config::config_dir_for_home(request.home);
    default_context(request, root, PARSER_VERSION)
}

pub(super) fn discover_sources(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    let managed = managed_root();
    let workdir = request.workspace.unwrap_or(request.home);
    let defaults_path = configured_system_file(
        "GEMINI_CLI_SYSTEM_DEFAULTS_PATH",
        managed.join("system-defaults.json"),
        workdir,
    );
    let settings_path = configured_system_file(
        "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
        managed.join("settings.json"),
        workdir,
    );

    let skill_directory_source = |input: SourceInput<'_>| {
        let mut spec = source(input);
        spec.path_policy = AgentAssetSourcePathPolicy::ReadonlySkillLinkRoot {
            shared_root: request.home.join(".agents/skills"),
        };
        spec
    };
    macro_rules! emit {
        ($($value:expr),+ $(,)?) => {
            $(if output.emit_initial($value).is_break() { return; })+
        };
    }
    emit!(
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "system-defaults",
            label: "Gemini CLI 系统默认设置",
            path: defaults_path.clone(),
            allowed_root: defaults_path.parent().unwrap_or(&managed),
            scope: AgentAssetScope::System,
            precedence: 5,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[
                AgentAssetCategory::Mcp,
                AgentAssetCategory::Skill,
                AgentAssetCategory::Hook,
                AgentAssetCategory::StatusUi
            ],
        }),
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "settings",
            label: "Gemini CLI 设置",
            path: root.join("settings.json"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[
                AgentAssetCategory::Mcp,
                AgentAssetCategory::Skill,
                AgentAssetCategory::Hook,
                AgentAssetCategory::StatusUi,
            ],
        }),
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "system-settings",
            label: "Gemini CLI 托管设置",
            path: settings_path.clone(),
            allowed_root: settings_path.parent().unwrap_or(&managed),
            scope: AgentAssetScope::Managed,
            precedence: 30,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[
                AgentAssetCategory::Mcp,
                AgentAssetCategory::Skill,
                AgentAssetCategory::Hook,
                AgentAssetCategory::StatusUi
            ],
        }),
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "mcp-enablement",
            label: "Gemini CLI MCP 启用状态",
            path: root.join("mcp-server-enablement.json"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 100,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: &[AgentAssetCategory::Mcp],
        }),
        skill_directory_source(SourceInput {
            origin: AgentAssetInstallationOrigin::LocalFiles,
            native_source_key: "skills",
            label: "Gemini CLI Skills",
            path: root.join("skills"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: false,
            source_kind: AgentAssetSourceKind::Directory,
            categories: &[AgentAssetCategory::Skill],
        }),
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::SharedFiles,
            native_source_key: "shared-skills",
            label: "Gemini CLI 共享 Skills",
            path: request.home.join(".agents/skills"),
            allowed_root: request.home,
            scope: AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            source_kind: AgentAssetSourceKind::Directory,
            categories: &[AgentAssetCategory::Skill],
        }),
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::NativePackage,
            native_source_key: EXTENSIONS_SOURCE_KEY,
            label: "Gemini CLI Extensions",
            path: root.join("extensions"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 0,
            sensitive: false,
            source_kind: AgentAssetSourceKind::Directory,
            categories: &[AgentAssetCategory::Extension],
        }),
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "extension-enablement",
            label: "Gemini CLI Extension 启用状态",
            path: root.join("extensions/extension-enablement.json"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 100,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: &[AgentAssetCategory::Extension],
        }),
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "trusted-folders",
            label: "Gemini CLI 工作区信任设置",
            path: root.join("trustedFolders.json"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 0,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: &[],
        }),
    );

    if let Some(workspace) = request.workspace {
        emit!(
            source(SourceInput {
                origin: AgentAssetInstallationOrigin::ConfigEntry,
                native_source_key: "workspace-settings",
                label: "Gemini CLI 工作区设置",
                path: workspace.join(".gemini/settings.json"),
                allowed_root: workspace,
                scope: AgentAssetScope::Workspace,
                precedence: 20,
                sensitive: true,
                source_kind: AgentAssetSourceKind::File,
                categories: &[
                    AgentAssetCategory::Mcp,
                    AgentAssetCategory::Skill,
                    AgentAssetCategory::Hook,
                    AgentAssetCategory::StatusUi,
                ],
            }),
            source(SourceInput {
                origin: AgentAssetInstallationOrigin::SharedFiles,
                native_source_key: "workspace-shared-skills",
                label: "Gemini CLI 工作区共享 Skills",
                path: workspace.join(".agents/skills"),
                allowed_root: workspace,
                scope: AgentAssetScope::Workspace,
                precedence: 40,
                sensitive: false,
                source_kind: AgentAssetSourceKind::Directory,
                categories: &[AgentAssetCategory::Skill],
            }),
            skill_directory_source(SourceInput {
                origin: AgentAssetInstallationOrigin::LocalFiles,
                native_source_key: "workspace-skills",
                label: "Gemini CLI 工作区 Skills",
                path: workspace.join(".gemini/skills"),
                allowed_root: workspace,
                scope: AgentAssetScope::Workspace,
                precedence: 30,
                sensitive: false,
                source_kind: AgentAssetSourceKind::Directory,
                categories: &[AgentAssetCategory::Skill],
            }),
        );
    }
}

pub(super) fn discover_follow_up_sources(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    let parent_key = request.parent.native_source_key.as_str();
    if parent_key == EXTENSIONS_SOURCE_KEY {
        for entry in request.manifest {
            if entry.name.trim().is_empty()
                || entry.is_symlink
                || entry.source_kind != AgentAssetSourceKind::Directory
            {
                continue;
            }
            let extension = entry.name.clone();
            for (target, native_source_key, label, source_kind, categories) in [
                (
                    AgentFollowUpSourceTarget::Descendant {
                        directory_entry_name: extension.clone(),
                        relative_path: PathBuf::from("gemini-extension.json"),
                    },
                    format!("{EXTENSION_MANIFEST_PREFIX}{extension}"),
                    format!("Gemini CLI Extension 清单：{extension}"),
                    AgentAssetSourceKind::File,
                    vec![AgentAssetCategory::Extension, AgentAssetCategory::Mcp],
                ),
                (
                    AgentFollowUpSourceTarget::Descendant {
                        directory_entry_name: extension.clone(),
                        relative_path: PathBuf::from("hooks/hooks.json"),
                    },
                    format!("{EXTENSION_HOOKS_PREFIX}{extension}"),
                    format!("Gemini CLI Extension Hooks：{extension}"),
                    AgentAssetSourceKind::File,
                    vec![AgentAssetCategory::Hook],
                ),
                (
                    AgentFollowUpSourceTarget::Descendant {
                        directory_entry_name: extension.clone(),
                        relative_path: PathBuf::from("skills"),
                    },
                    format!("{EXTENSION_SKILLS_PREFIX}{extension}"),
                    format!("Gemini CLI Extension Skills：{extension}"),
                    AgentAssetSourceKind::Directory,
                    vec![AgentAssetCategory::Skill],
                ),
            ] {
                if output
                    .emit_follow_up(AgentFollowUpSourceSpec {
                        parent_source_key: EXTENSIONS_SOURCE_KEY.to_owned(),
                        target,
                        native_source_key,
                        label,
                        scope: request.parent.scope,
                        precedence: if categories == [AgentAssetCategory::Skill] {
                            1
                        } else {
                            request.parent.precedence
                        },
                        sensitive: !categories.contains(&AgentAssetCategory::Skill),
                        source_kind,
                        categories,
                    })
                    .is_break()
                {
                    return;
                }
            }
        }
        return;
    }

    let is_skill_root = matches!(
        parent_key,
        "skills" | "shared-skills" | "workspace-skills" | "workspace-shared-skills"
    ) || parent_key.starts_with(super::builtin::SOURCE_PREFIX);
    let extension = parent_key.strip_prefix(EXTENSION_SKILLS_PREFIX);
    if !is_skill_root && extension.is_none() {
        return;
    }

    // Gemini loads both a root SKILL.md and one level of named skill folders.
    // The direct file target is deliberately separate from descendant targets.
    for entry in request.manifest {
        if entry.name.trim().is_empty() {
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
        let native_source_key = match extension {
            Some(extension) => format!("{EXTENSION_SKILL_PREFIX}{extension}:{}", entry.name),
            None => format!("{SKILL_MANIFEST_PREFIX}{parent_key}:{}", entry.name),
        };
        if output
            .emit_follow_up(AgentFollowUpSourceSpec {
                parent_source_key: parent_key.to_owned(),
                target,
                native_source_key,
                label: format!("Gemini CLI Skill：{}", entry.name),
                scope: request.parent.scope,
                precedence: request.parent.precedence,
                sensitive: false,
                source_kind: AgentAssetSourceKind::File,
                categories: vec![AgentAssetCategory::Skill],
            })
            .is_break()
        {
            return;
        }
    }
}
