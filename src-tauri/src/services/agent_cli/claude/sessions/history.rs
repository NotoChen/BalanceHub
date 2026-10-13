use super::projects::{ProjectCandidate, ProjectLookup, ProjectMatch, ProjectSelector};
use super::*;
use crate::{
    models::AgentSessionRole,
    services::{
        agent_cli::contracts::{
            SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
            SessionHistoryScan, SessionHistorySource, SessionHistoryWorkspace, SessionReadBudget,
        },
        cli_sessions::workbench::{cached_history_record_facts, HistoryRecordFacts},
    },
};
use std::path::PathBuf;

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment: super::super::native_context::launch_environment,
    source,
    scan,
    index,
};
fn source() -> Result<SessionHistorySource, String> {
    let config_root =
        super::super::config::config_dir().ok_or_else(|| "无法定位 Claude 原生目录".to_string())?;
    let launch_environment =
        super::super::native_context::launch_environment(&config_root).map_err(str::to_owned)?;
    Ok(SessionHistorySource {
        config_root,
        launch_environment,
        resume_reason: None,
    })
}
pub(super) fn is_subagent_path(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "subagents")
}
fn scan(
    kind: AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    let lookup = ProjectLookup::new(root, budget)?;
    if workdirs.is_empty() {
        let mut result = SessionHistoryScan::default();
        for project in lookup.discover_all(budget)? {
            if let Err(error) = budget.check() {
                result.failed(error);
                break;
            }
            let mut records = SessionHistoryWorkspace::new(Path::new(""));
            if let Err(error) = scan_project(kind, &project, None, &mut records, budget, mode) {
                records.failed(error);
            }
            for diagnostic in records.diagnostics {
                result.failed(diagnostic);
            }
            for record in records.records {
                let origin = PathBuf::from(&record.summary.workdir);
                result.workspace(&origin).records.push(record);
            }
        }
        return Ok(result);
    }
    let selectors = workdirs
        .iter()
        .map(|path| {
            budget.check()?;
            let selector = ProjectSelector::new(path);
            budget.check()?;
            Ok(selector)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let candidates = lookup.discover(&selectors, budget)?;
    let mut result = SessionHistoryScan::default();
    for ((workdir, selector), projects) in workdirs.iter().zip(&selectors).zip(candidates) {
        let mut workspace = SessionHistoryWorkspace::new(workdir);
        for project in projects {
            if let Err(error) =
                scan_project(kind, &project, Some(selector), &mut workspace, budget, mode)
            {
                workspace.failed(error);
            }
        }
        result.workspaces.push(workspace);
    }
    Ok(result)
}
fn scan_project(
    kind: AgentCliKind,
    project: &ProjectCandidate,
    selector: Option<&ProjectSelector>,
    workspace: &mut SessionHistoryWorkspace,
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<(), String> {
    budget.check()?;
    let entries = match fs::read_dir(&project.path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("读取 Claude 项目目录失败：{error}")),
    };
    for entry in entries {
        budget.check()?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                workspace.failed(error.to_string());
                continue;
            }
        };
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_symlink() {
            continue;
        }
        let path = entry.path();
        if ty.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
            add_record(kind, &path, project.kind, selector, workspace, mode);
        } else if ty.is_dir() {
            let child_root = path.join("subagents");
            if fs::symlink_metadata(&child_root)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                workspace.failed("Claude 子会话目录为符号链接，已跳过");
                continue;
            }
            let children = match fs::read_dir(child_root) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    workspace.failed(error.to_string());
                    continue;
                }
            };
            for child in children {
                budget.check()?;
                match child {
                    Ok(child)
                        if child.file_type().is_ok_and(|ty| ty.is_file())
                            && child.path().extension().is_some_and(|ext| ext == "jsonl") =>
                    {
                        add_record(kind, &child.path(), project.kind, selector, workspace, mode)
                    }
                    Ok(_) => {}
                    Err(error) => workspace.failed(error.to_string()),
                }
            }
        }
    }
    Ok(())
}
fn add_record(
    kind: AgentCliKind,
    path: &Path,
    project_match: ProjectMatch,
    selector: Option<&ProjectSelector>,
    workspace: &mut SessionHistoryWorkspace,
    mode: SessionHistoryReadMode,
) {
    let read = || {
        let (mut record, native_origin_workdir) = if mode == SessionHistoryReadMode::Identities {
            let mut native_id = None;
            let mut native_origin = None;
            let metadata = crate::services::cli_sessions::read_session_header(
                path,
                &["sessionId", "session_id", "cwd", "workdir", "isSidechain"],
                |fields, complete| {
                    if !is_subagent_path(path) && !complete && !fields.contains_key("isSidechain") {
                        return false;
                    }
                    if !is_subagent_path(path)
                        && fields.get("isSidechain").and_then(Value::as_bool) == Some(true)
                    {
                        return false;
                    }
                    let (id, origin) = transcript_identity(&Value::Object(fields.clone()));
                    // Match the summary parser's first native ID and origin,
                    // even when they are recorded in different native events.
                    native_id = native_id.take().or(id);
                    native_origin = native_origin.take().or(origin);
                    native_id.is_some()
                        && (project_match != ProjectMatch::Prefix
                            || native_origin.is_some()
                            || (selector.is_none() && complete))
                },
            )?;
            if metadata.is_none() {
                return Ok(None);
            }
            let id = native_id.ok_or("Claude 会话身份缺失")?;
            (
                SessionHistoryRecord::identity(kind, id, &workspace.workdir, path.to_path_buf()),
                native_origin,
            )
        } else {
            let Some(ParsedClaudeTranscript {
                summary,
                native_origin_workdir,
            }) = parse_transcript(kind, path)?
            else {
                return Ok(None);
            };
            (
                SessionHistoryRecord {
                    record_key: summary.id.clone(),
                    summary,
                    locator: path.to_path_buf(),
                    role: AgentSessionRole::Main,
                    parent_native_id: None,
                    resume_reason: None,
                    content_unavailable_reason: None,
                },
                native_origin_workdir,
            )
        };
        let child = is_subagent_path(path);
        if child {
            let parent = path
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|value| value.to_str());
            record.parent_native_id = parent
                .filter(|parent| *parent == record.summary.id)
                .map(str::to_string);
            record.summary.id = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or(&record.summary.id)
                .to_string();
            record.summary.can_resume = false;
            record
                .resume_reason
                .get_or_insert_with(|| "尚未验证 Claude 子 Agent 会话可独立继续".into());
        }
        record.record_key = if child {
            path.strip_prefix(
                path.parent()
                    .and_then(Path::parent)
                    .and_then(Path::parent)
                    .unwrap_or(path),
            )
            .unwrap_or(path)
            .to_string_lossy()
            .to_string()
        } else {
            record.summary.id.clone()
        };
        record.role = if child {
            AgentSessionRole::Subagent
        } else {
            AgentSessionRole::Main
        };
        Ok(Some(HistoryRecordFacts {
            record,
            native_origin_workdir,
        }))
    };
    // A long-path candidate requires origin evidence, while an exact native
    // project directory does not. Never reuse a shorter identity-only header
    // read as proof for a different candidate's stronger requirement.
    let result = if mode == SessionHistoryReadMode::Identities {
        read()
    } else {
        cached_history_record_facts(path, read)
    };
    match result {
        Ok(Some(facts)) => {
            if let Some(selector) = selector.filter(|_| project_match == ProjectMatch::Prefix) {
                match selector.proves_origin(facts.native_origin_workdir.as_deref()) {
                    Some(true) => {}
                    Some(false) => return,
                    None => {
                        if let Some(reason) = &facts.record.content_unavailable_reason {
                            workspace.failed(reason.clone());
                        }
                        workspace
                            .failed("Claude 长路径候选缺少可验证的原生工作目录归属，已跳过会话");
                        return;
                    }
                }
            }
            let mut record = facts.record;
            record.summary.workdir = if selector.is_some() {
                workspace.workdir.to_string_lossy().to_string()
            } else {
                facts
                    .native_origin_workdir
                    .as_deref()
                    .unwrap_or_else(|| Path::new(""))
                    .to_string_lossy()
                    .into_owned()
            };
            if let Some(reason) = &record.content_unavailable_reason {
                workspace.failed(reason.clone());
            }
            workspace.records.push(record);
        }
        Ok(None) => {}
        Err(error) => workspace.failed(error),
    }
}

fn index(record: &SessionHistoryRecord) -> Result<Vec<SessionIndexSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    Ok(vec![index_source(&record.locator)])
}
