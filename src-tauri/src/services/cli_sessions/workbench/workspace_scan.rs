use super::{scope::path_identity, state};
use crate::{
    models::{AgentCliKind, AgentSessionWorkspace},
    platform::paths::compatible_path,
    services::agent_cli::contracts::{
        SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryScan, SessionHistoryWorkspace,
        SessionReadBudget,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// Native logs retain the spelling of their original working directory. Keep
/// proven aliases of the selected physical directories when querying adapters,
/// then merge them before counting, indexing or building session references.
pub(super) fn scan_workspaces(
    adapter: &SessionHistoryAdapter,
    kind: AgentCliKind,
    root: &Path,
    workspaces: &[AgentSessionWorkspace],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    budget.check()?;
    let mut merged = BTreeMap::new();
    let mut paths = BTreeSet::new();
    for workspace in workspaces {
        budget.check()?;
        let path = PathBuf::from(&workspace.path);
        merged.insert(path_identity(&path), SessionHistoryWorkspace::new(&path));
        paths.insert(path);
    }
    let aliases = state::registry()
        .path_identities
        .iter()
        .filter(|(logical, identity)| *logical != *identity && merged.contains_key(*identity))
        .map(|(logical, identity)| (PathBuf::from(logical), identity.clone()))
        .collect::<Vec<_>>();
    for (path, identity) in aliases {
        budget.check()?;
        // A reachable alias may have been retargeted since scope creation.
        // Missing mounts keep their last proven identity for read-only history.
        if path_identity(&path) == identity {
            paths.insert(path);
        }
    }
    let paths = if paths.iter().any(|path| path.as_os_str().is_empty()) {
        Vec::new()
    } else {
        paths.into_iter().collect::<Vec<_>>()
    };
    // Guest logs contain Linux paths. Scan their native source once, then map
    // the workdir before identity, selection and indexing on the Windows host.
    let guest = crate::services::wsl::split_host_path(root).is_some();
    let scan = (adapter.scan)(kind, root, if guest { &[] } else { &paths }, budget, mode)?;
    for mut loaded in scan.workspaces {
        if guest && !loaded.workdir.as_os_str().is_empty() {
            match crate::services::wsl::recorded_path(root, &loaded.workdir.to_string_lossy()) {
                Ok(path) => loaded.workdir = path,
                Err(error) => {
                    // One malformed origin cannot hide the other guest sessions.
                    // Its transcript remains readable without granting resume.
                    loaded.failed(error);
                    loaded.workdir = PathBuf::new();
                }
            }
        }
        let identity = path_identity(&loaded.workdir);
        if !workspaces.is_empty() && !merged.contains_key(&identity) {
            continue;
        }
        let workspace = merged
            .entry(identity.clone())
            .or_insert_with(|| SessionHistoryWorkspace::new(Path::new(&identity)));
        workspace.records.extend(loaded.records);
        workspace.complete &= loaded.complete;
        for diagnostic in loaded.diagnostics {
            if !workspace.diagnostics.contains(&diagnostic) {
                workspace.diagnostics.push(diagnostic);
            }
        }
    }
    Ok(SessionHistoryScan {
        workspaces: merged
            .into_values()
            .map(|mut workspace| {
                workspace.records =
                    crate::services::cli_sessions::normalize_records(workspace.records);
                for record in &mut workspace.records {
                    record.summary.workdir = compatible_path(&workspace.workdir)
                        .to_string_lossy()
                        .into_owned();
                }
                workspace
            })
            .collect(),
        diagnostics: scan.diagnostics,
    })
}

#[cfg(test)]
mod tests;
