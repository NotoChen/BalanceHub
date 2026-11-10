//! Kimi Code native session history.
//!
//! `sessions/<workspace>/<session>/state.json` carries the display metadata;
//! `agents/main/wire.jsonl` is the append-only event journal. Subagent agents
//! keep their own journal and are listed as child sessions.

mod decoder;

use super::config;
use crate::{
    models::{AgentCliKind, AgentSessionRole, CliSessionSummary},
    services::agent_cli::contracts::{
        EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
        SessionHistoryScan, SessionHistorySource, SessionReadBudget, SessionTranscriptFormat,
        SessionTranscriptSource,
    },
    services::cli_sessions::{
        clean_text, first_non_empty, read_session_header,
    },
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

const INDEX_PARSER_VERSION: u32 = 1;
const MAX_SESSION_FILES: usize = 100_000;

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("Kimi Code 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("KIMI_CODE_HOME", root);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let root = config::config_dir().ok_or_else(|| "无法定位 Kimi Code 配置目录".to_string())?;
    let (launch_environment, resume_reason) = match launch_environment(&root) {
        Ok(environment) => (environment, None),
        Err(reason) => (EnvironmentPatch::default(), Some(reason.to_owned())),
    };
    Ok(SessionHistorySource {
        config_root: root,
        launch_environment,
        resume_reason,
    })
}

fn scan(
    kind: AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    let mut result = SessionHistoryScan::new(&[]);
    let mut candidates = Vec::new();
    if let Err(error) = collect(
        &root.join("sessions"),
        budget,
        &mut result.diagnostics,
        &mut candidates,
    ) {
        result.failed(error);
        return Ok(result);
    }
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    let fallback_workdir = (workdirs.len() == 1).then(|| workdirs[0].as_path());
    for candidate in candidates {
        if let Err(error) = budget.check() {
            result.failed(error);
            break;
        }
        let record = match mode {
            SessionHistoryReadMode::Identities => {
                identity_record(kind, &candidate, fallback_workdir)
            }
            SessionHistoryReadMode::Summaries => {
                summary_record(kind, &candidate, fallback_workdir)
            }
        };
        match record {
            Ok(Some(record)) => {
                let workdir = PathBuf::from(&record.summary.workdir);
                result.workspace(&workdir).records.push(record);
            }
            Ok(None) => {}
            Err(error) => result.failed(error),
        }
    }
    Ok(result)
}

fn transcript(record: &SessionHistoryRecord) -> Result<Vec<SessionTranscriptSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    Ok(vec![SessionTranscriptSource {
        path: record.locator.clone(),
        parser_version: INDEX_PARSER_VERSION,
        format: SessionTranscriptFormat::Lines,
        window_safe: true,
        sqlite: None,
        decode: decoder::decode,
    }])
}

struct Candidate {
    path: PathBuf,
    session_id: String,
    agent_id: String,
}

fn collect(
    sessions: &Path,
    budget: &SessionReadBudget,
    diagnostics: &mut Vec<String>,
    out: &mut Vec<Candidate>,
) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(sessions) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Kimi Code 会话目录不可读：{error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Kimi Code 会话目录不是普通目录".into());
    }
    for workspace in fs::read_dir(sessions).map_err(|error| error.to_string())? {
        budget.check()?;
        let workspace = match workspace {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(error.to_string());
                continue;
            }
        };
        if !workspace.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let workspace = workspace.path();
        for session in match fs::read_dir(&workspace) {
            Ok(entries) => entries,
            Err(error) => {
                diagnostics.push(format!("读取 Kimi Code 会话目录失败：{error}"));
                continue;
            }
        } {
            budget.check()?;
            let session = match session {
                Ok(entry) => entry,
                Err(error) => {
                    diagnostics.push(error.to_string());
                    continue;
                }
            };
            if !session.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let session_dir = session.path();
            let session_id = session.file_name().to_string_lossy().to_string();
            let agents = session_dir.join("agents");
            let agent_entries = match fs::read_dir(&agents) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    diagnostics.push(format!("读取 Kimi Code Agent 目录失败：{error}"));
                    continue;
                }
            };
            for agent in agent_entries {
                budget.check()?;
                let agent = match agent {
                    Ok(entry) => entry,
                    Err(error) => {
                        diagnostics.push(error.to_string());
                        continue;
                    }
                };
                if !agent.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                let wire = agent.path().join("wire.jsonl");
                if !wire.is_file() {
                    continue;
                }
                if out.len() >= MAX_SESSION_FILES {
                    diagnostics.push("Kimi Code 会话文件数量超过上限，已跳过部分记录".into());
                    return Ok(());
                }
                out.push(Candidate {
                    path: wire,
                    session_id: session_id.clone(),
                    agent_id: agent.file_name().to_string_lossy().to_string(),
                });
            }
        }
    }
    Ok(())
}

fn state_path(candidate: &Candidate) -> PathBuf {
    candidate
        .path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|session| session.join("state.json"))
        .unwrap_or_default()
}

fn read_header(candidate: &Candidate) -> Option<serde_json::Map<String, Value>> {
    let path = state_path(candidate);
    read_session_header(
        &path,
        &[
            "id",
            "title",
            "lastPrompt",
            "createdAt",
            "updatedAt",
            "archived",
            "cwd",
        ],
        |fields, _| fields.get("id").is_some() && fields.get("updatedAt").is_some(),
    )
    .ok()
    .flatten()
}

fn workdir(header: &serde_json::Map<String, Value>, fallback: Option<&Path>) -> PathBuf {
    header
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| fallback.map(Path::to_path_buf))
        .unwrap_or_default()
}

fn id_for(candidate: &Candidate, header: &serde_json::Map<String, Value>) -> String {
    let main = header
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&candidate.session_id);
    if candidate.agent_id == "main" {
        main.to_owned()
    } else {
        format!("{main}:{}", candidate.agent_id)
    }
}

fn identity_record(
    kind: AgentCliKind,
    candidate: &Candidate,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let Some(header) = read_header(candidate) else {
        return Ok(None);
    };
    Ok(Some(SessionHistoryRecord::identity(
        kind,
        id_for(candidate, &header),
        &workdir(&header, fallback_workdir),
        candidate.path.clone(),
    )))
}

fn summary_record(
    kind: AgentCliKind,
    candidate: &Candidate,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let Some(header) = read_header(candidate) else {
        return Ok(None);
    };
    let id = id_for(candidate, &header);
    let title = header
        .get("title")
        .and_then(Value::as_str)
        .and_then(|value| clean_text(value, 100));
    let last_prompt = header
        .get("lastPrompt")
        .and_then(Value::as_str)
        .and_then(|value| clean_text(value, 240));
    let subagent = candidate.agent_id != "main";
    let title = if subagent {
        first_non_empty([
            title.clone(),
            last_prompt.clone(),
            Some(candidate.agent_id.clone()),
        ])
    } else {
        first_non_empty([title, last_prompt.clone()])
    };
    Ok(Some(SessionHistoryRecord {
        summary: CliSessionSummary {
            id: id.clone(),
            title,
            preview: last_prompt,
            model: None,
            models: Vec::new(),
            cli_kind: kind,
            created_at: header
                .get("createdAt")
                .and_then(Value::as_i64)
                .and_then(|value| crate::services::cli_sessions::timestamp_from_value(Some(value), false)),
            updated_at: header
                .get("updatedAt")
                .and_then(Value::as_i64)
                .and_then(|value| crate::services::cli_sessions::timestamp_from_value(Some(value), false)),
            workdir: workdir(&header, fallback_workdir)
                .to_string_lossy()
                .into_owned(),
            cli_version: None,
            archived: header.get("archived").and_then(Value::as_bool).unwrap_or(false),
            can_resume: !subagent,
            metadata_source: "kimiWire".into(),
        },
        locator: candidate.path.clone(),
        record_key: id,
        role: if subagent {
            AgentSessionRole::Subagent
        } else {
            AgentSessionRole::Main
        },
        parent_native_id: subagent.then(|| candidate.session_id.clone()),
        resume_reason: subagent.then(|| "Kimi Code 子代理会话不能单独续聊".into()),
        content_unavailable_reason: None,
    }))
}
