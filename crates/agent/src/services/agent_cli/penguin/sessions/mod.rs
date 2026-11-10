//! PenguinHarness trace history.
//!
//! Each Session is a sequence of append-only trace shards:
//! `<data>/<project>/agents/<agent>/traces/<date>/<session>_<index>.jsonl`.
//! Compaction rotates to a new shard, so all shards with one session id form
//! the complete transcript.

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
        clean_text, first_non_empty, normalize_timestamp, read_json_lines_limited,
    },
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const SUMMARY_BYTES: usize = 256 * 1024;
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
        return Err("PenguinHarness 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("PENGUIN_HOME", root);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let root = config::data_dir().ok_or_else(|| "无法定位 PenguinHarness 数据目录".to_string())?;
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
    let mut sessions = Vec::new();
    if let Err(error) = collect(root, budget, &mut result.diagnostics, &mut sessions) {
        result.failed(error);
        return Ok(result);
    }
    sessions.sort_by(|left, right| left.shards.first().cmp(&right.shards.first()));
    let fallback_workdir = (workdirs.len() == 1).then(|| workdirs[0].as_path());
    for session in sessions {
        if let Err(error) = budget.check() {
            result.failed(error);
            break;
        }
        let record = match mode {
            SessionHistoryReadMode::Identities => {
                identity_record(kind, &session, fallback_workdir)
            }
            SessionHistoryReadMode::Summaries => {
                summary_record(kind, &session, fallback_workdir)
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
    let shards = shards_for(&record.locator)?;
    if shards.is_empty() {
        return Err("PenguinHarness 会话记录不存在".into());
    }
    Ok(shards
        .into_iter()
        .map(|path| SessionTranscriptSource {
            path,
            parser_version: INDEX_PARSER_VERSION,
            format: SessionTranscriptFormat::Lines,
            window_safe: true,
            sqlite: None,
            decode: decoder::decode,
        })
        .collect())
}

struct Session {
    project: String,
    agent: String,
    session_id: String,
    shards: Vec<PathBuf>,
}

fn collect(
    root: &Path,
    budget: &SessionReadBudget,
    diagnostics: &mut Vec<String>,
    out: &mut Vec<Session>,
) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("PenguinHarness 数据目录不可读：{error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("PenguinHarness 数据目录不是普通目录".into());
    }
    let mut sessions: BTreeMap<(String, String, String), Vec<PathBuf>> = BTreeMap::new();
    for project in fs::read_dir(root).map_err(|error| error.to_string())? {
        budget.check()?;
        let project = match project {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(error.to_string());
                continue;
            }
        };
        if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let project_name = project.file_name().to_string_lossy().to_string();
        let agents = project.path().join("agents");
        for agent in match fs::read_dir(&agents) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                diagnostics.push(format!("读取 PenguinHarness Agent 目录失败：{error}"));
                continue;
            }
        } {
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
            let agent_name = agent.file_name().to_string_lossy().to_string();
            let traces = agent.path().join("traces");
            for date in match fs::read_dir(&traces) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    diagnostics.push(format!("读取 PenguinHarness Trace 目录失败：{error}"));
                    continue;
                }
            } {
                budget.check()?;
                let date = match date {
                    Ok(entry) => entry,
                    Err(error) => {
                        diagnostics.push(error.to_string());
                        continue;
                    }
                };
                if !date.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                for file in match fs::read_dir(date.path()) {
                    Ok(entries) => entries,
                    Err(error) => {
                        diagnostics.push(format!("读取 PenguinHarness Trace 文件失败：{error}"));
                        continue;
                    }
                } {
                    budget.check()?;
                    let file = match file {
                        Ok(entry) => entry,
                        Err(error) => {
                            diagnostics.push(error.to_string());
                            continue;
                        }
                    };
                    if !file.file_type().is_ok_and(|kind| kind.is_file()) {
                        continue;
                    }
                    let name = file.file_name().to_string_lossy().to_string();
                    let Some(session_id) = session_id_from_name(&name) else {
                        continue;
                    };
                    if sessions.len() >= MAX_SESSION_FILES
                        && !sessions.contains_key(&(
                            project_name.clone(),
                            agent_name.clone(),
                            session_id.clone(),
                        ))
                    {
                        diagnostics.push("PenguinHarness 会话数量超过上限，已跳过部分记录".into());
                        return Ok(());
                    }
                    sessions
                        .entry((project_name.clone(), agent_name.clone(), session_id))
                        .or_default()
                        .push(file.path());
                }
            }
        }
    }
    for ((project, agent, session_id), mut shards) in sessions {
        shards.sort_by_key(|left| shard_sort_key(left));
        out.push(Session {
            project,
            agent,
            session_id,
            shards,
        });
    }
    Ok(())
}

fn session_id_from_name(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".jsonl")?;
    let (session, index) = stem.rsplit_once('_')?;
    if !session.is_empty() && !index.is_empty() && index.chars().all(|value| value.is_ascii_digit()) {
        Some(session.to_owned())
    } else {
        None
    }
}

fn shard_sort_key(path: &Path) -> (String, u32) {
    let date = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned();
    let index = path
        .file_stem()
        .and_then(|value| value.to_str())
        .and_then(|value| value.rsplit_once('_'))
        .and_then(|(_, index)| index.parse::<u32>().ok())
        .unwrap_or_default();
    (date, index)
}

fn shards_for(locator: &Path) -> Result<Vec<PathBuf>, String> {
    let Some(date_dir) = locator.parent() else {
        return Ok(Vec::new());
    };
    let Some(traces) = date_dir.parent() else {
        return Ok(Vec::new());
    };
    let Some(stem) = locator.file_stem().and_then(|value| value.to_str()) else {
        return Ok(Vec::new());
    };
    let Some((session_id, _)) = stem.rsplit_once('_') else {
        return Ok(Vec::new());
    };
    let mut shards = Vec::new();
    for date in fs::read_dir(traces).map_err(|error| error.to_string())? {
        let date = match date {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !date.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        for file in match fs::read_dir(date.path()) {
            Ok(entries) => entries,
            Err(_) => continue,
        } {
            let file = match file {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let name = file.file_name().to_string_lossy().to_string();
            if session_id_from_name(&name).as_deref() == Some(session_id) {
                shards.push(file.path());
            }
        }
    }
    shards.sort_by_key(|left| shard_sort_key(left));
    Ok(shards)
}

fn identity_record(
    kind: AgentCliKind,
    session: &Session,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let mut parsed = FileSummary::default();
    let _ = read_json_lines_limited(
        session.shards.last().unwrap_or(&PathBuf::new()),
        SUMMARY_BYTES,
        "读取 PenguinHarness 会话身份",
        |_, value| parsed.observe(&value),
    )?;
    let workdir = parsed
        .workspace
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| fallback_workdir.map(Path::to_path_buf))
        .unwrap_or_default();
    Ok(Some(SessionHistoryRecord::identity(
        kind,
        record_id(session),
        &workdir,
        session.shards.first().cloned().unwrap_or_default(),
    )))
}

fn summary_record(
    kind: AgentCliKind,
    session: &Session,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let mut parsed = FileSummary::default();
    if let Some(first) = session.shards.first() {
        let _ = read_json_lines_limited(first, SUMMARY_BYTES, "读取 PenguinHarness 会话摘要", |_, value| {
            parsed.observe(&value)
        })?;
    }
    if let Some(last) = session.shards.last() {
        if session.shards.len() > 1 {
            let _ = read_json_lines_limited(last, SUMMARY_BYTES, "读取 PenguinHarness 会话摘要", |_, value| {
                parsed.observe(&value)
            })?;
        }
    }
    if !parsed.seen_record {
        return Ok(None);
    }
    let workdir = parsed
        .workspace
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| fallback_workdir.map(Path::to_path_buf))
        .unwrap_or_default();
    let title = first_non_empty([
        parsed
            .first_user_text
            .clone()
            .and_then(|value| clean_text(value, 100)),
        Some(format!("{} · {}", session.project, session.agent)).and_then(|value| clean_text(value, 100)),
    ]);
    let preview = parsed
        .first_user_text
        .clone()
        .and_then(|value| clean_text(value, 240));
    Ok(Some(SessionHistoryRecord {
        summary: CliSessionSummary {
            // `chat --resume` accepts the upstream session id (or a unique
            // fragment), not BalanceHub's project/agent-qualified record key.
            id: session.session_id.clone(),
            title,
            preview,
            model: parsed.model,
            models: parsed.models.into_iter().collect(),
            cli_kind: kind,
            created_at: normalize_timestamp(parsed.created_at.as_deref()),
            updated_at: normalize_timestamp(parsed.updated_at.as_deref()),
            workdir: workdir.to_string_lossy().into_owned(),
            cli_version: None,
            archived: false,
            can_resume: true,
            metadata_source: "penguinTrace".into(),
        },
        locator: session.shards.first().cloned().unwrap_or_default(),
        record_key: record_id(session),
        role: if parsed.subagent {
            AgentSessionRole::Subagent
        } else {
            AgentSessionRole::Main
        },
        parent_native_id: None,
        resume_reason: None,
        content_unavailable_reason: None,
    }))
}

fn record_id(session: &Session) -> String {
    format!("{}/{}/{}", session.project, session.agent, session.session_id)
}

#[derive(Default)]
struct FileSummary {
    seen_record: bool,
    workspace: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    model: Option<String>,
    models: std::collections::BTreeSet<String>,
    first_user_text: Option<String>,
    subagent: bool,
}

impl FileSummary {
    fn observe(&mut self, value: &Value) {
        self.seen_record = true;
        if let Some(timestamp) = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|value| normalize_timestamp(Some(value)))
        {
            if self.created_at.is_none() {
                self.created_at = Some(timestamp.clone());
            }
            self.updated_at = Some(timestamp);
        }
        let payload = value.get("payload").unwrap_or(&Value::Null);
        match value.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                if let Some(workspace) = payload.get("workspace").and_then(Value::as_str) {
                    self.workspace = Some(workspace.to_owned());
                }
                if let Some(model) = payload.get("model_id").and_then(Value::as_str) {
                    let model = model.trim();
                    if !model.is_empty() {
                        self.models.insert(model.to_owned());
                        self.model = Some(model.to_owned());
                    }
                }
                self.subagent |= payload.get("source").and_then(Value::as_str) == Some("subagent");
            }
            Some("model_msg")
                if payload.get("type").and_then(Value::as_str) == Some("text")
                    && payload.get("role").and_then(Value::as_str) == Some("user")
                    && payload.get("sender").and_then(Value::as_str) != Some("harness")
                    && self.first_user_text.is_none() =>
            {
                self.first_user_text = payload
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            _ => {}
        }
    }
}
