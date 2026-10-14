use super::*;
use crate::{
    models::AgentSessionRole,
    services::agent_cli::contracts::{
        SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord, SessionHistoryScan,
        SessionHistorySource, SessionHistoryWorkspace, SessionReadBudget,
    },
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

mod unindexed;

const UNAVAILABLE_ROLLOUT_REASON: &str = "原生会话日志不可用，仅能查看索引摘要";

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    relocation: None,
    launch_environment: super::super::native_context::launch_environment,
    source,
    scan,
    index,
};
fn source() -> Result<SessionHistorySource, String> {
    let config_root = codex_home()?;
    let launch_environment =
        super::super::native_context::launch_environment(&config_root).map_err(str::to_owned)?;
    Ok(SessionHistorySource {
        config_root,
        launch_environment,
        resume_reason: None,
    })
}
fn scan(
    kind: AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    let mut result = SessionHistoryScan::new(workdirs);
    let titles = match (mode == SessionHistoryReadMode::Summaries)
        .then(|| super::index::read_session_titles(root))
        .transpose()
    {
        Ok(None) => HashMap::new(),
        Ok(Some(titles)) => titles,
        Err(error) => {
            result.failed(error.clone());
            for workspace in &mut result.workspaces {
                workspace.failed(error.clone());
            }
            HashMap::new()
        }
    };
    let mut workspace_indexes = HashMap::new();
    for (index, workdir) in workdirs.iter().enumerate() {
        workspace_indexes.insert(workdir.to_string_lossy().to_string(), index);
        if let Ok(path) = workdir.canonicalize() {
            workspace_indexes.insert(path.to_string_lossy().to_string(), index);
        }
    }
    let mut fallbacks: Option<HashMap<String, PathBuf>> = None;
    let mut indexed_locators = HashSet::new();
    for database in state_databases(root)? {
        let read = super::index::read_database_history(
            kind,
            &database,
            workdirs,
            budget,
            |mut record, source, parent| {
                let index = if let Some(index) = workspace_indexes.get(&record.summary.workdir) {
                    *index
                } else if workdirs.is_empty() {
                    let index = result.workspaces.len();
                    result
                        .workspaces
                        .push(SessionHistoryWorkspace::new(Path::new(
                            &record.summary.workdir,
                        )));
                    workspace_indexes.insert(record.summary.workdir.clone(), index);
                    index
                } else {
                    return;
                };
                let workspace = &mut result.workspaces[index];
                if let Some(title) = titles
                    .get(&record.summary.id)
                    .and_then(|value| clean_text(value, 100))
                {
                    record.summary.title = title;
                }
                let (role, parent_native_id) = decode_role(source.as_deref(), parent);
                record.summary.can_resume = false;
                record.summary.workdir = workspace.workdir.to_string_lossy().to_string();
                let metadata_locator = if record.rollout_path.starts_with('/')
                    && crate::services::wsl::split_host_path(root).is_some()
                {
                    crate::services::wsl::recorded_path(root, &record.rollout_path)
                } else {
                    Ok(root.join(&record.rollout_path))
                };
                let locator_valid = metadata_locator.is_ok();
                let metadata_record = SessionHistoryRecord {
                    record_key: record.summary.id.clone(),
                    summary: record.summary,
                    // This is the exact native index value, not a fabricated
                    // transcript. Unavailable records never authorize reads.
                    locator: metadata_locator
                        .unwrap_or_else(|_| PathBuf::from(&record.rollout_path)),
                    role,
                    parent_native_id,
                    resume_reason: Some(UNAVAILABLE_ROLLOUT_REASON.into()),
                    content_unavailable_reason: Some(UNAVAILABLE_ROLLOUT_REASON.into()),
                };
                if mode == SessionHistoryReadMode::Identities {
                    if locator_valid {
                        indexed_locators.insert(
                            crate::services::cli_sessions::source::existing_variant(
                                &metadata_record.locator,
                            ),
                        );
                    }
                    workspace.records.push(metadata_record);
                    return;
                }
                let locator = match resolve_record_locator(
                    root,
                    &record.rollout_path,
                    &mut fallbacks,
                    budget,
                ) {
                    Ok(path) => path,
                    Err(error) => {
                        workspace.failed(error);
                        if budget.check().is_ok() {
                            workspace.records.push(metadata_record);
                        }
                        return;
                    }
                };
                let loaded = crate::services::cli_sessions::workbench::cached_history_record(
                    &locator,
                    || unindexed::read_record(kind, root, &locator, mode),
                );
                match loaded {
                    Ok(Some(mut record)) => {
                        if !record.summary.id.is_empty()
                            && record.summary.id != metadata_record.summary.id
                        {
                            workspace.failed("Codex 索引与日志的原生会话 ID 不一致");
                            workspace.records.push(metadata_record);
                            return;
                        }
                        indexed_locators.insert(locator.clone());
                        // The transcript fingerprint does not change when the
                        // native database title, archive or parent is updated.
                        record.summary = metadata_record.summary.clone();
                        record.record_key = metadata_record.record_key.clone();
                        if record.role == AgentSessionRole::Unknown {
                            record.role = metadata_record.role;
                        }
                        record.parent_native_id = record
                            .parent_native_id
                            .or_else(|| metadata_record.parent_native_id.clone());
                        set_resume_capability(&mut record);
                        if let Some(reason) = &record.content_unavailable_reason {
                            workspace.failed(reason.clone());
                        }
                        workspace.records.push(record);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        workspace.failed(error);
                        if budget.check().is_ok() {
                            workspace.records.push(metadata_record);
                        }
                    }
                }
            },
        );
        if let Err(error) = read {
            result.failed(error.clone());
            for workspace in &mut result.workspaces {
                workspace.failed(error.clone());
            }
        }
    }
    unindexed::Scan {
        kind,
        root,
        workdirs,
        indexed_locators: &indexed_locators,
        titles: &titles,
        budget,
        mode,
    }
    .append(fallbacks, &mut result);
    Ok(result)
}

fn set_resume_capability(record: &mut SessionHistoryRecord) {
    record.resume_reason = if record.summary.archived {
        Some("该 Codex 会话已归档".into())
    } else if record.role == AgentSessionRole::Subagent {
        Some("尚未验证 Codex 子 Agent 会话可独立继续".into())
    } else if record.role == AgentSessionRole::Unknown {
        Some("原生会话角色未知，暂无法确认独立继续能力".into())
    } else {
        None
    };
    record.summary.can_resume = record.resume_reason.is_none();
}
pub(super) fn resolve_record_locator(
    root: &Path,
    indexed: &str,
    fallbacks: &mut Option<HashMap<String, PathBuf>>,
    budget: &SessionReadBudget,
) -> Result<PathBuf, String> {
    let indexed =
        if crate::services::wsl::split_host_path(root).is_some() && indexed.starts_with('/') {
            crate::services::wsl::recorded_path(root, indexed)?
        } else {
            PathBuf::from(indexed)
        };
    let direct = if indexed.is_absolute() {
        indexed.clone()
    } else {
        root.join(&indexed)
    };
    let direct = crate::services::cli_sessions::source::existing_variant(&direct);
    if direct.is_file()
        && direct
            .canonicalize()
            .map_err(|error| error.to_string())?
            .starts_with(root.canonicalize().map_err(|error| error.to_string())?)
    {
        return Ok(direct);
    }
    let logical = crate::services::cli_sessions::source::logical_path(&indexed);
    let name = logical
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Codex 正文定位缺失".to_string())?;
    if fallbacks.is_none() {
        *fallbacks = Some(rollout_files(root, budget)?);
    }
    fallbacks
        .as_ref()
        .and_then(|files| files.get(name))
        .cloned()
        .ok_or_else(|| "Codex 原生会话正文已不可用".into())
}

fn rollout_files(
    root: &Path,
    budget: &SessionReadBudget,
) -> Result<HashMap<String, PathBuf>, String> {
    let mut result = HashMap::new();
    let mut directories = vec![root.join("sessions"), root.join("archived_sessions")];
    while let Some(directory) = directories.pop() {
        budget.check()?;
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        for entry in entries {
            budget.check()?;
            let entry = entry.map_err(|e| e.to_string())?;
            let ty = entry.file_type().map_err(|e| e.to_string())?;
            if ty.is_dir() {
                directories.push(entry.path());
            } else if ty.is_file()
                && crate::services::cli_sessions::source::json_lines(&entry.path())
            {
                let logical = crate::services::cli_sessions::source::logical_path(&entry.path());
                let name = logical
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                // Both forms may coexist while Codex compresses an archived
                // rollout. Prefer the uncompressed file deterministically.
                if !result.contains_key(&name)
                    || !crate::services::cli_sessions::source::compressed(&entry.path())
                {
                    result.insert(name, entry.path());
                }
            }
        }
    }
    Ok(result)
}
fn decode_role(source: Option<&str>, parent: Option<String>) -> (AgentSessionRole, Option<String>) {
    let value = source
        .and_then(|source| serde_json::from_str::<Value>(source).ok())
        .or_else(|| source.map(|source| Value::String(source.into())));
    let mut parent = parent.filter(|id| !id.trim().is_empty());
    let role = match value.as_ref() {
        Some(Value::String(kind))
            if matches!(
                kind.as_str(),
                "cli" | "vscode" | "exec" | "appserver" | "app_server"
            ) =>
        {
            AgentSessionRole::Main
        }
        Some(Value::Object(source)) if source.contains_key("subagent") => {
            if let Some(id) = source
                .get("subagent")
                .and_then(|kind| kind.get("thread_spawn"))
                .and_then(|spawn| spawn.get("parent_thread_id"))
                .and_then(Value::as_str)
            {
                parent = Some(id.into());
            }
            AgentSessionRole::Subagent
        }
        _ if parent.is_some() => AgentSessionRole::Subagent,
        _ => AgentSessionRole::Unknown,
    };
    (role, parent)
}
fn index(record: &SessionHistoryRecord) -> Result<Vec<SessionIndexSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    Ok(vec![index_source(&record.locator)])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_subagent_sources_and_explicit_parents_do_not_guess_fork_relations() {
        assert_eq!(
            decode_role(
                Some(r#"{"subagent":{"thread_spawn":{"parent_thread_id":"parent","depth":1}}}"#),
                None
            ),
            (AgentSessionRole::Subagent, Some("parent".into()))
        );
        assert_eq!(
            decode_role(Some(r#"{"subagent":"review"}"#), None),
            (AgentSessionRole::Subagent, None)
        );
        assert_eq!(
            decode_role(None, Some("native-parent".into())),
            (AgentSessionRole::Subagent, Some("native-parent".into()))
        );
        assert_eq!(decode_role(None, None), (AgentSessionRole::Unknown, None));
        assert_eq!(
            decode_role(Some("internal"), None),
            (AgentSessionRole::Unknown, None)
        );
        assert_eq!(
            decode_role(Some("cli"), None),
            (AgentSessionRole::Main, None)
        );
    }
}
