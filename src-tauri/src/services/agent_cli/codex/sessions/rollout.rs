use crate::{
    models::{CliSessionMessage, CliSessionMessageRole},
    services::{
        agent_cli::contracts::{
            SessionContentSearchRequest, SessionContentSearchResult, SessionIndexLoadResult,
            SessionIndexMessage, SessionReadLimits,
        },
        cli_sessions::{
            compact_json, json_record_may_match, json_text, normalize_timestamp,
            read_json_lines_limited, scan_json_records, scan_json_records_background,
            session_index_source_fingerprint, truncate_text, SessionContentSearchCollector,
            SessionMessageCollector,
        },
    },
};
use serde_json::Value;
use std::{collections::HashMap, path::Path};

const INDEX_PARSER_VERSION: u32 = 1;

pub(super) fn index_rollout(
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

    let mut primary = Vec::new();
    let mut fallback = Vec::new();
    let mut has_primary = false;
    scan_json_records_background(path, "索引 Codex 会话正文", is_current, |sequence, line| {
        let possible_primary = contains_bytes(line, b"event_msg")
            && (contains_bytes(line, b"user_message")
                || contains_bytes(line, b"agent_message"));
        let possible_fallback = contains_bytes(line, b"response_item")
            && contains_bytes(line, b"message");
        if !possible_primary && !possible_fallback {
            return false;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return false;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("event_msg") => {
                let Some(payload) = value.get("payload") else {
                    return false;
                };
                let role = match payload.get("type").and_then(Value::as_str) {
                    Some("user_message") => CliSessionMessageRole::User,
                    Some("agent_message") => CliSessionMessageRole::Assistant,
                    _ => return false,
                };
                let Some(content) = payload
                    .get("message")
                    .or_else(|| payload.get("text"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|content| !content.is_empty())
                else {
                    return false;
                };
                has_primary = true;
                primary.push(SessionIndexMessage {
                    id: format!("codex-{sequence}"),
                    role,
                    content: content.to_string(),
                });
            }
            Some("response_item") => {
                let Some(payload) = value.get("payload") else {
                    return false;
                };
                if payload.get("type").and_then(Value::as_str) != Some("message") {
                    return false;
                }
                let role = match payload.get("role").and_then(Value::as_str) {
                    Some("user") => CliSessionMessageRole::User,
                    Some("assistant") => CliSessionMessageRole::Assistant,
                    _ => return false,
                };
                if let Some(content) = response_message_text(payload) {
                    fallback.push(SessionIndexMessage {
                        id: format!("codex-{sequence}"),
                        role,
                        content,
                    });
                }
            }
            _ => {}
        }
        false
    })?;
    Ok(SessionIndexLoadResult::Updated {
        fingerprint,
        source_bytes,
        messages: if has_primary { primary } else { fallback },
    })
}

pub(super) fn search_rollout(
    path: &Path,
    request: &SessionContentSearchRequest,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String> {
    let mut primary = SessionContentSearchCollector::new(request);
    let mut fallback = SessionContentSearchCollector::new(request);
    let mut has_primary = false;
    scan_json_records(
        path,
        "检索 Codex 会话正文",
        is_current,
        |_sequence, line| {
            let possible_primary = contains_bytes(line, b"event_msg")
                && (contains_bytes(line, b"user_message")
                    || contains_bytes(line, b"agent_message"));
            let may_match = json_record_may_match(line, request);
            if !possible_primary && !may_match {
                return false;
            }
            let Ok(value) = serde_json::from_slice::<Value>(line) else {
                return false;
            };
            match value.get("type").and_then(Value::as_str) {
                Some("event_msg") => {
                    let Some(payload) = value.get("payload") else {
                        return false;
                    };
                    let role = match payload.get("type").and_then(Value::as_str) {
                        Some("user_message") => Some(CliSessionMessageRole::User),
                        Some("agent_message") => Some(CliSessionMessageRole::Assistant),
                        _ => None,
                    };
                    if role.is_some() {
                        has_primary = true;
                        if let Some(content) = payload
                            .get("message")
                            .or_else(|| payload.get("text"))
                            .and_then(Value::as_str)
                        {
                            primary.observe(content);
                        }
                    }
                }
                Some("response_item") => {
                    if !may_match {
                        return false;
                    }
                    let Some(payload) = value.get("payload") else {
                        return false;
                    };
                    match payload.get("type").and_then(Value::as_str) {
                        Some("message") => {
                            match payload.get("role").and_then(Value::as_str) {
                                Some("user") | Some("assistant") => {}
                                _ => return false,
                            }
                            if let Some(content) = response_message_text(payload) {
                                fallback.observe(&content);
                            }
                        }
                        _ => return false,
                    }
                }
                _ => return false,
            }
            false
        },
    )?;
    Ok(if has_primary {
        primary.finish()
    } else {
        fallback.finish()
    })
}

fn contains_bytes(value: &[u8], needle: &[u8]) -> bool {
    value.windows(needle.len()).any(|window| window == needle)
}

#[derive(Debug)]
struct PendingMessage {
    sequence: usize,
    role: CliSessionMessageRole,
    content: String,
    timestamp: Option<String>,
    model: Option<String>,
    tool_name: Option<String>,
}

struct PendingBuffer {
    limits: SessionReadLimits,
    messages: Vec<PendingMessage>,
    total_chars: usize,
    truncated: bool,
}

impl PendingBuffer {
    fn new(limits: SessionReadLimits) -> Self {
        Self {
            limits,
            messages: Vec::new(),
            total_chars: 0,
            truncated: false,
        }
    }

    fn push(&mut self, mut message: PendingMessage) -> Option<usize> {
        if self.messages.len() >= self.limits.max_messages
            || self.total_chars >= self.limits.max_total_chars
        {
            self.truncated = true;
            return None;
        }
        let allowed = self
            .limits
            .max_message_chars
            .min(self.limits.max_total_chars - self.total_chars);
        let (content, content_truncated) = truncate_text(&message.content, allowed);
        if content.is_empty() {
            self.truncated = true;
            return None;
        }
        self.truncated |= content_truncated;
        self.total_chars = self.total_chars.saturating_add(content.chars().count());
        message.content = content;
        let index = self.messages.len();
        self.messages.push(message);
        Some(index)
    }

    fn append(&mut self, index: usize, prefix: &str, value: &str) {
        let Some(message) = self.messages.get_mut(index) else {
            return;
        };
        let message_chars = message.content.chars().count();
        let allowed = self
            .limits
            .max_message_chars
            .saturating_sub(message_chars)
            .min(self.limits.max_total_chars.saturating_sub(self.total_chars));
        let addition = format!("{prefix}{value}");
        let (addition, addition_truncated) = truncate_text(&addition, allowed);
        if addition.is_empty() {
            self.truncated = true;
            return;
        }
        self.truncated |= addition_truncated;
        self.total_chars = self.total_chars.saturating_add(addition.chars().count());
        message.content.push_str(&addition);
    }

    fn clear(&mut self) {
        self.messages.clear();
        self.total_chars = 0;
        self.truncated = false;
    }

    fn finish(self) -> (Vec<PendingMessage>, bool) {
        (self.messages, self.truncated)
    }
}

pub(super) fn parse_rollout_messages(
    path: &Path,
    limits: SessionReadLimits,
) -> Result<(Vec<CliSessionMessage>, bool, usize), String> {
    let mut primary = PendingBuffer::new(limits);
    let mut fallback = PendingBuffer::new(limits);
    let mut tools = PendingBuffer::new(limits);
    let mut tool_names = HashMap::<String, String>::new();
    let mut tool_indexes = HashMap::<String, usize>::new();
    let mut current_model = None::<String>;
    let mut has_primary = false;
    let source_truncated = read_json_lines_limited(
        path,
        limits.max_file_bytes,
        "读取 Codex 会话正文",
        |sequence, value| {
            let timestamp = normalize_timestamp(value.get("timestamp").and_then(Value::as_str));
            match value.get("type").and_then(Value::as_str) {
                Some("turn_context") => {
                    if let Some(model) = value
                        .get("payload")
                        .and_then(|payload| payload.get("model"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|model| !model.is_empty())
                    {
                        current_model = Some(model.to_string());
                    }
                }
                Some("event_msg") => {
                    let Some(payload) = value.get("payload") else {
                        return;
                    };
                    if let Some(message) =
                        event_tool_message(payload, Some(limits.max_message_chars))
                    {
                        tools.push(PendingMessage {
                            sequence,
                            role: CliSessionMessageRole::Tool,
                            content: message.content,
                            timestamp,
                            model: current_model.clone(),
                            tool_name: Some(message.name),
                        });
                        return;
                    }
                    let Some(content) = payload
                        .get("message")
                        .or_else(|| payload.get("text"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|content| !content.is_empty())
                    else {
                        return;
                    };
                    let role = match payload.get("type").and_then(Value::as_str) {
                        Some("user_message") => CliSessionMessageRole::User,
                        Some("agent_message") => CliSessionMessageRole::Assistant,
                        _ => return,
                    };
                    if !has_primary {
                        has_primary = true;
                        fallback.clear();
                    }
                    primary.push(PendingMessage {
                        sequence,
                        role,
                        content: content.to_string(),
                        timestamp,
                        model: (role == CliSessionMessageRole::Assistant)
                            .then(|| current_model.clone())
                            .flatten(),
                        tool_name: None,
                    });
                }
                Some("response_item") => {
                    let Some(payload) = value.get("payload") else {
                        return;
                    };
                    match payload.get("type").and_then(Value::as_str) {
                        Some("message") => {
                            if has_primary {
                                return;
                            }
                            let role = match payload.get("role").and_then(Value::as_str) {
                                Some("user") => CliSessionMessageRole::User,
                                Some("assistant") => CliSessionMessageRole::Assistant,
                                _ => return,
                            };
                            let Some(content) = response_message_text(payload) else {
                                return;
                            };
                            fallback.push(PendingMessage {
                                sequence,
                                role,
                                content,
                                timestamp,
                                model: (role == CliSessionMessageRole::Assistant)
                                    .then(|| current_model.clone())
                                    .flatten(),
                                tool_name: None,
                            });
                        }
                        Some("custom_tool_call") | Some("function_call") => {
                            let name = payload
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("工具")
                                .to_string();
                            let call_id = payload
                                .get("call_id")
                                .and_then(Value::as_str)
                                .map(str::to_string);
                            let input = payload
                                .get("input")
                                .or_else(|| payload.get("arguments"))
                                .map(|value| match value {
                                    Value::String(value) => value.to_string(),
                                    _ => compact_json(value, limits.max_message_chars),
                                })
                                .unwrap_or_default();
                            let tool_index = tools.push(PendingMessage {
                                sequence,
                                role: CliSessionMessageRole::Tool,
                                content: if input.trim().is_empty() {
                                    format!("调用工具 {name}")
                                } else {
                                    format!("调用工具 {name}\n{input}")
                                },
                                timestamp,
                                model: current_model.clone(),
                                tool_name: Some(name.clone()),
                            });
                            if let (Some(call_id), Some(tool_index)) = (call_id, tool_index) {
                                tool_names.insert(call_id.clone(), name);
                                tool_indexes.insert(call_id, tool_index);
                            }
                        }
                        Some("custom_tool_call_output") | Some("function_call_output") => {
                            let call_id = payload
                                .get("call_id")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            let name = tool_names
                                .get(call_id)
                                .cloned()
                                .unwrap_or_else(|| "工具".to_string());
                            let output = payload
                                .get("output")
                                .map(|value| match value {
                                    Value::String(value) => value.to_string(),
                                    _ => compact_json(value, limits.max_message_chars),
                                })
                                .unwrap_or_default();
                            if let Some(index) = tool_indexes.get(call_id).copied() {
                                let result = if output.trim().is_empty() {
                                    format!("{name} 已返回结果")
                                } else {
                                    output
                                };
                                tools.append(index, "\n结果\n", &result);
                            } else {
                                tools.push(PendingMessage {
                                    sequence,
                                    role: CliSessionMessageRole::Tool,
                                    content: if output.trim().is_empty() {
                                        format!("{name} 已返回结果")
                                    } else {
                                        output
                                    },
                                    timestamp,
                                    model: current_model.clone(),
                                    tool_name: Some(name),
                                });
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        },
    )?;

    let (mut selected, message_truncated) = if has_primary {
        primary.finish()
    } else {
        fallback.finish()
    };
    let (tool_messages, tool_truncated) = tools.finish();
    selected.extend(tool_messages);
    selected.sort_by_key(|message| message.sequence);
    let mut collector = SessionMessageCollector::new(limits);
    for (index, message) in selected.into_iter().enumerate() {
        collector.push(
            format!("codex-{index}"),
            message.role,
            message.content,
            message.timestamp,
            message.model,
            message.tool_name,
        );
    }
    Ok(collector.finish(source_truncated || message_truncated || tool_truncated))
}

struct EventToolMessage {
    name: String,
    content: String,
}

fn event_tool_message(payload: &Value, limit: Option<usize>) -> Option<EventToolMessage> {
    match payload.get("type").and_then(Value::as_str) {
        Some("mcp_tool_call_end") => {
            let invocation = payload.get("invocation")?;
            let server = invocation
                .get("server")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let tool = invocation
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("MCP 工具");
            let name = if server.is_empty() {
                tool.to_string()
            } else {
                format!("{server}.{tool}")
            };
            let arguments = invocation
                .get("arguments")
                .map(|value| render_json(value, limit))
                .filter(|value| !value.is_empty());
            let result = payload
                .get("result")
                .map(|value| render_json(value, limit))
                .filter(|value| !value.is_empty());
            let mut sections = vec![format!("调用工具 {name}")];
            if let Some(arguments) = arguments {
                sections.push(format!("参数\n{arguments}"));
            }
            if let Some(result) = result {
                sections.push(format!("结果\n{result}"));
            }
            Some(EventToolMessage {
                name,
                content: sections.join("\n"),
            })
        }
        Some("patch_apply_end") => {
            let status = payload
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let changes = payload
                .get("changes")
                .map(|value| render_json(value, limit))
                .filter(|value| !value.is_empty());
            let mut sections = vec![if status.is_empty() {
                "应用代码补丁".to_string()
            } else {
                format!("应用代码补丁：{status}")
            }];
            if let Some(changes) = changes {
                sections.push(changes);
            }
            Some(EventToolMessage {
                name: "apply_patch".to_string(),
                content: sections.join("\n"),
            })
        }
        _ => None,
    }
}

fn render_json(value: &Value, limit: Option<usize>) -> String {
    let text = json_text(value);
    limit
        .map(|limit| truncate_text(&text, limit).0)
        .unwrap_or(text)
}

fn response_message_text(payload: &Value) -> Option<String> {
    let content = payload.get("content")?;
    if let Some(content) = content.as_str() {
        return (!content.trim().is_empty()).then(|| content.to_string());
    }
    let text = content
        .as_array()?
        .iter()
        .filter(|part| {
            matches!(
                part.get("type").and_then(Value::as_str),
                None | Some("text") | Some("input_text") | Some("output_text")
            )
        })
        .filter_map(|part| {
            part.get("text")
                .or_else(|| part.get("input_text"))
                .or_else(|| part.get("output_text"))
                .and_then(Value::as_str)
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}
