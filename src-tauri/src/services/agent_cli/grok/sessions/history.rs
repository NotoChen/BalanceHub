use super::*;
use crate::{
    models::AgentSessionRole,
    services::agent_cli::contracts::{
        SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord, SessionHistoryScan,
        SessionHistorySource, SessionHistoryWorkspace, SessionMetadataLookupBudget,
        SessionReadBudget,
    },
};

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment: super::super::native_context::launch_environment,
    source,
    scan,
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
) -> Result<SessionHistoryScan, String> {
    // Grok already enumerates summary.json metadata. No transcript or resume
    // inspection is involved in either mode.
    let sessions_root = root.join("sessions");
    if !sessions_root.exists() {
        return Ok(SessionHistoryScan::new(workdirs));
    }
    let canonical = sessions_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if workdirs.is_empty() {
        let mut result = SessionHistoryScan::default();
        for entry in fs::read_dir(&canonical).map_err(|error| error.to_string())? {
            if let Err(error) = budget.check() {
                result.failed(error);
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    result.failed(error.to_string());
                    continue;
                }
            };
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let mut project = SessionHistoryWorkspace::new(Path::new(""));
            if let Err(error) = scan_workspace(kind, &entry.path(), &mut project, budget) {
                project.failed(error);
            }
            for error in project.diagnostics {
                result.failed(error);
            }
            for record in project.records {
                let origin = PathBuf::from(&record.summary.workdir);
                result.workspace(&origin).records.push(record);
            }
        }
        return Ok(result);
    }
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
    let mut result = SessionHistoryScan::default();
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
        result.workspaces.push(workspace);
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
                parse_summary(kind, &path, None)?.map(|(summary, child)| SessionHistoryRecord {
                    record_key: summary.id.clone(),
                    summary,
                    locator: path.clone(),
                    role: if child {
                        AgentSessionRole::Subagent
                    } else {
                        AgentSessionRole::Main
                    },
                    parent_native_id: None,
                    resume_reason: child.then(|| "尚未验证 Grok 子 Agent 会话可独立继续".into()),
                    content_unavailable_reason: None,
                }),
            )
        });
        match record {
            Ok(Some(record)) => {
                if workspace.workdir.as_os_str().is_empty()
                    || path_key(Path::new(&record.summary.workdir)) == path_key(&workspace.workdir)
                {
                    workspace.records.push(record);
                }
            }
            Ok(None) => {}
            Err(error) => workspace.failed(error),
        }
    }
    Ok(())
}
fn index(record: &SessionHistoryRecord) -> Result<Vec<SessionIndexSource>, String> {
    let directory = record.locator.parent().ok_or("Grok Build 会话目录无效")?;
    let mut sources = Vec::new();
    for (name, decode) in [
        (
            "updates.jsonl",
            decoder::update as fn(&Path, u64, &[u8], &mut Value) -> Vec<SessionIndexMutation>,
        ),
        ("chat_history.jsonl", decoder::chat),
    ] {
        let path = directory.join(name);
        if path.is_file() {
            sources.push(SessionIndexSource {
                path,
                parser_version: INDEX_PARSER_VERSION,
                format: SessionIndexFormat::JsonLines,
                decode,
            });
        }
    }
    if sources.is_empty() {
        return Err("Grok Build 会话摘要存在，但正文文件已不可用".into());
    }
    Ok(sources)
}
