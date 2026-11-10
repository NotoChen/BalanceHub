//! GitHub Copilot CLI native session history.
//!
//! Copilot keeps one directory per session below `<COPILOT_HOME>/session-state`.
//! `workspace.yaml` holds display metadata while `events.jsonl` is the
//! append-only event journal. Rewound sessions can link a standalone head to a
//! numbered predecessor journal through `session.journal_link`; `history`
//! reads that chain oldest record first.

mod decoder;

use super::config;
use crate::{
    models::{AgentCliKind, AgentSessionRole, CliSessionSummary},
    services::{
        agent_cli::contracts::{
            EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
            SessionHistoryScan, SessionHistorySource, SessionReadBudget, SessionTranscriptFormat,
            SessionTranscriptSource,
        },
        cli_sessions::{
            clean_text, first_non_empty, normalize_timestamp, read_json_lines_limited,
            workbench::read_session_text_file_limited,
        },
    },
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

const PARSER_VERSION: u32 = 1;
const METADATA_SOURCE: &str = "copilotSessionState";
const MAX_SESSION_DIRS: usize = 100_000;
const MAX_WORKSPACE_BYTES: usize = 256 * 1024;
const MAX_EVENT_SCAN_BYTES: usize = 1024 * 1024;
const MAX_JOURNAL_HEAD_BYTES: usize = 64 * 1024;
const MAX_JOURNAL_DEPTH: usize = 32;
const EVENTS_FILE: &str = "events.jsonl";
const WORKSPACE_FILE: &str = "workspace.yaml";

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("Copilot CLI 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("COPILOT_HOME", root);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let root = config::config_root().ok_or_else(|| "无法定位 Copilot CLI 配置目录".to_string())?;
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
    let sessions = root.join("session-state");
    let entries = match fs::read_dir(&sessions) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(error) => return Err(format!("Copilot CLI 会话目录不可读：{error}")),
    };
    let fallback_workdir = (workdirs.len() == 1).then(|| workdirs[0].as_path());
    let mut seen = 0usize;
    for entry in entries {
        if let Err(error) = budget.check() {
            result.failed(error);
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        seen = seen.saturating_add(1);
        if seen > MAX_SESSION_DIRS {
            result.failed("Copilot CLI 会话目录数量超过读取上限");
            break;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let directory = entry.path();
        let workspace_path = directory.join(WORKSPACE_FILE);
        match fs::symlink_metadata(&workspace_path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                result.failed(format!("Copilot CLI 会话元数据不可读：{error}"));
                continue;
            }
        }
        let record = match mode {
            SessionHistoryReadMode::Identities => {
                identity_record(kind, &directory, fallback_workdir)
            }
            SessionHistoryReadMode::Summaries => {
                summary_record(kind, &directory, fallback_workdir)
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
    let mut chain = vec![record.locator.clone()];
    let mut current = record.locator.clone();
    for _ in 0..MAX_JOURNAL_DEPTH {
        let Some(previous) = journal_predecessor(&current) else {
            break;
        };
        if chain.contains(&previous) {
            break;
        }
        chain.push(previous.clone());
        current = previous;
    }
    chain.reverse();
    Ok(chain
        .into_iter()
        .map(|path| SessionTranscriptSource {
            path,
            parser_version: PARSER_VERSION,
            format: SessionTranscriptFormat::Lines,
            window_safe: true,
            sqlite: None,
            decode: decoder::decode,
        })
        .collect())
}

struct WorkspaceMeta {
    id: String,
    cwd: Option<PathBuf>,
    name: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

fn read_workspace(directory: &Path) -> Result<WorkspaceMeta, String> {
    let path = directory.join(WORKSPACE_FILE);
    let text = read_session_text_file_limited(&path, MAX_WORKSPACE_BYTES, "读取 Copilot CLI 会话元数据")
        .map_err(|error| format!("{error}（{}）", directory.display()))?;
    let value = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text)
        .map_err(|error| format!("Copilot CLI 会话元数据格式无效：{error}"))?;
    let mapping = value
        .as_mapping()
        .ok_or_else(|| "Copilot CLI 会话元数据不是映射".to_string())?;
    let field = |name: &str| {
        mapping
            .get(serde_yaml_ng::Value::String(name.to_owned()))
            .and_then(serde_yaml_ng::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let id = field("id")
        .or_else(|| {
            directory
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .filter(|name| !name.is_empty())
        })
        .ok_or_else(|| "Copilot CLI 会话元数据缺少会话标识".to_string())?;
    Ok(WorkspaceMeta {
        id,
        cwd: field("cwd")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute()),
        name: field("name"),
        created_at: field("created_at").and_then(|value| normalize_timestamp(Some(&value))),
        updated_at: field("updated_at").and_then(|value| normalize_timestamp(Some(&value))),
    })
}

fn identity_record(
    kind: AgentCliKind,
    directory: &Path,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let meta = read_workspace(directory)?;
    let id = meta.id.clone();
    Ok(Some(SessionHistoryRecord::identity(
        kind,
        id,
        &workdir(&meta, fallback_workdir),
        directory.join(EVENTS_FILE),
    )))
}

fn summary_record(
    kind: AgentCliKind,
    directory: &Path,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let meta = read_workspace(directory)?;
    let events_path = directory.join(EVENTS_FILE);
    let (facts, unavailable) = match fs::symlink_metadata(&events_path) {
        Ok(metadata) if metadata.is_file() => {
            match event_facts(&events_path) {
                Ok(facts) => (facts, None),
                Err(error) => (EventFacts::default(), Some(error)),
            }
        }
        Ok(_) => (
            EventFacts::default(),
            Some("Copilot CLI 会话事件文件不是普通文件".to_string()),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
            EventFacts::default(),
            Some("Copilot CLI 尚未写入会话事件".to_string()),
        ),
        Err(error) => (
            EventFacts::default(),
            Some(format!("Copilot CLI 会话事件不可读：{error}")),
        ),
    };
    let id = meta.id.clone();
    let preview = facts.preview.clone();
    let title = first_non_empty([
        meta.name.clone().and_then(|value| clean_text(value, 100)),
        preview.clone(),
        workdir(&meta, fallback_workdir)
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .and_then(|value| clean_text(value, 100)),
    ]);
    let created_at = meta.created_at.clone();
    let updated_at = later_timestamp(meta.updated_at.clone(), facts.last_at.clone());
    let parent_native_id = facts.parent.clone();
    Ok(Some(SessionHistoryRecord {
        summary: CliSessionSummary {
            id: id.clone(),
            title,
            preview,
            model: facts.model.clone(),
            models: facts.models.into_iter().collect(),
            cli_kind: kind,
            created_at,
            updated_at,
            workdir: workdir(&meta, fallback_workdir)
                .to_string_lossy()
                .into_owned(),
            cli_version: facts.version,
            archived: false,
            can_resume: true,
            metadata_source: METADATA_SOURCE.into(),
        },
        locator: events_path,
        record_key: id,
        role: AgentSessionRole::Main,
        parent_native_id,
        resume_reason: None,
        content_unavailable_reason: unavailable,
    }))
}

fn workdir(meta: &WorkspaceMeta, fallback: Option<&Path>) -> PathBuf {
    meta.cwd
        .clone()
        .or_else(|| fallback.map(Path::to_path_buf))
        .unwrap_or_default()
}

#[derive(Default)]
struct EventFacts {
    preview: Option<String>,
    models: BTreeSet<String>,
    model: Option<String>,
    version: Option<String>,
    parent: Option<String>,
    last_at: Option<String>,
}

fn event_facts(path: &Path) -> Result<EventFacts, String> {
    let mut facts = EventFacts::default();
    let mut latest_millis = i64::MIN;
    let mut latest_model = String::new();
    read_json_lines_limited(
        path,
        MAX_EVENT_SCAN_BYTES,
        "读取 Copilot CLI 会话事件失败",
        |_, value| {
            let timestamp = value
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(|value| normalize_timestamp(Some(value)));
            let millis = timestamp
                .as_deref()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .map(|value| value.timestamp_millis());
            if let (Some(timestamp), Some(millis)) = (timestamp.as_ref(), millis) {
                let replace = facts
                    .last_at
                    .as_deref()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .is_none_or(|previous| millis >= previous.timestamp_millis());
                if replace {
                    facts.last_at = Some(timestamp.clone());
                }
            }
            let note_model = |models: &mut BTreeSet<String>,
                                  latest_millis: &mut i64,
                                  latest_model: &mut String,
                                  model: Option<&str>| {
                let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) else {
                    return;
                };
                models.insert(model.to_owned());
                let order = millis.unwrap_or(i64::MIN);
                if order >= *latest_millis {
                    *latest_millis = order;
                    *latest_model = model.to_owned();
                }
            };
            match value.get("type").and_then(Value::as_str) {
                Some("user.message") if facts.preview.is_none() => {
                    facts.preview = value
                        .pointer("/data/content")
                        .and_then(Value::as_str)
                        .and_then(|value| clean_text(value, 240));
                }
                Some("assistant.message") => {
                    note_model(
                        &mut facts.models,
                        &mut latest_millis,
                        &mut latest_model,
                        value.pointer("/data/model").and_then(Value::as_str),
                    );
                }
                Some("session.model_change") => {
                    note_model(
                        &mut facts.models,
                        &mut latest_millis,
                        &mut latest_model,
                        value.pointer("/data/newModel").and_then(Value::as_str),
                    );
                }
                Some("session.start") => {
                    facts.version = value
                        .pointer("/data/copilotVersion")
                        .and_then(Value::as_str)
                        .and_then(|value| clean_text(value, 80));
                    facts.parent = value
                        .pointer("/data/detachedFromSpawningParentSessionId")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    note_model(
                        &mut facts.models,
                        &mut latest_millis,
                        &mut latest_model,
                        value.pointer("/data/selectedModel").and_then(Value::as_str),
                    );
                }
                Some("session.resume") => {
                    note_model(
                        &mut facts.models,
                        &mut latest_millis,
                        &mut latest_model,
                        value.pointer("/data/selectedModel").and_then(Value::as_str),
                    );
                }
                _ => {}
            }
        },
    )?;
    if !latest_model.is_empty() {
        facts.model = Some(latest_model);
    }
    Ok(facts)
}

fn later_timestamp(current: Option<String>, candidate: Option<String>) -> Option<String> {
    let millis = |value: &str| {
        chrono::DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|value| value.timestamp_millis())
    };
    match (current, candidate) {
        (Some(current), Some(candidate)) => {
            match (millis(&current), millis(&candidate)) {
                (Some(left), Some(right)) if right > left => Some(candidate),
                _ => Some(current),
            }
        }
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn journal_predecessor(path: &Path) -> Option<PathBuf> {
    let mut previous: Option<PathBuf> = None;
    let _ = read_json_lines_limited(
        path,
        MAX_JOURNAL_HEAD_BYTES,
        "读取 Copilot CLI 会话日志失败",
        |_, value| {
            if previous.is_some()
                || value.get("type").and_then(Value::as_str) != Some("session.journal_link")
            {
                return;
            }
            let Some(name) = value
                .pointer("/data/previousFile")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
            else {
                return;
            };
            let mut components = Path::new(name).components();
            let (Some(Component::Normal(_)), None) = (components.next(), components.next()) else {
                return;
            };
            let Some(parent) = path.parent() else {
                return;
            };
            let candidate = parent.join(name);
            match fs::symlink_metadata(&candidate) {
                Ok(metadata) if metadata.is_file() => previous = Some(candidate),
                _ => {}
            }
        },
    );
    previous
}
