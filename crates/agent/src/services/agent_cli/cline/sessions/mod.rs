//! Cline CLI session history.
//!
//! Cline writes one directory per Session below its session data root. The
//! directory holds a manifest (`<id>.json`) and the Anthropic-style message
//! list (`<id>.messages.json`). Only directories with a manifest count as
//! Sessions; subagent/teammate message artifacts inside the same directory are
//! not separate Session roots.

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
        clean_text, first_non_empty, normalize_timestamp, read_json_messages_limited,
        read_session_header,
    },
};
use serde_json::Value;
use std::{
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
        return Err("Cline 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("CLINE_SESSION_DATA_DIR", root);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let root = config::session_data_dir().ok_or_else(|| "无法定位 Cline 会话目录".to_string())?;
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
    sessions.sort_by(|left, right| left.id.cmp(&right.id));
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
    let path = record.locator.clone();
    if !path.is_file() {
        return Err("Cline 会话消息文件不存在".into());
    }
    Ok(vec![SessionTranscriptSource {
        path,
        parser_version: INDEX_PARSER_VERSION,
        format: SessionTranscriptFormat::Messages,
        window_safe: true,
        sqlite: None,
        decode: decoder::decode,
    }])
}

struct Session {
    id: String,
    directory: PathBuf,
    manifest: PathBuf,
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
        Err(error) => return Err(format!("Cline 会话目录不可读：{error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Cline 会话目录不是普通目录".into());
    }
    for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        budget.check()?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(error.to_string());
                continue;
            }
        };
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let manifest = entry.path().join(format!("{name}.json"));
        if !manifest.is_file() {
            continue;
        }
        if out.len() >= MAX_SESSION_FILES {
            diagnostics.push("Cline 会话数量超过上限，已跳过部分记录".into());
            return Ok(());
        }
        out.push(Session {
            id: name,
            directory: entry.path(),
            manifest,
        });
    }
    Ok(())
}

fn read_header(path: &Path) -> Option<serde_json::Map<String, Value>> {
    read_session_header(
        path,
        &[
            "session_id",
            "provider",
            "model",
            "cwd",
            "workspace_root",
            "started_at",
            "ended_at",
            "messages_path",
        ],
        |fields, _| {
            fields
                .get("session_id")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        },
    )
    .ok()
    .flatten()
}

fn session_id(header: &serde_json::Map<String, Value>, fallback: &str) -> String {
    header
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

fn messages_path(
    header: &serde_json::Map<String, Value>,
    session: &Session,
) -> PathBuf {
    let configured = header
        .get("messages_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    if let Some(path) = configured {
        let path = if path.is_absolute() {
            path
        } else {
            session.directory.join(path)
        };
        if path.is_file() {
            return path;
        }
    }
    session
        .directory
        .join(format!("{}.messages.json", session.id))
}

fn workdir(header: &serde_json::Map<String, Value>, fallback: Option<&Path>) -> PathBuf {
    ["cwd", "workspace_root"]
        .iter()
        .find_map(|key| header.get(*key).and_then(Value::as_str))
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| fallback.map(Path::to_path_buf))
        .unwrap_or_default()
}

fn identity_record(
    kind: AgentCliKind,
    session: &Session,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let Some(header) = read_header(&session.manifest) else {
        return Ok(None);
    };
    let id = session_id(&header, &session.id);
    Ok(Some(SessionHistoryRecord::identity(
        kind,
        id,
        &workdir(&header, fallback_workdir),
        messages_path(&header, session),
    )))
}

fn summary_record(
    kind: AgentCliKind,
    session: &Session,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let Some(header) = read_header(&session.manifest) else {
        return Ok(None);
    };
    let id = session_id(&header, &session.id);
    let messages = messages_path(&header, session);
    let mut parsed = FileSummary::default();
    if messages.is_file() {
        read_json_messages_limited(&messages, SUMMARY_BYTES, &|| true, |value| {
            parsed.observe(&value)
        })?;
    }
    let model = header
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or(parsed.model);
    let mut models = parsed.models;
    if let Some(model) = &model {
        models.insert(model.clone());
    }
    let title = first_non_empty([
        parsed
            .first_user_text
            .clone()
            .and_then(|value| clean_text(value, 100)),
        Some(format!("Cline 会话 {id}")).and_then(|value| clean_text(value, 100)),
    ]);
    let preview = parsed
        .first_user_text
        .and_then(|value| clean_text(value, 240));
    let updated_at = header
        .get("ended_at")
        .and_then(Value::as_str)
        .and_then(|value| normalize_timestamp(Some(value)))
        .or_else(|| {
            fs::metadata(&messages)
                .and_then(|metadata| metadata.modified())
                .ok()
                .map(chrono::DateTime::<chrono::Utc>::from)
                .map(|time| time.to_rfc3339())
        });
    Ok(Some(SessionHistoryRecord {
        summary: CliSessionSummary {
            id: id.clone(),
            title,
            preview,
            model,
            models: models.into_iter().collect(),
            cli_kind: kind,
            created_at: header
                .get("started_at")
                .and_then(Value::as_str)
                .and_then(|value| normalize_timestamp(Some(value))),
            updated_at,
            workdir: workdir(&header, fallback_workdir).to_string_lossy().into_owned(),
            cli_version: None,
            archived: false,
            can_resume: true,
            metadata_source: "clineSession".into(),
        },
        locator: messages,
        record_key: id,
        role: AgentSessionRole::Main,
        parent_native_id: None,
        resume_reason: None,
        content_unavailable_reason: None,
    }))
}

#[derive(Default)]
struct FileSummary {
    first_user_text: Option<String>,
    model: Option<String>,
    models: std::collections::BTreeSet<String>,
}

impl FileSummary {
    fn observe(&mut self, value: &Value) {
        if let Some(model) = value
            .pointer("/modelInfo/modelId")
            .or_else(|| value.pointer("/modelInfo/model"))
            .or_else(|| value.get("model"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            self.models.insert(model.to_owned());
            self.model = Some(model.to_owned());
        }
        let role = value
            .get("role")
            .or_else(|| value.pointer("/message/role"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if role == "user" && self.first_user_text.is_none() {
            self.first_user_text = message_text(value.get("content"))
                .filter(|text| !text.trim().is_empty());
        }
    }
}

fn message_text(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(text) = value.as_str() {
        return Some(text.to_owned());
    }
    let blocks = value.as_array()?;
    let text = blocks
        .iter()
        .filter_map(|block| {
            block
                .as_str()
                .map(str::to_owned)
                .or_else(|| block.get("text").and_then(Value::as_str).map(str::to_owned))
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}
