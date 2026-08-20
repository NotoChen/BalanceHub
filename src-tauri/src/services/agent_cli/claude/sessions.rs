use crate::{
    models::{
        AgentCliKind, CliSessionDetail, CliSessionMessageRole, CliSessionSummary,
    },
    services::cli_sessions::{
        clean_text, compact_json, first_non_empty, normalize_timestamp,
        read_json_lines_limited, scan_json_lines_matching, scan_json_records_background,
        session_index_source_fingerprint, SessionContentSearchCollector, SessionMessageCollector,
    },
};
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File},
    io::{BufRead, BufReader},
    path::Path,
};

use super::super::contracts::{
    SessionContentSearchRequest, SessionContentSearchResult, SessionIndexLoadResult,
    SessionIndexMessage, SessionReadLimits,
};

const INDEX_PARSER_VERSION: u32 = 1;

pub(super) fn list(
    cli_kind: AgentCliKind,
    workdir: &Path,
) -> Result<Vec<CliSessionSummary>, String> {
    let projects = super::config::config_dir()
        .map(|config_dir| config_dir.join("projects"))
        .ok_or_else(|| "无法定位用户目录，无法读取 Claude Code 历史会话".to_string())?;
    let encoded = encode_project_path(workdir);
    let project_dir = projects.join(encoded);
    if !project_dir.is_dir() {
        return Ok(Vec::new());
    }

    // Claude stores main transcripts directly in this directory and keeps
    // sub-agent transcripts below each session's `subagents/` directory.
    // Only the direct files are resumable targets for the selected session.
    let files = fs::read_dir(&project_dir)
        .map_err(|err| {
            format!(
                "读取 Claude Code 项目会话目录失败：{}：{err}",
                project_dir.display()
            )
        })?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect::<Vec<_>>();

    let mut sessions = Vec::new();
    let mut last_error = None;
    let mut failed_files = 0;
    for path in files.iter() {
        match parse_transcript(cli_kind, path, workdir) {
            Ok(Some(session)) if session.can_resume => sessions.push(session),
            Ok(_) => {}
            Err(error) => {
                failed_files += 1;
                last_error = Some(error);
            }
        }
    }
    if !files.is_empty() && sessions.is_empty() && failed_files == files.len() {
        return Err(last_error.unwrap_or_else(|| "读取 Claude Code 历史会话失败".to_string()));
    }
    Ok(sessions)
}

pub(super) fn detail(
    cli_kind: AgentCliKind,
    workdir: &Path,
    session_id: &str,
    limits: SessionReadLimits,
) -> Result<CliSessionDetail, String> {
    let projects = super::config::config_dir()
        .map(|config_dir| config_dir.join("projects"))
        .ok_or_else(|| "无法定位用户目录，无法读取 Claude Code 历史会话".to_string())?;
    let project_dir = projects.join(encode_project_path(workdir));
    let path = find_transcript_path(cli_kind, &project_dir, workdir, session_id)?
        .ok_or_else(|| "未找到指定的 Claude Code 会话".to_string())?;
    let summary = parse_transcript(cli_kind, &path, workdir)?
        .filter(|summary| summary.id == session_id)
        .ok_or_else(|| "Claude Code 会话索引与正文文件不一致".to_string())?;
    let (messages, truncated, omitted_message_count) =
        parse_transcript_messages(&path, limits)?;
    Ok(CliSessionDetail {
        session: summary,
        messages,
        truncated,
        omitted_message_count,
        content_source: "claudeTranscript".to_string(),
    })
}

pub(super) fn search(
    cli_kind: AgentCliKind,
    workdir: &Path,
    session_id: &str,
    request: &SessionContentSearchRequest,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String> {
    let projects = super::config::config_dir()
        .map(|config_dir| config_dir.join("projects"))
        .ok_or_else(|| "无法定位用户目录，无法读取 Claude Code 历史会话".to_string())?;
    let project_dir = projects.join(encode_project_path(workdir));
    let path = find_transcript_path(cli_kind, &project_dir, workdir, session_id)?
        .ok_or_else(|| "未找到指定的 Claude Code 会话".to_string())?;
    search_transcript(&path, request, is_current)
}

pub(super) fn index(
    cli_kind: AgentCliKind,
    workdir: &Path,
    session_id: &str,
    known_fingerprint: Option<&str>,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionIndexLoadResult, String> {
    let projects = super::config::config_dir()
        .map(|config_dir| config_dir.join("projects"))
        .ok_or_else(|| "无法定位用户目录，无法读取 Claude Code 历史会话".to_string())?;
    let project_dir = projects.join(encode_project_path(workdir));
    let path = find_transcript_path(cli_kind, &project_dir, workdir, session_id)?
        .ok_or_else(|| "未找到指定的 Claude Code 会话".to_string())?;
    index_transcript(&path, known_fingerprint, is_current)
}

fn index_transcript(
    path: &Path,
    known_fingerprint: Option<&str>,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionIndexLoadResult, String> {
    let (fingerprint, source_bytes) =
        session_index_source_fingerprint(path, INDEX_PARSER_VERSION)?;
    if known_fingerprint == Some(fingerprint.as_str()) {
        return Ok(SessionIndexLoadResult::Unchanged {
            fingerprint,
            source_bytes,
        });
    }

    let mut messages = Vec::new();
    scan_json_records_background(path, "索引 Claude Code 会话正文", is_current, |line_index, line| {
        if !(line.windows(4).any(|window| window.eq_ignore_ascii_case(b"user"))
            || line
                .windows(9)
                .any(|window| window.eq_ignore_ascii_case(b"assistant")))
        {
            return false;
        }
        if line.windows(11).any(|window| window == b"tool_result")
            && !line.windows(6).any(|window| window == b"\"text\"")
        {
            return false;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return false;
        };
        if value
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || value
                .get("isMeta")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            return false;
        }
        let role = match value.get("type").and_then(Value::as_str) {
            Some("user") => CliSessionMessageRole::User,
            Some("assistant") => CliSessionMessageRole::Assistant,
            _ => return false,
        };
        let Some(content) = value
            .get("message")
            .and_then(|message| message.get("content"))
        else {
            return false;
        };
        if let Some(text) = content.as_str() {
            let text = text.trim();
            if !text.is_empty()
                && (role != CliSessionMessageRole::User || visible_user_text(text))
            {
                messages.push(SessionIndexMessage {
                    id: format!("claude-{line_index}"),
                    role,
                    content: text.to_string(),
                });
            }
            return false;
        }
        if let Some(parts) = content.as_array() {
            for (part_index, part) in parts.iter().enumerate() {
                if !matches!(part.get("type").and_then(Value::as_str), Some("text") | None) {
                    continue;
                }
                let Some(text) = part
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                else {
                    continue;
                };
                if role == CliSessionMessageRole::User && !visible_user_text(text) {
                    continue;
                }
                messages.push(SessionIndexMessage {
                    id: format!("claude-{line_index}-{part_index}"),
                    role,
                    content: text.to_string(),
                });
            }
        }
        false
    })?;
    Ok(SessionIndexLoadResult::Updated {
        fingerprint,
        source_bytes,
        messages,
    })
}

fn search_transcript(
    path: &Path,
    request: &SessionContentSearchRequest,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String> {
    let mut collector = SessionContentSearchCollector::new(request);
    scan_json_lines_matching(
        path,
        "检索 Claude Code 会话正文",
        request,
        is_current,
        |_line_index, value| {
            if value
                .get("isSidechain")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || value
                    .get("isMeta")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            {
                return false;
            }
            let role = match value.get("type").and_then(Value::as_str) {
                Some("user") => CliSessionMessageRole::User,
                Some("assistant") => CliSessionMessageRole::Assistant,
                _ => return false,
            };
            let Some(content) = value
                .get("message")
                .and_then(|message| message.get("content"))
            else {
                return false;
            };
            if let Some(text) = content.as_str() {
                if role != CliSessionMessageRole::User || visible_user_text(text) {
                    collector.observe(text);
                }
                return collector.complete();
            }
            if let Some(parts) = content.as_array() {
                for part in parts {
                    let text = match part.get("type").and_then(Value::as_str) {
                        Some("text") | None => {
                            let Some(text) = part.get("text").and_then(Value::as_str) else {
                                continue;
                            };
                            if role == CliSessionMessageRole::User && !visible_user_text(text) {
                                continue;
                            }
                            text.to_string()
                        }
                        _ => continue,
                    };
                    collector.observe(&text);
                    if collector.complete() {
                        break;
                    }
                }
            }
            collector.complete()
        },
    )?;
    Ok(collector.finish())
}

fn find_transcript_path(
    cli_kind: AgentCliKind,
    project_dir: &Path,
    workdir: &Path,
    session_id: &str,
) -> Result<Option<std::path::PathBuf>, String> {
    if !session_id
        .chars()
        .any(|character| matches!(character, '/' | '\\'))
    {
        let direct = project_dir.join(format!("{session_id}.jsonl"));
        if direct.is_file() {
            return Ok(Some(direct));
        }
    }
    let entries = match fs::read_dir(project_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "读取 Claude Code 项目会话目录失败：{}：{error}",
                project_dir.display()
            ));
        }
    };
    for path in entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "jsonl"))
    {
        if parse_transcript(cli_kind, &path, workdir)?
            .is_some_and(|summary| summary.id == session_id)
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn parse_transcript_messages(
    path: &Path,
    limits: SessionReadLimits,
) -> Result<(Vec<crate::models::CliSessionMessage>, bool, usize), String> {
    let mut collector = SessionMessageCollector::new(limits);
    let mut tool_names = HashMap::<String, String>::new();
    let source_truncated = read_json_lines_limited(
        path,
        limits.max_file_bytes,
        "读取 Claude Code 会话正文",
        |line_index, value| {
            if value
                .get("isSidechain")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || value
                    .get("isMeta")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            {
                return;
            }
            let record_type = value.get("type").and_then(Value::as_str);
            let role = match record_type {
                Some("user") => CliSessionMessageRole::User,
                Some("assistant") => CliSessionMessageRole::Assistant,
                _ => return,
            };
            let timestamp = normalize_timestamp(value.get("timestamp").and_then(Value::as_str));
            let model = value
                .get("message")
                .and_then(|message| message.get("model"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|model| model != "<synthetic>");
            let Some(content) = value
                .get("message")
                .and_then(|message| message.get("content"))
            else {
                return;
            };
            if let Some(text) = content.as_str() {
                if role != CliSessionMessageRole::User || visible_user_text(text) {
                    collector.push(
                        format!("claude-{line_index}"),
                        role,
                        text,
                        timestamp,
                        model,
                        None,
                    );
                }
                return;
            }
            let Some(parts) = content.as_array() else {
                return;
            };
            for (part_index, part) in parts.iter().enumerate() {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") | None => {
                        let Some(text) = part.get("text").and_then(Value::as_str) else {
                            continue;
                        };
                        if role == CliSessionMessageRole::User && !visible_user_text(text) {
                            continue;
                        }
                        collector.push(
                            format!("claude-{line_index}-{part_index}"),
                            role,
                            text,
                            timestamp.clone(),
                            model.clone(),
                            None,
                        );
                    }
                    Some("tool_use") => {
                        let name = part
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("工具")
                            .to_string();
                        if let Some(id) = part.get("id").and_then(Value::as_str) {
                            tool_names.insert(id.to_string(), name.clone());
                        }
                        let input = part
                            .get("input")
                            .map(|value| compact_json(value, limits.max_message_chars))
                            .unwrap_or_default();
                        collector.push(
                            format!("claude-{line_index}-{part_index}"),
                            CliSessionMessageRole::Tool,
                            if input.is_empty() {
                                format!("调用工具 {name}")
                            } else {
                                format!("调用工具 {name}\n{input}")
                            },
                            timestamp.clone(),
                            model.clone(),
                            Some(name),
                        );
                    }
                    Some("tool_result") => {
                        let tool_id = part
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let tool_name = tool_names
                            .get(tool_id)
                            .cloned()
                            .unwrap_or_else(|| "工具结果".to_string());
                        let result = part
                            .get("content")
                            .and_then(content_value_text)
                            .unwrap_or_else(|| compact_json(part, limits.max_message_chars));
                        collector.push(
                            format!("claude-{line_index}-{part_index}"),
                            CliSessionMessageRole::Tool,
                            result,
                            timestamp.clone(),
                            model.clone(),
                            Some(tool_name),
                        );
                    }
                    _ => {}
                }
            }
        },
    )?;
    Ok(collector.finish(source_truncated))
}

fn parse_transcript(
    cli_kind: AgentCliKind,
    path: &Path,
    expected_workdir: &Path,
) -> Result<Option<CliSessionSummary>, String> {
    let file = File::open(path)
        .map_err(|err| format!("打开 Claude Code 会话记录失败：{}：{err}", path.display()))?;
    let mut summary = TranscriptSummary::default();
    for line in BufReader::new(file).lines() {
        let line = line
            .map_err(|err| format!("读取 Claude Code 会话记录失败：{}：{err}", path.display()))?;
        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        summary.observe(&value);
    }

    // The encoded project directory is Claude's primary workspace index. A
    // transcript can contain events written after a directory change, or omit
    // `cwd` on metadata-only records, so do not discard it solely because the
    // first observed cwd is absent or differs from the selected path.
    let workdir = summary.workdir_for(expected_workdir);
    let id = summary.session_id.or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_string)
    });
    let Some(id) = id.filter(|id| !id.trim().is_empty()) else {
        return Ok(None);
    };
    let title = first_non_empty([
        summary
            .latest_ai_title
            .and_then(|value| clean_text(value, 100)),
        summary
            .first_user_message
            .clone()
            .and_then(|value| clean_text(value, 100)),
    ]);
    let preview = summary
        .first_user_message
        .and_then(|value| clean_text(value, 240));
    let model = summary.last_model.clone();
    let models = summary.models.into_iter().collect::<Vec<_>>();
    Ok(Some(CliSessionSummary {
        id,
        title,
        preview,
        model,
        models,
        cli_kind,
        created_at: normalize_timestamp(summary.created_at.as_deref()),
        updated_at: normalize_timestamp(summary.updated_at.as_deref()),
        workdir,
        cli_version: summary.cli_version.and_then(|value| clean_text(value, 50)),
        archived: false,
        can_resume: true,
        metadata_source: "claudeTranscript".to_string(),
    }))
}

#[derive(Default)]
struct TranscriptSummary {
    session_id: Option<String>,
    workdirs: Vec<String>,
    cli_version: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    first_user_message: Option<String>,
    latest_ai_title: Option<String>,
    last_model: Option<String>,
    models: BTreeSet<String>,
}

impl TranscriptSummary {
    fn observe(&mut self, value: &Value) {
        self.session_id = self
            .session_id
            .take()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| string_field(value, &["sessionId", "session_id"]));
        if let Some(workdir) = string_field(value, &["cwd", "workdir"]) {
            if !workdir.trim().is_empty() && !self.workdirs.iter().any(|item| item == &workdir) {
                self.workdirs.push(workdir);
            }
        }
        self.cli_version = self
            .cli_version
            .take()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| string_field(value, &["version", "cliVersion"]));
        let timestamp = string_field(value, &["timestamp", "createdAt"]);
        if self.created_at.is_none() {
            self.created_at = timestamp.clone();
        }
        if timestamp.is_some() {
            self.updated_at = timestamp;
        }

        if value.get("type").and_then(Value::as_str) == Some("ai-title") {
            if let Some(title) = string_field(value, &["aiTitle", "title", "summary", "name"]) {
                if !title.trim().is_empty() {
                    self.latest_ai_title = Some(title);
                }
            }
        }
        if value.get("type").and_then(Value::as_str) == Some("user")
            && !value
                .get("isMeta")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            && self.first_user_message.is_none()
        {
            self.first_user_message = message_text(value);
        }
        if value.get("type").and_then(Value::as_str) == Some("assistant") {
            if let Some(model) = value
                .get("message")
                .and_then(|message| message.get("model"))
                .and_then(Value::as_str)
                .filter(|model| !model.trim().is_empty() && *model != "<synthetic>")
            {
                let model = model.trim().to_string();
                self.models.insert(model.clone());
                self.last_model = Some(model);
            }
        }
    }

    fn workdir_for(&self, expected: &Path) -> String {
        let expected_key = path_key(expected);
        self.workdirs
            .iter()
            .find(|workdir| path_key(Path::new(workdir)) == expected_key)
            .cloned()
            .unwrap_or_else(|| expected.to_string_lossy().to_string())
    }
}

fn string_field(value: &Value, fields: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| {
        value
            .get(*field)
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn message_text(value: &Value) -> Option<String> {
    let content = value.get("message")?.get("content")?;
    if let Some(text) = content.as_str() {
        return user_text_candidate(text);
    }
    let parts = content.as_array()?.iter().filter_map(|part| {
        let object = part.as_object()?;
        match object.get("type").and_then(Value::as_str) {
            Some("text") | None => object.get("text").and_then(Value::as_str),
            _ => None,
        }
    });
    let text = parts.collect::<Vec<_>>().join(" ");
    user_text_candidate(&text)
}

fn content_value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => (!text.trim().is_empty()).then(|| text.to_string()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| {
                    part.as_str().or_else(|| {
                        part.get("text")
                            .or_else(|| part.get("content"))
                            .and_then(Value::as_str)
                    })
                })
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(text)
        }
        Value::Object(_) => value
            .get("text")
            .or_else(|| value.get("content"))
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn visible_user_text(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.starts_with("<command-name")
        && !value.starts_with("<local-command")
        && !value.starts_with("<command-message")
}

fn user_text_candidate(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.starts_with("<command-name")
        || value.starts_with("<local-command")
        || value.starts_with("<command-message")
    {
        return None;
    }
    Some(value.to_string())
}

fn path_key(path: &Path) -> String {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut value = path.to_string_lossy().replace('\\', "/");
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        value.make_ascii_lowercase();
    }
    value
}

fn encode_project_path(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' => '-',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests;
