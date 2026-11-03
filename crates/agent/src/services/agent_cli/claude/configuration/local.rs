//! Claude's version-sensitive repository-local settings candidate. Discovery
//! does not run Git, trust a pointer outside the native user/workspace boundary,
//! or claim that a particular CLI/SDK invocation loaded the candidate.
use super::{
    support, AgentConfigurationSourceOutput, AgentConfigurationSourceSpec,
    AgentSourceDiscoveryRequest,
};
use crate::{
    models::*,
    services::agent_cli::environment::{source, verified_path::inspect_verified_path, SourceInput},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn candidate(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn AgentConfigurationSourceOutput,
) -> Option<AgentConfigurationSourceSpec> {
    let workspace = request.workspace?;
    output.diagnostic(support::diagnostic("configurationClaudeLocalContext",
        "Claude 项目个人配置受版本、Git/worktree、目录所有权、操作系统与 SDK 启动方式影响；当前目录与仓库来源均是候选，未证明实际加载或合并结果",
        AgentConfigurationDiagnosticSeverity::Info));
    let root = repository_local_root(workspace, request.home)?;
    if root == workspace {
        return None;
    }
    let mut spec = support::describe(source(SourceInput {
        origin: AgentAssetInstallationOrigin::ConfigEntry, native_source_key: "workspace-root-local-settings", label: "Claude 仓库个人配置（条件来源）",
        path: root.join(".claude/settings.local.json"), allowed_root: &root, scope: AgentAssetScope::Local, precedence: 31,
        sensitive: true, source_kind: AgentAssetSourceKind::File, categories: &[],
    }), AgentConfigurationFormat::Json, false,
        "v2.1.211+ 在满足 Git、所有权和平台条件时使用仓库根；标准 worktree 使用主 checkout。原目录个人设置仍可能合并，未证明当前 CLI/SDK 已加载",
        "保存后，请在原生新会话中确认条件来源的实际加载与合并结果");
    spec.version_requirement = Some("需要核对 Claude Code v2.1.211+；Windows、主目录作为仓库根、不同所有者或 SDK 入口使用当前目录规则".into());
    Some(spec)
}

fn repository_local_root(workspace: &Path, home: &Path) -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    for root in workspace.ancestors().take(32) {
        let marker = root.join(".git");
        let Ok(metadata) = fs::symlink_metadata(&marker) else {
            continue;
        };
        if metadata.file_type().is_symlink() || !owned(root) || !owned(&marker) {
            return None;
        }
        let root = if metadata.is_dir() {
            root.to_owned()
        } else if metadata.is_file() {
            // Git's standard worktree layout supplies both pointers. Require
            // these exact verified facts, not a guessed path from a filename.
            let text = read_pointer(&marker, home, workspace)?;
            let target = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
            if !target.is_absolute()
                || target
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
            {
                return None;
            }
            let worktrees = target.parent()?;
            if worktrees.file_name()? != "worktrees" {
                return None;
            }
            let common = worktrees.parent()?;
            if common.file_name()? != ".git"
                || read_pointer(&target.join("commondir"), home, workspace)?.trim() != "../.."
            {
                return None;
            }
            if !owned(common) {
                return None;
            }
            common.parent()?.to_owned()
        } else {
            return None;
        };
        if root == home || !owned(&root) {
            return None;
        }
        let settings = root.join(".claude");
        if fs::symlink_metadata(&settings).is_ok() && !owned(&settings) {
            return None;
        }
        return Some(root);
    }
    None
}

fn read_pointer(path: &Path, home: &Path, workspace: &Path) -> Option<String> {
    let allowed_root = if path.starts_with(home) {
        home
    } else if path.starts_with(workspace) {
        workspace
    } else {
        return None;
    };
    let guard = inspect_verified_path(
        &[allowed_root],
        allowed_root,
        path,
        AgentAssetSourceKind::File,
    )
    .ok()?;
    let bytes = guard.read_file_bounded(4096).ok()?;
    guard.revalidate().ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(unix)]
fn owned(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        !metadata.file_type().is_symlink() && metadata.uid() == unsafe { libc::geteuid() }
    })
}

#[cfg(not(unix))]
fn owned(_path: &Path) -> bool {
    false
}
