use crate::{
    limits,
    models::{
        AgentCliKind, CliSessionDetail, CliSessionMessageRole, CliSessionSummary,
    },
    services::cli_sessions::{
        clean_text, combine_content_search_results, compact_json, first_non_empty,
        normalize_timestamp, read_json_lines_limited, scan_json_lines_matching,
        scan_json_records, scan_json_records_background, session_index_source_fingerprint,
        session_sort_key, timestamp_from_unix, SessionContentSearchCollector,
        SessionMessageCollector,
    },
    util::read_text_file_limited,
};
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

use super::super::contracts::{
    SessionContentSearchRequest, SessionContentSearchResult, SessionIndexLoadResult,
    SessionIndexMessage, SessionReadLimits,
};

const MAX_SCAN_DIRECTORIES: usize = 10_000;
const MAX_SUMMARY_FILES: usize = 2_000;
const MAX_SUMMARY_FILE_BYTES: usize = 256 * 1024;
const INDEX_PARSER_VERSION: u32 = 1;

pub(super) fn list(
    cli_kind: AgentCliKind,
    workdir: &Path,
) -> Result<Vec<CliSessionSummary>, String> {
    let grok_home = super::config::config_dir()
        .ok_or_else(|| "无法定位用户目录，无法读取 Grok Build 历史会话".to_string())?;
    list_from_home(cli_kind, &grok_home, workdir)
}

pub(super) fn detail(
    cli_kind: AgentCliKind,
    workdir: &Path,
    session_id: &str,
    limits: SessionReadLimits,
) -> Result<CliSessionDetail, String> {
    let grok_home = super::config::config_dir()
        .ok_or_else(|| "无法定位用户目录，无法读取 Grok Build 历史会话".to_string())?;
    let sessions_root = grok_home.join("sessions");
    for summary_path in collect_summary_files(&sessions_root)? {
        let Some(summary) = parse_summary(cli_kind, &summary_path, workdir)? else {
            continue;
        };
        if summary.id != session_id {
            continue;
        }
        let session_dir = summary_path
            .parent()
            .ok_or_else(|| "Grok Build 会话目录无效".to_string())?;
        let updates_path = session_dir.join("updates.jsonl");
        let chat_history_path = session_dir.join("chat_history.jsonl");
        let (messages, truncated, omitted_message_count, source) =
            if updates_path.is_file() {
                let (messages, truncated, omitted) =
                    parse_updates(&updates_path, limits)?;
                if messages.is_empty() && chat_history_path.is_file() {
                    let (messages, truncated, omitted) =
                        parse_chat_history(&chat_history_path, limits)?;
                    (messages, truncated, omitted, "grokChatHistory")
                } else {
                    (messages, truncated, omitted, "grokUpdates")
                }
            } else if chat_history_path.is_file() {
                let (messages, truncated, omitted) =
                    parse_chat_history(&chat_history_path, limits)?;
                (messages, truncated, omitted, "grokChatHistory")
            } else {
                return Err("Grok Build 会话摘要存在，但正文文件已不可用".to_string());
            };
        return Ok(CliSessionDetail {
            session: summary,
            messages,
            truncated,
            omitted_message_count,
            content_source: source.to_string(),
        });
    }
    Err("未找到指定的 Grok Build 会话".to_string())
}

pub(super) fn search(
    cli_kind: AgentCliKind,
    workdir: &Path,
    session_id: &str,
    request: &SessionContentSearchRequest,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String> {
    let grok_home = super::config::config_dir()
        .ok_or_else(|| "无法定位用户目录，无法读取 Grok Build 历史会话".to_string())?;
    let sessions_root = grok_home.join("sessions");
    for summary_path in collect_summary_files(&sessions_root)? {
        let Some(summary) = parse_summary(cli_kind, &summary_path, workdir)? else {
            continue;
        };
        if summary.id != session_id {
            continue;
        }
        let session_dir = summary_path
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
        return search_chat_history(&chat_history_path, request, is_current);
    }
    Err("未找到指定的 Grok Build 会话".to_string())
}

pub(super) fn index(
    cli_kind: AgentCliKind,
    workdir: &Path,
    session_id: &str,
    known_fingerprint: Option<&str>,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionIndexLoadResult, String> {
    let grok_home = super::config::config_dir()
        .ok_or_else(|| "无法定位用户目录，无法读取 Grok Build 历史会话".to_string())?;
    let sessions_root = grok_home.join("sessions");
    for summary_path in collect_summary_files(&sessions_root)? {
        let Some(summary) = parse_summary(cli_kind, &summary_path, workdir)? else {
            continue;
        };
        if summary.id != session_id {
            continue;
        }
        let session_dir = summary_path
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
        return Ok(SessionIndexLoadResult::Updated {
            fingerprint,
            source_bytes,
            messages,
        });
    }
    Err("未找到指定的 Grok Build 会话".to_string())
}

fn index_updates(
    path: &Path,
    is_current: &dyn Fn() -> bool,
) -> Result<Vec<SessionIndexMessage>, String> {
    let mut messages = Vec::new();
    let mut current: Option<(CliSessionMessageRole, String, usize)> = None;
    scan_json_records_background(path, "索引 Grok Build 会话正文", is_current, |line_index, line| {
        if !(line.windows(18).any(|window| window == b"user_message_chunk")
            || line
                .windows(19)
                .any(|window| window == b"agent_message_chunk"))
        {
            return false;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return false;
        };
        let Some(update) = value
            .get("params")
            .and_then(|params| params.get("update"))
        else {
            return false;
        };
        let role = match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("user_message_chunk") => CliSessionMessageRole::User,
            Some("agent_message_chunk") => CliSessionMessageRole::Assistant,
            _ => return false,
        };
        let Some(content) = update.get("content").and_then(content_text) else {
            return false;
        };
        if current
            .as_ref()
            .is_some_and(|(current_role, _, _)| *current_role != role)
        {
            if let Some((pending_role, pending_content, pending_line)) = current.take() {
                if !pending_content.trim().is_empty() {
                    messages.push(SessionIndexMessage {
                        id: format!("grok-{pending_line}"),
                        role: pending_role,
                        content: pending_content,
                    });
                }
            }
        }
        let pending = current.get_or_insert_with(|| (role, String::new(), line_index));
        pending.1.push_str(&content);
        false
    })?;
    if let Some((role, content, line_index)) = current {
        if !content.trim().is_empty() {
            messages.push(SessionIndexMessage {
                id: format!("grok-{line_index}"),
                role,
                content,
            });
        }
    }
    Ok(messages)
}

fn index_chat_history(
    path: &Path,
    is_current: &dyn Fn() -> bool,
) -> Result<Vec<SessionIndexMessage>, String> {
    let mut messages = Vec::new();
    scan_json_records_background(path, "索引 Grok Build 会话正文", is_current, |line_index, line| {
        if !(line.windows(4).any(|window| window.eq_ignore_ascii_case(b"user"))
            || line
                .windows(9)
                .any(|window| window.eq_ignore_ascii_case(b"assistant")))
        {
            return false;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return false;
        };
        let role = match value.get("type").and_then(Value::as_str) {
            Some("user") => CliSessionMessageRole::User,
            Some("assistant") => CliSessionMessageRole::Assistant,
            _ => return false,
        };
        if let Some(content) = value.get("content").and_then(content_text) {
            messages.push(SessionIndexMessage {
                id: format!("grok-{line_index}"),
                role,
                content,
            });
        }
        false
    })?;
    Ok(messages)
}

fn search_updates(
    path: &Path,
    request: &SessionContentSearchRequest,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String> {
    let mut state = GrokUpdateSearchState::new(request);
    scan_json_records(
        path,
        "检索 Grok Build 会话正文",
        is_current,
        |line_index, line| {
            let Ok(value) = serde_json::from_slice::<Value>(line) else {
                return false;
            };
            let Some(update) = value
                .get("params")
                .and_then(|params| params.get("update"))
            else {
                return false;
            };
            state.observe(line_index, update);
            false
        },
    )?;
    Ok(state.finish())
}

const MAX_STREAMED_MESSAGE_CHARS: usize = 4 * 1024 * 1024;

struct GrokUpdateSearchState<'a> {
    request: &'a SessionContentSearchRequest,
    messages: Vec<SessionContentSearchResult>,
    current: Option<PendingStreamMessage>,
}

struct PendingStreamMessage {
    role: CliSessionMessageRole,
    content: String,
    char_count: usize,
}

impl<'a> GrokUpdateSearchState<'a> {
    fn new(request: &'a SessionContentSearchRequest) -> Self {
        Self {
            request,
            messages: Vec::new(),
            current: None,
        }
    }

    fn observe(&mut self, _line_index: usize, update: &Value) {
        let update_type = update.get("sessionUpdate").and_then(Value::as_str);
        match update_type {
            Some("user_message_chunk") | Some("agent_message_chunk") => {
                let role = if update_type == Some("user_message_chunk") {
                    CliSessionMessageRole::User
                } else {
                    CliSessionMessageRole::Assistant
                };
                let Some(content) = update.get("content").and_then(content_text) else {
                    return;
                };
                if self
                    .current
                    .as_ref()
                    .is_some_and(|current| current.role != role)
                {
                    self.flush_current();
                }
                // A single provider chunk can itself exceed the per-message bound. Split it
                // into bounded segments instead of appending an unbounded chunk or repeatedly
                // rescanning the accumulated string to discover its length.
                let mut remainder = content.as_str();
                while !remainder.is_empty() {
                    if self.current.is_none() {
                        self.current = Some(PendingStreamMessage {
                            role,
                            content: String::new(),
                            char_count: 0,
                        });
                    }
                    let room = self
                        .current
                        .as_ref()
                        .map(|current| {
                            MAX_STREAMED_MESSAGE_CHARS.saturating_sub(current.char_count)
                        })
                        .unwrap_or_default();
                    if room == 0 {
                        self.flush_current();
                        continue;
                    }
                    let split_at = remainder
                        .char_indices()
                        .nth(room)
                        .map(|(index, _)| index)
                        .unwrap_or(remainder.len());
                    let addition = &remainder[..split_at];
                    if let Some(current) = self.current.as_mut() {
                        current.char_count = current
                            .char_count
                            .saturating_add(addition.chars().count());
                        current.content.push_str(addition);
                    }
                    remainder = &remainder[split_at..];
                    if !remainder.is_empty() {
                        self.flush_current();
                    }
                }
            }
            Some(kind) if kind.contains("tool") => {
                self.flush_current();
            }
            _ => {}
        }
    }

    fn flush_current(&mut self) {
        let Some(current) = self.current.take() else {
            return;
        };
        let mut collector = SessionContentSearchCollector::new(self.request);
        collector.observe(&current.content);
        self.messages.push(collector.finish());
    }

    fn finish(mut self) -> SessionContentSearchResult {
        self.flush_current();
        combine_content_search_results(self.messages)
    }
}

fn search_chat_history(
    path: &Path,
    request: &SessionContentSearchRequest,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String> {
    let mut collector = SessionContentSearchCollector::new(request);
    scan_json_lines_matching(
        path,
        "检索 Grok Build 会话正文",
        request,
        is_current,
        |_line_index, value| {
            match value.get("type").and_then(Value::as_str) {
                Some("user") => CliSessionMessageRole::User,
                Some("assistant") => CliSessionMessageRole::Assistant,
                _ => return false,
            };
            if let Some(content) = value.get("content").and_then(content_text) {
                collector.observe(&content);
            }
            collector.complete()
        },
    )?;
    Ok(collector.finish())
}

fn parse_updates(
    path: &Path,
    limits: SessionReadLimits,
) -> Result<(Vec<crate::models::CliSessionMessage>, bool, usize), String> {
    struct PendingUpdateMessage {
        role: CliSessionMessageRole,
        content: String,
        timestamp: Option<String>,
        model: Option<String>,
        tool_name: Option<String>,
    }

    let mut pending = Vec::<PendingUpdateMessage>::new();
    let mut tool_positions = HashMap::<String, usize>::new();
    let source_truncated = read_json_lines_limited(
        path,
        limits.max_file_bytes,
        "读取 Grok Build 会话正文",
        |_line_index, value| {
            let timestamp = timestamp_from_unix(value.get("timestamp").and_then(Value::as_i64));
            let Some(update) = value
                .get("params")
                .and_then(|params| params.get("update"))
            else {
                return;
            };
            let update_type = update.get("sessionUpdate").and_then(Value::as_str);
            let model = update
                .get("_meta")
                .and_then(|meta| meta.get("modelId"))
                .and_then(Value::as_str)
                .map(str::to_string);
            match update_type {
                Some("user_message_chunk") | Some("agent_message_chunk") => {
                    let role = if update_type == Some("user_message_chunk") {
                        CliSessionMessageRole::User
                    } else {
                        CliSessionMessageRole::Assistant
                    };
                    if let Some(content) = update.get("content").and_then(content_text) {
                        if let Some(previous) = pending.last_mut().filter(|message| {
                            message.role == role && message.tool_name.is_none()
                        }) {
                            previous.content.push_str(&content);
                            if previous.timestamp.is_none() {
                                previous.timestamp = timestamp;
                            }
                            if model.is_some() {
                                previous.model = model;
                            }
                        } else {
                            pending.push(PendingUpdateMessage {
                                role,
                                content,
                                timestamp,
                                model,
                                tool_name: None,
                            });
                        }
                    }
                }
                Some(kind) if kind.contains("tool") => {
                    let name = update
                        .get("title")
                        .or_else(|| update.get("name"))
                        .or_else(|| update.get("toolName"))
                        .and_then(Value::as_str)
                        .unwrap_or("工具")
                        .to_string();
                    let content = compact_json(update, limits.max_message_chars);
                    let tool_id = update
                        .get("toolCallId")
                        .or_else(|| update.get("tool_call_id"))
                        .or_else(|| update.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    if let Some(index) = tool_id
                        .as_ref()
                        .and_then(|tool_id| tool_positions.get(tool_id))
                        .copied()
                    {
                        pending[index].content = content;
                        pending[index].timestamp = timestamp;
                        pending[index].model = model;
                        pending[index].tool_name = Some(name);
                    } else {
                        let index = pending.len();
                        pending.push(PendingUpdateMessage {
                            role: CliSessionMessageRole::Tool,
                            content,
                            timestamp,
                            model,
                            tool_name: Some(name),
                        });
                        if let Some(tool_id) = tool_id {
                            tool_positions.insert(tool_id, index);
                        }
                    }
                }
                _ => {}
            }
        },
    )?;
    let mut collector = SessionMessageCollector::new(limits);
    for (index, message) in pending.into_iter().enumerate() {
        collector.push(
            format!("grok-{index}"),
            message.role,
            message.content,
            message.timestamp,
            message.model,
            message.tool_name,
        );
    }
    Ok(collector.finish(source_truncated))
}

fn parse_chat_history(
    path: &Path,
    limits: SessionReadLimits,
) -> Result<(Vec<crate::models::CliSessionMessage>, bool, usize), String> {
    let mut collector = SessionMessageCollector::new(limits);
    let source_truncated = read_json_lines_limited(
        path,
        limits.max_file_bytes,
        "读取 Grok Build 会话正文",
        |line_index, value| {
            let role = match value.get("type").and_then(Value::as_str) {
                Some("user") => CliSessionMessageRole::User,
                Some("assistant") => CliSessionMessageRole::Assistant,
                _ => return,
            };
            let model = value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string);
            let timestamp = normalize_timestamp(
                value
                    .get("timestamp")
                    .or_else(|| value.get("ts"))
                    .and_then(Value::as_str),
            );
            let Some(content) = value.get("content") else {
                return;
            };
            if let Some(text) = content_text(content) {
                collector.push(
                    format!("grok-{line_index}"),
                    role,
                    text,
                    timestamp.clone(),
                    model.clone(),
                    None,
                );
            }
            if let Some(parts) = content.as_array() {
                for (part_index, part) in parts.iter().enumerate() {
                    let kind = part.get("type").and_then(Value::as_str).unwrap_or_default();
                    if kind.contains("tool") {
                        let name = part
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("工具")
                            .to_string();
                        collector.push(
                            format!("grok-{line_index}-{part_index}"),
                            CliSessionMessageRole::Tool,
                            compact_json(part, limits.max_message_chars),
                            timestamp.clone(),
                            model.clone(),
                            Some(name),
                        );
                    }
                }
            }
        },
    )?;
    Ok(collector.finish(source_truncated))
}

fn content_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => (!text.trim().is_empty()).then(|| text.to_string()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter(|part| !content_part_is_hidden(part))
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

fn content_part_is_hidden(value: &Value) -> bool {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    ["tool", "thought", "thinking", "reasoning", "analysis"]
        .iter()
        .any(|hidden| kind.contains(hidden))
}

fn list_from_home(
    cli_kind: AgentCliKind,
    grok_home: &Path,
    workdir: &Path,
) -> Result<Vec<CliSessionSummary>, String> {
    let sessions_root = grok_home.join("sessions");
    if !sessions_root.is_dir() {
        return Ok(Vec::new());
    }
    let files = collect_summary_files(&sessions_root)?;
    let mut sessions = Vec::<CliSessionSummary>::new();
    let mut indexes = HashMap::<String, usize>::new();
    let mut failed_files = 0usize;
    let mut last_error = None;
    for path in &files {
        match parse_summary(cli_kind, path, workdir) {
            Ok(Some(session)) => {
                if let Some(index) = indexes.get(&session.id).copied() {
                    if session_sort_key(session.updated_at.as_deref())
                        > session_sort_key(sessions[index].updated_at.as_deref())
                    {
                        sessions[index] = session;
                    }
                } else {
                    indexes.insert(session.id.clone(), sessions.len());
                    sessions.push(session);
                }
            }
            Ok(None) => {}
            Err(error) => {
                failed_files += 1;
                last_error = Some(error);
            }
        }
    }
    if !files.is_empty() && sessions.is_empty() && failed_files == files.len() {
        return Err(last_error.unwrap_or_else(|| "读取 Grok Build 历史会话失败".to_string()));
    }
    Ok(sessions)
}

fn collect_summary_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut directories = vec![root.to_path_buf()];
    let mut scanned = 0usize;
    while let Some(directory) = directories.pop() {
        if scanned >= MAX_SCAN_DIRECTORIES || files.len() >= MAX_SUMMARY_FILES {
            break;
        }
        scanned += 1;
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(err) if directory == root => {
                return Err(format!(
                    "读取 Grok Build 会话目录失败：{}：{err}",
                    directory.display()
                ));
            }
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                if directories.len() + scanned < MAX_SCAN_DIRECTORIES {
                    directories.push(path);
                }
            } else if file_type.is_file()
                && path.file_name().is_some_and(|name| name == "summary.json")
            {
                files.push(path);
                if files.len() >= MAX_SUMMARY_FILES {
                    break;
                }
            }
        }
    }
    Ok(files)
}

fn parse_summary(
    cli_kind: AgentCliKind,
    path: &Path,
    expected_workdir: &Path,
) -> Result<Option<CliSessionSummary>, String> {
    let text = read_text_file_limited(
        path,
        MAX_SUMMARY_FILE_BYTES.min(limits::MAX_CLI_CONFIG_FILE_BYTES),
        "读取 Grok Build 会话摘要",
    )?;
    let value = serde_json::from_str::<Value>(&text)
        .map_err(|err| format!("解析 Grok Build 会话摘要失败({}): {err}", path.display()))?;
    let Some(info) = value.get("info").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(id) = info
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return Ok(None);
    };
    let Some(workdir) = info
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|workdir| !workdir.is_empty())
    else {
        return Ok(None);
    };
    if path_key(Path::new(workdir)) != path_key(expected_workdir) {
        return Ok(None);
    }
    if value.get("hidden").and_then(Value::as_bool) == Some(true)
        || value
            .get("session_kind")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|kind| kind.starts_with("subagent"))
    {
        return Ok(None);
    }
    let has_message_count = value.get("num_messages").is_some()
        || value.get("num_chat_messages").is_some();
    let num_messages = value
        .get("num_messages")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let num_chat_messages = value
        .get("num_chat_messages")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    if has_message_count && num_messages == 0 && num_chat_messages == 0 {
        return Ok(None);
    }

    let generated_title = string_field(&value, "generated_title");
    let session_summary = string_field(&value, "session_summary");
    let last_turn_summary = string_field(&value, "last_turn_summary");
    let title = first_non_empty([
        generated_title
            .as_deref()
            .and_then(|title| clean_text(title, 100)),
        session_summary
            .as_deref()
            .and_then(|title| clean_text(title, 100)),
        last_turn_summary
            .as_deref()
            .and_then(|title| clean_text(title, 100)),
    ]);
    let preview = session_summary
        .as_deref()
        .filter(|summary| generated_title.as_deref() != Some(*summary))
        .and_then(|summary| clean_text(summary, 240))
        .or_else(|| last_turn_summary.and_then(|summary| clean_text(summary, 240)));
    let model = string_field(&value, "current_model_id")
        .and_then(|model| clean_text(model, 120));
    let models = model
        .clone()
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let created_at = normalize_timestamp(
        value
            .get("created_at")
            .and_then(Value::as_str),
    );
    let updated_at = normalize_timestamp(value.get("last_active_at").and_then(Value::as_str))
        .or_else(|| normalize_timestamp(value.get("updated_at").and_then(Value::as_str)));

    Ok(Some(CliSessionSummary {
        id: id.to_string(),
        title,
        preview,
        model,
        models,
        cli_kind,
        created_at,
        updated_at,
        workdir: workdir.to_string(),
        cli_version: None,
        archived: false,
        can_resume: true,
        metadata_source: "grokSummary".to_string(),
    }))
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn path_key(path: &Path) -> String {
    let mut value = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        value.make_ascii_lowercase();
    }
    value
}

#[cfg(test)]
mod tests;
