//! Durable native trust only. Runtime feature overrides and the Grok process
//! cache are not observable here, so every unproved decision remains Unknown.
use crate::{
    models::{
        AgentAssetDiagnostic, AgentAssetDocumentFormat, AgentAssetInstallationOrigin,
        AgentAssetScope, AgentAssetSourceKind, AgentTrustState,
    },
    services::agent_cli::{
        contracts::{
            AgentAssetResolveSource, AgentAssetSnapshot, AgentDiagnosticOutput,
            AgentWorkspaceTrustResolveRequest, AgentWorkspaceTrustSourceRequest,
            InitialSourceOutput,
        },
        environment::{source, SourceInput},
    },
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

#[cfg(test)]
mod tests;
mod topology;

const PREFIX: &str = "workspace-trust-";
const STORE_KEY: &str = "workspace-trust-store";
const REGISTRY_KEY: &str = "workspace-trust-registry-root";
const HOME_ROOT_KEY: &str = "workspace-trust-home-root";
const ROOT_PREFIX: &str = "workspace-trust-root:";

pub(super) fn is_authority_source(key: &str) -> bool {
    key.starts_with(PREFIX)
}

pub(in crate::services::agent_cli::grok) fn discover_workspace_trust_sources(
    request: AgentWorkspaceTrustSourceRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    if !request.home.is_absolute()
        || !request.workspace.is_absolute()
        || !request.config_root.is_absolute()
    {
        return;
    }
    let mut emit = |key: &str, path, allowed_root, scope, source_kind| {
        let mut spec = source(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: key,
            label: "Grok 原生工作区信任依据",
            path,
            allowed_root,
            scope,
            precedence: 0,
            sensitive: true,
            source_kind,
            categories: &[],
        });
        spec.writable = false;
        output.emit_initial(spec).is_break()
    };
    if emit(
        STORE_KEY,
        request.config_root.join("trusted_folders.toml"),
        request.config_root,
        AgentAssetScope::User,
        AgentAssetSourceKind::File,
    ) {
        return;
    }
    // Native worktree_record_for_cwd ignores the registry outside this tree.
    // Its directory manifest proves absence without reading an arbitrary SQLite DB.
    let registry_required = request
        .workspace
        .starts_with(request.config_root.join("worktrees"));
    if registry_required
        && emit(
            if request.config_root == request.home {
                HOME_ROOT_KEY
            } else {
                REGISTRY_KEY
            },
            request.config_root.to_owned(),
            request.config_root,
            AgentAssetScope::User,
            AgentAssetSourceKind::Directory,
        )
    {
        return;
    }
    let boundary = if request.workspace.starts_with(request.home) {
        request.home
    } else {
        request.workspace
    };
    for (index, directory) in request
        .workspace
        .ancestors()
        .take_while(|directory| directory.starts_with(boundary))
        .enumerate()
    {
        let root_key = if directory == request.home {
            HOME_ROOT_KEY.to_owned()
        } else {
            format!("{ROOT_PREFIX}{index}")
        };
        let scope = if directory == request.workspace {
            AgentAssetScope::Workspace
        } else {
            AgentAssetScope::User
        };
        for (key, path, kind) in [
            (
                root_key,
                directory.to_owned(),
                AgentAssetSourceKind::Directory,
            ),
            (
                format!("{PREFIX}git:{index}"),
                directory.join(".git"),
                AgentAssetSourceKind::Directory,
            ),
            (
                format!("{PREFIX}head:{index}"),
                directory.join(".git/HEAD"),
                AgentAssetSourceKind::File,
            ),
            (
                format!("{PREFIX}config:{index}"),
                directory.join(".git/config"),
                AgentAssetSourceKind::File,
            ),
        ] {
            if registry_required
                && path == request.config_root
                && kind == AgentAssetSourceKind::Directory
            {
                continue;
            }
            if emit(&key, path, boundary, scope, kind) {
                return;
            }
        }
    }
}

#[derive(Deserialize)]
struct TrustDocument {
    #[serde(default)]
    folders: BTreeMap<String, FolderTrust>,
}

#[derive(Deserialize)]
struct FolderTrust {
    trusted: bool,
    #[serde(default, rename = "decided_at")]
    _decided_at: Option<i64>,
}

pub(in crate::services::agent_cli::grok) fn resolve_workspace_trust(
    request: AgentWorkspaceTrustResolveRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> AgentTrustState {
    let Some(store) = request.sources.iter().find(|source| {
        source.spec.native_source_key == STORE_KEY
            && source.spec.scope == AgentAssetScope::User
            && source.spec.path == source.spec.allowed_root.join("trusted_folders.toml")
    }) else {
        return AgentTrustState::Unknown;
    };
    let AgentAssetSnapshot::File { bytes, .. } = store.snapshot else {
        if let AgentAssetSnapshot::Blocked { diagnostic, .. } = store.snapshot {
            output.emit_diagnostic(diagnostic.clone());
        }
        return AgentTrustState::Unknown;
    };
    let Some(document) = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| toml::from_str::<TrustDocument>(text).ok())
    else {
        output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
            format: AgentAssetDocumentFormat::Toml,
            location: Some("trusted_folders".to_owned()),
        });
        return AgentTrustState::Unknown;
    };
    if request
        .workspace
        .starts_with(store.spec.allowed_root.join("worktrees"))
        && !registry_absent(request.sources, &store.spec.allowed_root)
    {
        return AgentTrustState::Unknown;
    }
    let topology = topology::Topology::new(request.sources);
    // Grok normalizes cwd to workspace_key BEFORE consulting TrustStore.
    let Some(key) = topology.workspace_key(request.workspace) else {
        return AgentTrustState::Unknown;
    };
    if durable_grant(&document, &key, &topology) == Some(true) {
        AgentTrustState::Trusted
    } else {
        AgentTrustState::Unknown
    }
}

fn registry_absent(sources: &[AgentAssetResolveSource<'_>], config_root: &Path) -> bool {
    let Some(source) = sources.iter().find(|source| {
        matches!(
            source.spec.native_source_key.as_str(),
            REGISTRY_KEY | HOME_ROOT_KEY
        ) && source.spec.path == config_root
    }) else {
        return false;
    };
    matches!(source.snapshot, AgentAssetSnapshot::DirectoryManifest { entries, complete: true, .. }
        if !entries.iter().any(|entry| ["worktrees.db", "worktrees.db-wal", "worktrees.db-shm"]
            .iter().any(|name| entry.name.eq_ignore_ascii_case(name))))
}

fn durable_grant(
    document: &TrustDocument,
    query: &Path,
    topology: &topology::Topology<'_>,
) -> Option<bool> {
    let query_id = topology.workspace_key(query)?;
    let mut winner: Option<(usize, bool)> = None;
    let mut uncertain_depth = None;
    for (folder, record) in &document.folders {
        let folder = Path::new(folder);
        if topology.unsafe_root(folder) || !query.starts_with(folder) {
            continue;
        }
        let depth = folder.components().count();
        match topology.workspace_key(folder) {
            Some(folder_id) if folder_id == query_id => match &mut winner {
                Some((best_depth, trusted)) if *best_depth == depth => *trusted &= record.trusted,
                Some((best_depth, _)) if *best_depth > depth => {}
                _ => winner = Some((depth, record.trusted)),
            },
            Some(_) => {}
            None => uncertain_depth = Some(uncertain_depth.unwrap_or(0).max(depth)),
        }
    }
    let (depth, trusted) = winner?;
    if uncertain_depth.is_some_and(|unknown| unknown >= depth) {
        None
    } else {
        Some(trusted)
    }
}
