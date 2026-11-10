//! Mister Morph native session history.
//!
//! Mister Morph records every Console/local-chat turn in an append-only task
//! journal at `<state>/journal/journal.db`. `journal_events` holds raw-deflate
//! compressed JSON payloads; task snapshots carry both the task and its topic,
//! so the latest snapshot per task is enough to rebuild every conversation.
//! A topic is one session; its tasks are the user/assistant turns.

mod decoder;

#[cfg(test)]
mod tests;

use super::config;
use crate::{
    models::{AgentCliKind, CliSessionSummary},
    services::{
        agent_cli::contracts::{
            EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
            SessionHistoryScan, SessionHistorySource, SessionReadBudget, SessionTranscriptFormat,
            SessionTranscriptSource, SqliteTranscript,
        },
        cli_sessions::{clean_text, first_non_empty, normalize_timestamp, workbench},
    },
};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    time::Duration,
};

const PARSER_VERSION: u32 = 1;
const METADATA_SOURCE: &str = "morphTaskJournal";
const RESUME_REASON: &str = "Mister Morph 暂未接入临时启动续聊";
const MAX_EVENTS: usize = 200_000;
const MAX_CONFIG_BYTES: usize = 256 * 1024;
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);

const LATEST_TASKS_QUERY: &str = "SELECT e.seq, hex(e.payload) AS payload \
    FROM journal_events e \
    JOIN (SELECT task_id, MAX(seq) AS seq FROM journal_events \
          WHERE domain = 'task' AND type IN ('task_upsert', 'task_update') \
            AND task_id <> '' GROUP BY task_id) latest ON latest.seq = e.seq \
    ORDER BY e.seq";

const LATEST_TOPICS_QUERY: &str = "SELECT e.type, hex(e.payload) AS payload \
    FROM journal_events e \
    JOIN (SELECT topic_id, MAX(seq) AS seq FROM journal_events \
          WHERE domain = 'task' \
            AND type IN ('topic_upsert', 'topic_title_updated', 'topic_tags_updated', 'topic_deleted') \
            AND topic_id <> '' GROUP BY topic_id) latest ON latest.seq = e.seq \
    ORDER BY e.seq";

const TRANSCRIPT_QUERY: &str = "SELECT seq AS row_key, hex(payload) AS payload \
    FROM journal_events \
    WHERE domain = 'task' AND type IN ('task_upsert', 'task_update') \
      AND topic_id = ?3 AND seq > ?1 ORDER BY seq LIMIT ?2";

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("Mister Morph 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("MISTER_MORPH_CONFIG", &root.join("config.yaml"));
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let home = crate::services::cli_paths::user_home()
        .ok_or_else(|| "无法定位用户目录".to_string())?;
    let config_path = config::config_path_for_home(&home);
    let root = state_root(&config_path, &home);
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

/// `file_state_dir` in the native config moves the journal; empty keeps
/// `~/.morph`. A relative value would resolve against the CLI's own cwd and
/// cannot be followed from here.
fn state_root(config_path: &Path, home: &Path) -> PathBuf {
    let fallback = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".morph"));
    let Ok(text) =
        workbench::read_session_text_file_limited(config_path, MAX_CONFIG_BYTES, "读取 Mister Morph 配置")
    else {
        return fallback;
    };
    let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text) else {
        return fallback;
    };
    let Some(state_dir) = value
        .as_mapping()
        .and_then(|mapping| mapping.get(serde_yaml_ng::Value::String("file_state_dir".into())))
        .and_then(serde_yaml_ng::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return fallback;
    };
    super::super::integration::expand_tilde(state_dir, home)
}

fn journal_path(root: &Path) -> PathBuf {
    root.join("journal/journal.db")
}

fn scan(
    kind: AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    let mut result = SessionHistoryScan::new(&[]);
    let path = journal_path(root);
    if !path.is_file() {
        return Ok(result);
    }
    if let Err(error) = scan_journal(kind, &path, workdirs, budget, mode, &mut result) {
        result.failed(error);
    }
    Ok(result)
}

fn transcript(record: &SessionHistoryRecord) -> Result<Vec<SessionTranscriptSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    if !record.locator.is_file() {
        return Err("Mister Morph 任务日志不存在".into());
    }
    Ok(vec![SessionTranscriptSource {
        path: record.locator.clone(),
        parser_version: PARSER_VERSION,
        format: SessionTranscriptFormat::Lines,
        window_safe: false,
        sqlite: Some(SqliteTranscript {
            query: TRANSCRIPT_QUERY,
            scope: record.record_key.clone(),
        }),
        decode: decoder::decode,
    }])
}

#[derive(Default)]
struct TopicFacts {
    title: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    deleted: bool,
    preview: Option<(i64, String)>,
    latest_model: Option<(i64, String)>,
    models: BTreeSet<String>,
}

fn scan_journal(
    kind: AgentCliKind,
    path: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
    result: &mut SessionHistoryScan,
) -> Result<(), String> {
    let connection = open(path)?;
    if !table_exists(&connection, "journal_events") {
        return Err("Mister Morph 任务日志缺少 journal_events 表".into());
    }
    let mut topics: HashMap<String, TopicFacts> = HashMap::new();
    read_latest_tasks(&connection, budget, &mut topics)?;
    read_latest_topics(&connection, budget, &mut topics)?;
    let fallback_workdir = (workdirs.len() == 1).then(|| workdirs[0].as_path());
    let workdir = fallback_workdir.map(Path::to_path_buf).unwrap_or_default();
    let mut entries = topics.into_iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        timestamp_order(&right.1.updated_at)
            .cmp(&timestamp_order(&left.1.updated_at))
            .then_with(|| left.0.cmp(&right.0))
    });
    for (id, facts) in entries {
        budget.check()?;
        if facts.deleted || internal_topic(&id) || facts.preview.is_none() {
            continue;
        }
        let preview = facts
            .preview
            .as_ref()
            .map(|(_, text)| text.clone())
            .and_then(|value| clean_text(value, 240));
        let model = facts.latest_model.map(|(_, model)| model);
        let record = match mode {
            SessionHistoryReadMode::Identities => {
                SessionHistoryRecord::identity(kind, id.clone(), &workdir, path.to_path_buf())
            }
            SessionHistoryReadMode::Summaries => SessionHistoryRecord {
                summary: CliSessionSummary {
                    id: id.clone(),
                    title: first_non_empty([
                        facts.title.and_then(|value| clean_text(value, 100)),
                        preview.clone().and_then(|value| clean_text(value, 100)),
                    ]),
                    preview,
                    model: model.clone(),
                    models: model.into_iter().collect::<BTreeSet<_>>().into_iter().collect(),
                    cli_kind: kind,
                    created_at: facts.created_at,
                    updated_at: facts.updated_at,
                    workdir: workdir.to_string_lossy().into_owned(),
                    cli_version: None,
                    archived: false,
                    can_resume: false,
                    metadata_source: METADATA_SOURCE.into(),
                },
                locator: path.to_path_buf(),
                record_key: id,
                role: crate::models::AgentSessionRole::Main,
                parent_native_id: None,
                resume_reason: Some(RESUME_REASON.into()),
                content_unavailable_reason: None,
            },
        };
        result.workspace(&workdir).records.push(record);
    }
    Ok(())
}

fn read_latest_tasks(
    connection: &Connection,
    budget: &SessionReadBudget,
    topics: &mut HashMap<String, TopicFacts>,
) -> Result<(), String> {
    let mut statement = connection
        .prepare(LATEST_TASKS_QUERY)
        .map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let mut seen = 0usize;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        budget.check()?;
        seen = seen.saturating_add(1);
        if seen > MAX_EVENTS {
            return Err("Mister Morph 任务数量超过读取上限".into());
        }
        let Ok(payload) = row.get::<_, String>(1) else {
            continue;
        };
        let Some(payload) = decoder::payload_hex(&payload) else {
            continue;
        };
        note_task(&payload, topics);
    }
    Ok(())
}

fn note_task(payload: &Value, topics: &mut HashMap<String, TopicFacts>) {
    let Some(task) = payload.get("task") else {
        return;
    };
    let Some(id) = task
        .get("topic_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let facts = topics.entry(id.to_owned()).or_default();
    if let Some(topic) = payload.get("topic") {
        if let Some(title) = topic.get("title").and_then(Value::as_str) {
            let title = title.trim();
            if !title.is_empty() {
                facts.title = Some(title.to_owned());
            }
        }
        facts.created_at =
            first_timestamp(facts.created_at.clone(), entry_timestamp(topic.get("created_at")));
        facts.updated_at =
            later_timestamp(facts.updated_at.clone(), entry_timestamp(topic.get("updated_at")));
    }
    let at = timestamp_millis(task.get("created_at")).unwrap_or(i64::MIN);
    if let Some(prompt) = task
        .get("task")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let replace = facts.preview.as_ref().is_none_or(|(current, _)| at < *current);
        if replace {
            facts.preview = Some((at, prompt.to_owned()));
        }
    }
    if let Some(model) = task
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        facts.models.insert(model.to_owned());
        if facts
            .latest_model
            .as_ref()
            .is_none_or(|(current, _)| at >= *current)
        {
            facts.latest_model = Some((at, model.to_owned()));
        }
    }
}

fn read_latest_topics(
    connection: &Connection,
    budget: &SessionReadBudget,
    topics: &mut HashMap<String, TopicFacts>,
) -> Result<(), String> {
    let mut statement = connection
        .prepare(LATEST_TOPICS_QUERY)
        .map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let mut seen = 0usize;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        budget.check()?;
        seen = seen.saturating_add(1);
        if seen > MAX_EVENTS {
            return Err("Mister Morph 主题数量超过读取上限".into());
        }
        let Ok(kind) = row.get::<_, String>(0) else {
            continue;
        };
        let Ok(payload) = row.get::<_, String>(1) else {
            continue;
        };
        let Some(payload) = decoder::payload_hex(&payload) else {
            continue;
        };
        let Some(topic) = payload.get("topic") else {
            continue;
        };
        let Some(id) = topic
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let facts = topics.entry(id.to_owned()).or_default();
        if kind == "topic_deleted" {
            facts.deleted = true;
            continue;
        }
        if let Some(title) = topic.get("title").and_then(Value::as_str) {
            let title = title.trim();
            if !title.is_empty() {
                facts.title = Some(title.to_owned());
            }
        }
        facts.created_at = first_timestamp(facts.created_at.clone(), entry_timestamp(topic.get("created_at")));
        facts.updated_at = later_timestamp(facts.updated_at.clone(), entry_timestamp(topic.get("updated_at")));
    }
    Ok(())
}

fn internal_topic(id: &str) -> bool {
    id == "_awareness"
        || id == "_heartbeat"
        || id.starts_with("_agentlink_")
        || id.starts_with("_cron_")
}

fn timestamp_millis(value: Option<&Value>) -> Option<i64> {
    let raw = value?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|value| value.timestamp_millis())
}

fn entry_timestamp(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .and_then(|value| normalize_timestamp(Some(value)))
}

fn timestamp_order(value: &Option<String>) -> i64 {
    value
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis())
        .unwrap_or(i64::MIN)
}

fn first_timestamp(current: Option<String>, candidate: Option<String>) -> Option<String> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => {
            if timestamp_order(&Some(candidate.clone())) < timestamp_order(&Some(current.clone())) {
                Some(candidate)
            } else {
                Some(current)
            }
        }
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn later_timestamp(current: Option<String>, candidate: Option<String>) -> Option<String> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => {
            if timestamp_order(&Some(candidate.clone())) > timestamp_order(&Some(current.clone())) {
                Some(candidate)
            } else {
                Some(current)
            }
        }
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn open(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("打开 Mister Morph 任务日志失败：{error}"))?;
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

fn table_exists(connection: &Connection, table: &str) -> bool {
    connection
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?")
        .ok()
        .and_then(|mut statement| {
            statement
                .query_row([table], |row| row.get::<_, i64>(0))
                .ok()
        })
        .is_some()
}
