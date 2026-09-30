use super::super::config;
use super::parse::{emit_malformed, select_exact_project, ProjectSelection};
use super::{PARSER_VERSION, SKILL_PREFIX};
use crate::models::{AgentAssetCategory, AgentAssetScope, AgentAssetSourceKind, AgentTrustState};
use crate::services::agent_cli::contracts::{
    AgentAssetLogicalOrigin, AgentAssetSnapshot, AgentAssetSourcePathPolicy, AgentAssetSourceSpec,
    AgentContextDiscoveryRequest, AgentDiagnosticOutput, AgentFollowUpSourceDiscoveryRequest,
    AgentFollowUpSourceSpec, AgentFollowUpSourceTarget, AgentSourceDiscoveryRequest,
    AgentWorkspaceTrustResolveRequest, AgentWorkspaceTrustSourceRequest, FollowUpSourceOutput,
    InitialSourceOutput,
};
use crate::services::agent_cli::environment::{
    default_context, source, source_with_logical_origins, SourceInput,
};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

pub(in crate::services::agent_cli::claude) fn discover_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    _output: &mut dyn AgentDiagnosticOutput,
) -> Vec<crate::models::AgentConfigurationContext> {
    default_context(
        request,
        config::config_dir_for_home(request.home),
        PARSER_VERSION,
    )
}

fn global_state_path(home: &Path, config_root: &Path) -> PathBuf {
    if config_root == home.join(".claude") {
        home.join(".claude.json")
    } else {
        config_root.join(".claude.json")
    }
}

pub(in crate::services::agent_cli::claude) fn managed_root() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/ClaudeCode")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(r"C:\Program Files\ClaudeCode")
    } else {
        PathBuf::from("/etc/claude-code")
    }
}

fn account_input<'a>(home: &'a Path, root: &'a Path) -> SourceInput<'a> {
    let path = global_state_path(home, root);
    let allowed_root = if root == home.join(".claude") {
        home
    } else {
        root
    };
    SourceInput {
        origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
        native_source_key: "account",
        label: "Claude Code 账户与 MCP 状态",
        allowed_root,
        path,
        scope: AgentAssetScope::User,
        precedence: 10,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: &[AgentAssetCategory::Mcp],
    }
}

fn account_source(
    home: &Path,
    root: &Path,
) -> crate::services::agent_cli::contracts::AgentAssetSourceSpec {
    let input = account_input(home, root);
    source_with_logical_origins(
        input,
        &[
            AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            },
            AgentAssetLogicalOrigin {
                scope: AgentAssetScope::Local,
                precedence: 30,
            },
        ],
    )
}

fn skill_directory_source(input: SourceInput<'_>, native_home: &Path) -> AgentAssetSourceSpec {
    let mut source = source(input);
    source.path_policy = AgentAssetSourcePathPolicy::ReadonlySkillLinkRoot {
        shared_root: native_home.join(".agents/skills"),
    };
    source
}

pub(in crate::services::agent_cli::claude) fn discover_workspace_trust_sources(
    request: AgentWorkspaceTrustSourceRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    if request.workspace.is_absolute() {
        let _ = output.emit_initial(account_source(request.home, request.config_root));
    }
}

pub(in crate::services::agent_cli::claude) fn resolve_workspace_trust(
    request: AgentWorkspaceTrustResolveRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> AgentTrustState {
    let Some(account) = request
        .sources
        .iter()
        .find(|item| item.spec.native_source_key == "account")
    else {
        return AgentTrustState::Unknown;
    };
    let AgentAssetSnapshot::File { bytes, .. } = account.snapshot else {
        if let AgentAssetSnapshot::Blocked { diagnostic, .. } = account.snapshot {
            output.emit_diagnostic(diagnostic.clone());
        } else if matches!(
            account.snapshot,
            AgentAssetSnapshot::DirectoryManifest { .. }
        ) {
            emit_malformed(output, "account");
        }
        return AgentTrustState::Unknown;
    };
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        emit_malformed(output, "projects");
        return AgentTrustState::Unknown;
    };
    let Some(root) = value.as_object() else {
        emit_malformed(output, "projects");
        return AgentTrustState::Unknown;
    };
    let Some(projects) = root.get("projects").and_then(Value::as_object) else {
        if root.contains_key("projects") {
            emit_malformed(output, "projects");
        }
        return AgentTrustState::Unknown;
    };
    let entry = match select_exact_project(projects, request.workspace, request.workspace_lexical) {
        ProjectSelection::Unique(entry) => entry,
        ProjectSelection::Malformed => {
            emit_malformed(output, "projects");
            return AgentTrustState::Unknown;
        }
        ProjectSelection::Missing | ProjectSelection::Ambiguous => {
            return AgentTrustState::Unknown;
        }
    };
    match entry.get("hasTrustDialogAccepted") {
        Some(Value::Bool(true)) => AgentTrustState::Trusted,
        Some(Value::Bool(false)) => AgentTrustState::Untrusted,
        Some(_) => {
            emit_malformed(output, "projects");
            AgentTrustState::Unknown
        }
        None => AgentTrustState::Unknown,
    }
}

fn registry_source(root: &Path) -> AgentAssetSourceSpec {
    source_with_logical_origins(
        SourceInput {
            origin: crate::models::AgentAssetInstallationOrigin::NativePackage,
            native_source_key: "plugin-registry",
            label: "Claude Code 已安装 Plugin",
            path: root.join("plugins/installed_plugins.json"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 5,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[AgentAssetCategory::Plugin],
        },
        &[
            AgentAssetLogicalOrigin {
                scope: AgentAssetScope::Managed,
                precedence: 40,
            },
            AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            },
            AgentAssetLogicalOrigin {
                scope: AgentAssetScope::Workspace,
                precedence: 20,
            },
            AgentAssetLogicalOrigin {
                scope: AgentAssetScope::Local,
                precedence: 30,
            },
        ],
    )
}

pub(in crate::services::agent_cli::claude) fn discover_resource_sources(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    let managed = managed_root();
    macro_rules! emit { ($($value:expr),+ $(,)?) => { $(if output.emit_initial($value).is_break() { return; })+ }; }
    emit!(
        source(SourceInput {
            origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "settings",
            label: "Claude Code 设置",
            path: root.join("settings.json"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[
                AgentAssetCategory::Mcp,
                AgentAssetCategory::Plugin,
                AgentAssetCategory::Hook,
                AgentAssetCategory::StatusUi
            ]
        }),
        account_source(request.home, root),
        skill_directory_source(
            SourceInput {
                origin: crate::models::AgentAssetInstallationOrigin::LocalFiles,
                native_source_key: "skills",
                label: "Claude Code Skills",
                path: root.join("skills"),
                allowed_root: root,
                scope: AgentAssetScope::User,
                precedence: 10,
                sensitive: false,
                source_kind: AgentAssetSourceKind::Directory,
                categories: &[AgentAssetCategory::Skill]
            },
            request.home
        ),
        source(SourceInput {
            origin: crate::models::AgentAssetInstallationOrigin::NativePackage,
            native_source_key: "plugin-cache",
            label: "Claude Code Plugin 缓存",
            path: root.join("plugins/cache"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 5,
            sensitive: false,
            source_kind: AgentAssetSourceKind::Directory,
            categories: &[]
        }),
        source(SourceInput {
            origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "known-marketplaces",
            label: "Claude Code Marketplace 记录",
            path: root.join("plugins/known_marketplaces.json"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 5,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[]
        }),
        source(SourceInput {
            origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "managed-settings",
            label: "Claude Code 托管设置",
            path: managed.join("managed-settings.json"),
            allowed_root: &managed,
            scope: AgentAssetScope::Managed,
            precedence: 40,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[
                AgentAssetCategory::Mcp,
                AgentAssetCategory::Plugin,
                AgentAssetCategory::Hook,
                AgentAssetCategory::StatusUi
            ]
        }),
        source(SourceInput {
            origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "managed-mcp",
            label: "Claude Code 托管 MCP",
            path: managed.join("managed-mcp.json"),
            allowed_root: &managed,
            scope: AgentAssetScope::Managed,
            precedence: 40,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[AgentAssetCategory::Mcp]
        })
    );
    if let Some(workspace) = request.workspace {
        let settings = workspace.join(".claude");
        emit!(
            source(SourceInput {
                origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
                native_source_key: "workspace-settings",
                label: "Claude Code 工作区设置",
                path: settings.join("settings.json"),
                allowed_root: workspace,
                scope: AgentAssetScope::Workspace,
                precedence: 20,
                sensitive: true,
                source_kind: AgentAssetSourceKind::File,
                categories: &[
                    AgentAssetCategory::Mcp,
                    AgentAssetCategory::Plugin,
                    AgentAssetCategory::Hook,
                    AgentAssetCategory::StatusUi
                ]
            }),
            source(SourceInput {
                origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
                native_source_key: "workspace-local-settings",
                label: "Claude Code 工作区本地设置",
                path: settings.join("settings.local.json"),
                allowed_root: workspace,
                scope: AgentAssetScope::Local,
                precedence: 30,
                sensitive: true,
                source_kind: AgentAssetSourceKind::File,
                categories: &[
                    AgentAssetCategory::Mcp,
                    AgentAssetCategory::Plugin,
                    AgentAssetCategory::Hook,
                    AgentAssetCategory::StatusUi
                ]
            }),
            source(SourceInput {
                origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
                native_source_key: "workspace-mcp",
                label: "Claude Code 工作区 MCP",
                path: workspace.join(".mcp.json"),
                allowed_root: workspace,
                scope: AgentAssetScope::Workspace,
                precedence: 20,
                sensitive: true,
                source_kind: AgentAssetSourceKind::File,
                categories: &[AgentAssetCategory::Mcp]
            }),
            skill_directory_source(
                SourceInput {
                    origin: crate::models::AgentAssetInstallationOrigin::LocalFiles,
                    native_source_key: "workspace-skills",
                    label: "Claude Code 工作区 Skills",
                    path: settings.join("skills"),
                    allowed_root: workspace,
                    scope: AgentAssetScope::Workspace,
                    precedence: 20,
                    sensitive: false,
                    source_kind: AgentAssetSourceKind::Directory,
                    categories: &[AgentAssetCategory::Skill]
                },
                request.home
            )
        );
    }
    super::plugin::discover(request, registry_source(root), output);
}

pub(in crate::services::agent_cli::claude) fn discover_follow_up_sources(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    if !matches!(
        request.parent.native_source_key.as_str(),
        "skills" | "workspace-skills"
    ) {
        return;
    }
    for entry in request.manifest {
        if !valid_skill_entry_name(&entry.name) {
            continue;
        }
        let target = if entry.is_symlink {
            AgentFollowUpSourceTarget::ReadonlySkillDirectoryLink {
                directory_entry_name: entry.name.clone(),
            }
        } else if entry.source_kind == AgentAssetSourceKind::Directory {
            AgentFollowUpSourceTarget::Descendant {
                directory_entry_name: entry.name.clone(),
                relative_path: PathBuf::from("SKILL.md"),
            }
        } else {
            continue;
        };
        if output
            .emit_follow_up(AgentFollowUpSourceSpec {
                parent_source_key: request.parent.native_source_key.clone(),
                target,
                native_source_key: format!(
                    "{SKILL_PREFIX}{}:{}",
                    request.parent.native_source_key, entry.name
                ),
                label: format!("Claude Code Skill：{}", entry.name),
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

fn valid_skill_entry_name(name: &str) -> bool {
    if name.trim().is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        // A drive letter followed by `:` is a Windows path prefix even when
        // the following component is relative (`C:skill`). Keep this check
        // portable so Unix inventory cannot admit a Windows-only path form.
        || name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && name.as_bytes().get(1) == Some(&b':')
        || name.chars().any(char::is_control)
    {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_state_uses_profile_root() {
        assert_eq!(
            global_state_path(Path::new("/tmp/home"), Path::new("/tmp/home/.claude")),
            PathBuf::from("/tmp/home/.claude.json")
        );
        assert_eq!(
            global_state_path(Path::new("/tmp/home"), Path::new("/tmp/profile")),
            PathBuf::from("/tmp/profile/.claude.json")
        );
    }

    #[test]
    fn skill_entry_name_requires_one_portable_normal_component() {
        for name in [
            "",
            " ",
            ".",
            "..",
            "nested/name",
            r"nested\name",
            "/absolute",
            r"C:\absolute",
            "C:skill",
            "plugin/skill",
        ] {
            assert!(!valid_skill_entry_name(name), "must reject {name:?}");
        }
        for name in ["manual", "skill-name", "with spaces"] {
            assert!(valid_skill_entry_name(name), "must accept {name:?}");
        }
    }
}
