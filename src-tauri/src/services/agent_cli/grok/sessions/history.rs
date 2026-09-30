use super::*;
use crate::{
    models::AgentSessionRole,
    services::agent_cli::contracts::{
        SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord, SessionHistorySource,
        SessionHistoryWorkspace, SessionMetadataLookupBudget, SessionReadBudget,
    },
};

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    source,
    scan,
    detail,
    search,
    index,
};
fn source() -> Result<SessionHistorySource, String> {
    let config_root =
        super::super::config::config_dir().ok_or_else(|| "无法定位 Grok 原生目录".to_string())?;
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
    _mode: SessionHistoryReadMode,
) -> Result<Vec<SessionHistoryWorkspace>, String> {
    // Grok already enumerates summary.json metadata. No transcript or resume
    // inspection is involved in either mode.
    let sessions_root = root.join("sessions");
    if !sessions_root.exists() {
        return Ok(workdirs
            .iter()
            .map(|workdir| SessionHistoryWorkspace::new(workdir))
            .collect());
    }
    let canonical = sessions_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let metadata_budget = SessionMetadataLookupBudget {
        max_bytes: budget.max_bytes.min(usize::MAX as u64) as usize,
        deadline: budget.deadline,
        cancelled: budget.cancelled.clone(),
    };
    let markers = workspace_markers(&canonical, &metadata_budget, None);
    let marker_error = markers
        .as_ref()
        .err()
        .map(|error| format!("部分 Grok 项目映射不可读：{error:?}"));
    let markers = markers.unwrap_or_default();
    let mut result = Vec::new();
    for workdir in workdirs {
        let mut workspace = SessionHistoryWorkspace::new(workdir);
        if let Some(error) = &marker_error {
            workspace.failed(error.clone());
        }
        match exact_workspace_roots(&canonical, workdir, &metadata_budget, Some(&markers)) {
            Ok(roots) => {
                for root in roots {
                    if let Err(error) = scan_workspace(kind, &root, &mut workspace, budget) {
                        workspace.failed(error);
                    }
                }
            }
            Err(error) => workspace.failed(format!("Grok 项目根不可读：{error:?}")),
        }
        result.push(workspace);
    }
    Ok(result)
}
fn scan_workspace(
    kind: AgentCliKind,
    root: &Path,
    workspace: &mut SessionHistoryWorkspace,
    budget: &SessionReadBudget,
) -> Result<(), String> {
    budget.check()?;
    let entries = fs::read_dir(root).map_err(|error| error.to_string())?;
    for entry in entries {
        budget.check()?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                workspace.failed(error.to_string());
                continue;
            }
        };
        if !entry.file_type().is_ok_and(|ty| ty.is_dir()) {
            continue;
        }
        let path = entry.path().join("summary.json");
        if !path.is_file() {
            continue;
        }
        let record = crate::services::cli_sessions::workbench::cached_history_record(&path, || {
            Ok(
                parse_summary(kind, &path, &workspace.workdir)?.map(|(summary, child)| {
                    SessionHistoryRecord {
                        record_key: summary.id.clone(),
                        summary,
                        locator: path.clone(),
                        role: if child {
                            AgentSessionRole::Subagent
                        } else {
                            AgentSessionRole::Main
                        },
                        parent_native_id: None,
                        resume_reason: child
                            .then(|| "尚未验证 Grok 子 Agent 会话可独立继续".into()),
                        content_unavailable_reason: None,
                    }
                }),
            )
        });
        match record {
            Ok(Some(record)) => workspace.records.push(record),
            Ok(None) => {}
            Err(error) => workspace.failed(error),
        }
    }
    Ok(())
}
fn detail(
    record: &SessionHistoryRecord,
    limits: SessionReadLimits,
) -> Result<CliSessionDetail, String> {
    let session_dir = record
        .locator
        .parent()
        .ok_or_else(|| "Grok Build 会话目录无效".to_string())?;
    let updates_path = session_dir.join("updates.jsonl");
    let chat_history_path = session_dir.join("chat_history.jsonl");
    let (messages, truncated, omitted_message_count, source) = if updates_path.is_file() {
        let (messages, truncated, omitted) = parse_updates(&updates_path, limits)?;
        if messages.is_empty() && chat_history_path.is_file() {
            let (messages, truncated, omitted) = parse_chat_history(&chat_history_path, limits)?;
            (messages, truncated, omitted, "grokChatHistory")
        } else {
            (messages, truncated, omitted, "grokUpdates")
        }
    } else if chat_history_path.is_file() {
        let (messages, truncated, omitted) = parse_chat_history(&chat_history_path, limits)?;
        (messages, truncated, omitted, "grokChatHistory")
    } else {
        return Err("Grok Build 会话摘要存在，但正文文件已不可用".to_string());
    };
    Ok(CliSessionDetail {
        session: record.summary.clone(),
        messages,
        truncated,
        omitted_message_count,
        content_source: source.to_string(),
    })
}
fn search(
    record: &SessionHistoryRecord,
    request: &SessionContentSearchRequest,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String> {
    let session_dir = record
        .locator
        .parent()
        .ok_or_else(|| "Grok Build 会话目录无效".to_string())?;
    let updates_path = session_dir.join("updates.jsonl");
    let chat_history_path = session_dir.join("chat_history.jsonl");
    if !updates_path.is_file() && !chat_history_path.is_file() {
        return Err("Grok Build 会话摘要存在，但正文文件已不可用".to_string());
    }
    let result = if updates_path.is_file() {
        search_updates(&updates_path, request, is_current)?
    } else {
        SessionContentSearchResult::default()
    };
    if result.has_content || !chat_history_path.is_file() {
        return Ok(result);
    }
    search_chat_history(&chat_history_path, request, is_current)
}
fn index(
    record: &SessionHistoryRecord,
    known_fingerprint: Option<&str>,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionIndexLoadResult, String> {
    let session_dir = record
        .locator
        .parent()
        .ok_or_else(|| "Grok Build 会话目录无效".to_string())?;
    let updates_path = session_dir.join("updates.jsonl");
    let chat_history_path = session_dir.join("chat_history.jsonl");
    if !updates_path.is_file() && !chat_history_path.is_file() {
        return Err("Grok Build 会话摘要存在，但正文文件已不可用".to_string());
    }
    let mut source_bytes = 0u64;
    let mut fingerprint_parts = Vec::new();
    for path in [&updates_path, &chat_history_path] {
        if !path.is_file() {
            continue;
        }
        let (part, bytes) = session_index_source_fingerprint(path, INDEX_PARSER_VERSION)?;
        fingerprint_parts.push(part);
        source_bytes = source_bytes.saturating_add(bytes);
    }
    let fingerprint = fingerprint_parts.join("|");
    if known_fingerprint == Some(fingerprint.as_str()) {
        return Ok(SessionIndexLoadResult::Unchanged {
            fingerprint,
            source_bytes,
        });
    }
    let mut messages = if updates_path.is_file() {
        index_updates(&updates_path, is_current)?
    } else {
        Vec::new()
    };
    if messages.is_empty() && chat_history_path.is_file() {
        messages = index_chat_history(&chat_history_path, is_current)?;
    }
    Ok(SessionIndexLoadResult::Updated {
        fingerprint,
        source_bytes,
        messages,
    })
}
