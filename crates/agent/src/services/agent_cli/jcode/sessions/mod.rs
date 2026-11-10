//! jcode native session history.
//!
//! A session is a JSON snapshot plus an append-only journal. The snapshot is a
//! single `messages` document; each journal line is `{meta, append_messages}`.
//! Both are read through the shared bounded readers.

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
        return Err("jcode 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("JCODE_HOME", root);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let root = config::config_dir().ok_or_else(|| "无法定位 jcode 配置目录".to_string())?;
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
    if let Err(error) = collect(
        &root.join("sessions"),
        budget,
        &mut result.diagnostics,
        &mut sessions,
    ) {
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
    // The locator names either the snapshot or the journal. Sources are
    // derived from that one name so the same file is never read twice: the
    // snapshot holds the baseline state and the journal replays appends on
    // top of it (duplicate message ids replace, they do not accumulate).
    let mut sources = Vec::new();
    if let Some(snapshot) = snapshot_path(&record.locator) {
        if snapshot.is_file() {
            sources.push(SessionTranscriptSource {
                path: snapshot,
                parser_version: INDEX_PARSER_VERSION,
                format: SessionTranscriptFormat::Messages,
                window_safe: true,
                sqlite: None,
                decode: decoder::decode,
            });
        }
    }
    let journal = journal_path(&record.locator);
    if journal.is_file() {
        sources.push(SessionTranscriptSource {
            path: journal,
            parser_version: INDEX_PARSER_VERSION,
            format: SessionTranscriptFormat::Lines,
            window_safe: true,
            sqlite: None,
            decode: decoder::decode,
        });
    }
    if sources.is_empty() {
        return Err("jcode 会话文件不存在".into());
    }
    Ok(sources)
}

struct Session {
    id: String,
    snapshot: Option<PathBuf>,
    journal: Option<PathBuf>,
}

fn collect(
    sessions: &Path,
    budget: &SessionReadBudget,
    diagnostics: &mut Vec<String>,
    out: &mut Vec<Session>,
) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(sessions) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("jcode 会话目录不可读：{error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("jcode 会话目录不是普通目录".into());
    }
    let mut by_id: std::collections::BTreeMap<String, Session> = std::collections::BTreeMap::new();
    for entry in fs::read_dir(sessions).map_err(|error| error.to_string())? {
        budget.check()?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(error.to_string());
                continue;
            }
        };
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let (id, journal) = if let Some(id) = name.strip_suffix(".journal.jsonl") {
            (id.to_owned(), true)
        } else if let Some(id) = name.strip_suffix(".json") {
            (id.to_owned(), false)
        } else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        if out.len() >= MAX_SESSION_FILES {
            diagnostics.push("jcode 会话文件数量超过上限，已跳过部分记录".into());
            return Ok(());
        }
        let session = by_id.entry(id.clone()).or_insert_with(|| Session {
            id,
            snapshot: None,
            journal: None,
        });
        if journal {
            session.journal = Some(path);
        } else {
            session.snapshot = Some(path);
        }
    }
    out.extend(by_id.into_values());
    Ok(())
}

fn snapshot_path(locator: &Path) -> Option<PathBuf> {
    let name = locator.file_name()?.to_str()?;
    if let Some(stem) = name.strip_suffix(".journal.jsonl") {
        return Some(locator.with_file_name(format!("{stem}.json")));
    }
    name.ends_with(".json").then(|| locator.to_path_buf())
}

fn journal_path(locator: &Path) -> PathBuf {
    let Some(name) = locator.file_name().and_then(|name| name.to_str()) else {
        return locator.to_path_buf();
    };
    if name.ends_with(".journal.jsonl") {
        return locator.to_path_buf();
    }
    match name.strip_suffix(".json") {
        Some(stem) => locator.with_file_name(format!("{stem}.journal.jsonl")),
        None => locator.to_path_buf(),
    }
}

fn locator(session: &Session) -> PathBuf {
    session
        .snapshot
        .clone()
        .or_else(|| session.journal.clone())
        .unwrap_or_default()
}

fn identity_record(
    kind: AgentCliKind,
    session: &Session,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let mut parsed = FileSummary::default();
    if let Some(journal) = &session.journal {
        let _ = read_json_lines_limited(journal, SUMMARY_BYTES, "读取 jcode 会话身份", |_, value| {
            parsed.observe_entry(&value)
        })?;
    }
    let workdir = parsed
        .working_dir
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| fallback_workdir.map(Path::to_path_buf))
        .unwrap_or_default();
    Ok(Some(SessionHistoryRecord::identity(
        kind,
        session.id.clone(),
        &workdir,
        locator(session),
    )))
}

fn summary_record(
    kind: AgentCliKind,
    session: &Session,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let mut parsed = FileSummary::default();
    if let Some(journal) = &session.journal {
        let _ = read_json_lines_limited(journal, SUMMARY_BYTES, "读取 jcode 会话摘要", |_, value| {
            parsed.observe_entry(&value)
        })?;
    }
    if !parsed.seen_record {
        if let Some(snapshot) = &session.snapshot {
            parsed.observe_snapshot(snapshot)?;
        }
    }
    if !parsed.seen_record {
        return Ok(None);
    }
    let workdir = parsed
        .working_dir
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| fallback_workdir.map(Path::to_path_buf))
        .unwrap_or_default();
    let title = first_non_empty([
        parsed.title.clone().and_then(|value| clean_text(value, 100)),
        parsed
            .first_user_text
            .clone()
            .and_then(|value| clean_text(value, 100)),
    ]);
    let preview = parsed
        .first_user_text
        .clone()
        .and_then(|value| clean_text(value, 240));
    let subagent = parsed.parent_id.is_some();
    Ok(Some(SessionHistoryRecord {
        summary: CliSessionSummary {
            id: session.id.clone(),
            title,
            preview,
            model: parsed.model.clone(),
            models: parsed.models.iter().cloned().collect(),
            cli_kind: kind,
            created_at: normalize_timestamp(parsed.created_at.as_deref()),
            updated_at: normalize_timestamp(parsed.updated_at.as_deref()),
            workdir: workdir.to_string_lossy().into_owned(),
            cli_version: None,
            archived: false,
            can_resume: true,
            metadata_source: "jcodeJournal".into(),
        },
        locator: locator(session),
        record_key: session.id.clone(),
        role: if subagent {
            AgentSessionRole::Subagent
        } else {
            AgentSessionRole::Main
        },
        parent_native_id: parsed.parent_id.clone(),
        resume_reason: None,
        content_unavailable_reason: None,
    }))
}

#[derive(Default)]
struct FileSummary {
    seen_record: bool,
    title: Option<String>,
    working_dir: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    parent_id: Option<String>,
    model: Option<String>,
    models: std::collections::BTreeSet<String>,
    first_user_text: Option<String>,
    has_messages: bool,
}

impl FileSummary {
    fn observe_entry(&mut self, value: &Value) {
        let Some(meta) = value.get("meta") else {
            return;
        };
        self.seen_record = true;
        let updated = meta
            .get("updated_at")
            .and_then(Value::as_str)
            .and_then(|value| normalize_timestamp(Some(value)));
        if let Some(updated) = &updated {
            if self.created_at.is_none() {
                self.created_at = Some(updated.clone());
            }
            self.updated_at = Some(updated.clone());
        }
        if let Some(title) = meta
            .get("custom_title")
            .or_else(|| meta.get("title"))
            .and_then(Value::as_str)
            .and_then(|value| clean_text(value, 100))
        {
            self.title = Some(title);
        }
        if let Some(working_dir) = meta.get("working_dir").and_then(Value::as_str) {
            self.working_dir = Some(working_dir.to_owned());
        }
        if let Some(parent) = meta.get("parent_id").and_then(Value::as_str) {
            self.parent_id = Some(parent.to_owned());
        }
        if let Some(model) = meta.get("model").and_then(Value::as_str) {
            self.record_model(model);
        }
        if let Some(messages) = value.get("append_messages").and_then(Value::as_array) {
            self.has_messages |= !messages.is_empty();
            for message in messages {
                self.observe_message(message);
            }
        }
    }

    fn observe_snapshot(&mut self, path: &Path) -> Result<(), String> {
        let header = read_session_header(
            path,
            &["id", "title", "custom_title", "working_dir", "model", "parent_id"],
            |fields, _| fields.get("id").is_some() && fields.get("working_dir").is_some(),
        )
        .map_err(|error| error.to_string())?;
        let Some(header) = header else {
            return Ok(());
        };
        self.seen_record = true;
        self.title = header
            .get("custom_title")
            .or_else(|| header.get("title"))
            .and_then(Value::as_str)
            .and_then(|value| clean_text(value, 100));
        self.working_dir = header
            .get("working_dir")
            .and_then(Value::as_str)
            .map(str::to_owned);
        self.parent_id = header
            .get("parent_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if let Some(model) = header.get("model").and_then(Value::as_str) {
            self.record_model(model);
        }
        Ok(())
    }

    fn observe_message(&mut self, message: &Value) {
        match message.get("role").and_then(Value::as_str) {
            Some("User") | Some("user") => {
                if self.first_user_text.is_none() {
                    self.first_user_text = stored_content_text(message.get("content"));
                }
                self.has_messages = true;
            }
            Some("Assistant") | Some("assistant") => {
                self.has_messages = true;
            }
            _ => {}
        }
    }

    fn record_model(&mut self, model: &str) {
        let model = model.trim();
        if model.is_empty() {
            return;
        }
        self.models.insert(model.to_owned());
        self.model = Some(model.to_owned());
    }
}

fn stored_content_text(value: Option<&Value>) -> Option<String> {
    let blocks = value?.as_array()?;
    let text = blocks
        .iter()
        .filter_map(|block| {
            block
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| block.pointer("/Text/text").and_then(Value::as_str))
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}
