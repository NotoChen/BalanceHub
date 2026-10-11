use super::{scope::path_identity, state};
use crate::{
    models::{AgentCliKind, AgentSessionWorkspace},
    services::agent_cli::contracts::{
        SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryWorkspace, SessionReadBudget,
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
) -> Result<Vec<SessionHistoryWorkspace>, String> {
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
    let paths = paths.into_iter().collect::<Vec<_>>();
    let loaded = (adapter.scan)(kind, root, &paths, budget, mode)?;
    for loaded in loaded {
        let Some(workspace) = merged.get_mut(&path_identity(&loaded.workdir)) else {
            continue;
        };
        workspace.records.extend(loaded.records);
        workspace.complete &= loaded.complete;
        for diagnostic in loaded.diagnostics {
            if !workspace.diagnostics.contains(&diagnostic) {
                workspace.diagnostics.push(diagnostic);
            }
        }
    }
    Ok(merged
        .into_values()
        .map(|mut workspace| {
            workspace.records = crate::services::cli_sessions::normalize_records(workspace.records);
            workspace
        })
        .collect())
}

#[cfg(all(test, unix))]
mod tests;
