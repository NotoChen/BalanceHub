use super::*;
use crate::{
    models::*,
    services::agent_cli::{contracts::*, environment::*},
};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TrustPathPlatform {
    Macos,
    Windows,
    Linux,
}

pub(super) fn discover_workspace_trust_sources(
    request: AgentWorkspaceTrustSourceRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    if !request.workspace.is_absolute() {
        return;
    }
    let _ = output.emit_initial(source(SourceInput {
        origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
        native_source_key: "trusted-folders",
        label: "Gemini CLI 工作区信任设置",
        path: request.config_root.join("trustedFolders.json"),
        allowed_root: request.config_root,
        scope: AgentAssetScope::User,
        precedence: 0,
        sensitive: false,
        source_kind: AgentAssetSourceKind::File,
        categories: &[],
    }));
}

pub(super) fn resolve_workspace_trust(
    request: AgentWorkspaceTrustResolveRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> AgentTrustState {
    resolve_workspace_trust_with_platform(request, output, trust_path_platform())
}

/// Platform-specific path semantics are injected here so CI can exercise all
/// native path forms even when it runs on a different host OS.
pub(super) fn resolve_workspace_trust_with_platform(
    request: AgentWorkspaceTrustResolveRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
    platform: TrustPathPlatform,
) -> AgentTrustState {
    let Some(source) = request
        .sources
        .iter()
        .find(|item| item.spec.native_source_key == "trusted-folders")
    else {
        return AgentTrustState::Unknown;
    };
    let AgentAssetSnapshot::File { bytes, .. } = source.snapshot else {
        if let AgentAssetSnapshot::Blocked { diagnostic, .. } = source.snapshot {
            output.emit_diagnostic(diagnostic.clone());
        }
        return AgentTrustState::Unknown;
    };
    let Some(root) = parse_gemini_jsonc(bytes, output, AgentAssetCategory::Mcp) else {
        return AgentTrustState::Unknown;
    };
    let Some(root) = root.as_object() else {
        emit_malformed(output, "trustedFolders");
        return AgentTrustState::Unknown;
    };
    let workspace_forms = [
        normalize_path(request.workspace, platform),
        normalize_path(
            request.workspace_lexical.unwrap_or(request.workspace),
            platform,
        ),
    ];
    let mut winner: Option<(usize, String, AgentTrustState)> = None;
    for (raw_path, raw_level) in root {
        let Some(level) = raw_level.as_str() else {
            emit_malformed(output, "trustedFolders");
            return AgentTrustState::Unknown;
        };
        let Some(rule_path) = valid_absolute_path(raw_path, platform) else {
            emit_malformed(output, "trustedFolders");
            return AgentTrustState::Unknown;
        };
        let Some(mut effective_rule) = normalize_path(&rule_path, platform) else {
            emit_malformed(output, "trustedFolders");
            return AgentTrustState::Unknown;
        };
        let parent_rule = level == "TRUST_PARENT";
        if parent_rule {
            effective_rule = dirname(&effective_rule);
        }
        let level = match level {
            "TRUST_FOLDER" => AgentTrustState::Trusted,
            "TRUST_PARENT" => AgentTrustState::Trusted,
            "DO_NOT_TRUST" => AgentTrustState::Untrusted,
            _ => {
                emit_malformed(output, "trustedFolders");
                return AgentTrustState::Unknown;
            }
        };
        let matches = workspace_forms.iter().any(|workspace| {
            workspace
                .as_deref()
                .is_some_and(|workspace| path_matches_rule(&effective_rule, workspace))
        });
        if !matches {
            continue;
        }
        let rank = raw_path.len();
        match &winner {
            Some((old_rank, old_rule, old_level)) if *old_rank == rank => {
                if old_rule != &effective_rule || *old_level != level {
                    return AgentTrustState::Unknown;
                }
            }
            Some((old_rank, _, _)) if *old_rank > rank => {}
            _ => winner = Some((rank, effective_rule, level)),
        }
    }
    winner
        .map(|(_, _, value)| value)
        .unwrap_or(AgentTrustState::Unknown)
}

fn valid_absolute_path(value: &str, platform: TrustPathPlatform) -> Option<PathBuf> {
    if value.is_empty() || value.chars().any(char::is_control) || value.contains('\0') {
        return None;
    }
    let normalized = value.replace('\\', "/");
    let absolute = normalized.starts_with('/')
        || (matches!(platform, TrustPathPlatform::Windows)
            && normalized.len() >= 3
            && normalized.as_bytes()[1] == b':'
            && normalized.as_bytes()[2] == b'/'
            && normalized.as_bytes()[0].is_ascii_alphabetic());
    if !absolute || normalized.split('/').any(|component| component == "..") {
        return None;
    }
    Some(PathBuf::from(normalized))
}

fn trust_path_platform() -> TrustPathPlatform {
    if cfg!(target_os = "macos") {
        TrustPathPlatform::Macos
    } else if cfg!(target_os = "windows") {
        TrustPathPlatform::Windows
    } else {
        TrustPathPlatform::Linux
    }
}

fn normalize_path(path: &Path, platform: TrustPathPlatform) -> Option<String> {
    let raw = path.to_string_lossy().replace('\\', "/");
    normalize_path_string(&raw, platform)
}

fn normalize_path_string(raw: &str, platform: TrustPathPlatform) -> Option<String> {
    let value = raw.replace('\\', "/");
    let windows_absolute = matches!(platform, TrustPathPlatform::Windows)
        && value.as_bytes().get(1) == Some(&b':')
        && value.as_bytes().get(2) == Some(&b'/')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic);
    if !value.starts_with('/') && !windows_absolute {
        return None;
    }
    let (prefix, components) = if windows_absolute {
        (&value[..3], &value[3..])
    } else {
        ("/", value.strip_prefix('/').unwrap_or_default())
    };
    let mut normalized = prefix.to_owned();
    for component in components.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return None;
        }
        if !normalized.ends_with('/') {
            normalized.push('/');
        }
        normalized.push_str(component);
    }
    let mut value = normalized;
    let is_windows_root = windows_absolute && value.len() == 3 && value.ends_with('/');
    if value.len() > 1 && value.ends_with('/') && !is_windows_root {
        value.pop();
    }
    if matches!(
        platform,
        TrustPathPlatform::Macos | TrustPathPlatform::Windows
    ) {
        value.make_ascii_lowercase();
    }
    Some(value)
}

fn dirname(path: &str) -> String {
    if path.ends_with(":/") {
        return path.to_owned();
    }
    path.rsplit_once('/')
        .map(|(parent, _)| {
            if parent.is_empty() {
                "/".to_owned()
            } else if parent.ends_with(':') {
                format!("{parent}/")
            } else {
                parent.to_owned()
            }
        })
        .unwrap_or_else(|| "/".to_owned())
}

fn path_matches_rule(rule: &str, workspace: &str) -> bool {
    let equal = rule == workspace;
    equal
        || rule == "/"
        || (rule.ends_with(":/") && workspace.starts_with(rule))
        || (workspace.starts_with(rule) && workspace.as_bytes().get(rule.len()) == Some(&b'/'))
}
