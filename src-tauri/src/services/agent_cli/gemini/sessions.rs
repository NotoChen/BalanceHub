use crate::{
    models::{
        AgentCliKind, CliSessionDetail, CliSessionMessageRole, CliSessionSummary,
    },
    services::cli_sessions::{
        clean_text, combine_content_search_results, compact_json, first_non_empty,
        normalize_timestamp, read_json_lines_limited, scan_json_records,
        scan_json_records_background,
        session_index_source_fingerprint, session_sort_key, SessionContentSearchCollector,
        SessionMessageCollector,
    },
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
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
    let config_dir = super::config::config_dir()
        .ok_or_else(|| "无法定位用户目录，无法读取 Gemini CLI 历史会话".to_string())?;
    list_from_config_dir(cli_kind, &config_dir, workdir)
}

pub(super) fn detail(
    cli_kind: AgentCliKind,
    workdir: &Path,
    session_id: &str,
    limits: SessionReadLimits,
) -> Result<CliSessionDetail, String> {
    let config_dir = super::config::config_dir()
        .ok_or_else(|| "无法定位用户目录，无法读取 Gemini CLI 历史会话".to_string())?;
    for path in chat_files(&config_dir, workdir) {
        let Some(summary) = parse_session(cli_kind, &path, workdir)? else {
            continue;
        };
        if summary.id != session_id {
            continue;
        }
        let (conversation, source_truncated) = load_conversation_limited(&path, limits)?;
        let mut collector = SessionMessageCollector::new(limits);
        for (index, message) in conversation.messages.into_iter().enumerate() {
            let timestamp = normalize_timestamp(message.timestamp.as_deref());
            let model = message.model.clone();
            match message.kind.as_str() {
                "user" => {
                    if let Some(text) = message.text {
                        if !is_ignored_user_content(text.trim()) {
                            collector.push(
                                format!("gemini-{index}"),
                                CliSessionMessageRole::User,
                                text,
                                timestamp.clone(),
                                None,
                                None,
                            );
                        }
                    }
                }
                "gemini" => {
                    if let Some(text) = message.text {
                        collector.push(
                            format!("gemini-{index}"),
                            CliSessionMessageRole::Assistant,
                            text,
                            timestamp.clone(),
                            model.clone(),
                            None,
                        );
                    }
                }
                kind if kind.contains("tool") => {
                    if let Some(text) = message.text {
                        collector.push(
                            format!("gemini-{index}"),
                            CliSessionMessageRole::Tool,
                            text,
                            timestamp.clone(),
                            model.clone(),
                            None,
                        );
                    }
                }
                _ => {}
            }
            for (tool_index, tool) in message.tool_calls.into_iter().enumerate() {
                collector.push(
                    format!("gemini-{index}-tool-{tool_index}"),
                    CliSessionMessageRole::Tool,
                    tool.content,
                    timestamp.clone(),
                    model.clone(),
                    Some(tool.name),
                );
            }
        }
        let (messages, truncated, omitted_message_count) =
            collector.finish(source_truncated);
        return Ok(CliSessionDetail {
            session: summary,
            messages,
            truncated,
            omitted_message_count,
            content_source: "geminiTranscript".to_string(),
        });
    }
    Err("未找到指定的 Gemini CLI 会话".to_string())
}

pub(super) fn search(
    cli_kind: AgentCliKind,
    workdir: &Path,
    session_id: &str,
    request: &SessionContentSearchRequest,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String> {
    let config_dir = super::config::config_dir()
        .ok_or_else(|| "无法定位用户目录，无法读取 Gemini CLI 历史会话".to_string())?;
    for path in chat_files(&config_dir, workdir) {
        let Some(summary) = parse_session(cli_kind, &path, workdir)? else {
            continue;
        };
        if summary.id == session_id {
            return search_conversation(&path, request, is_current);
        }
    }
    Err("未找到指定的 Gemini CLI 会话".to_string())
}

pub(super) fn index(
    cli_kind: AgentCliKind,
    workdir: &Path,
    session_id: &str,
    known_fingerprint: Option<&str>,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionIndexLoadResult, String> {
    let config_dir = super::config::config_dir()
        .ok_or_else(|| "无法定位用户目录，无法读取 Gemini CLI 历史会话".to_string())?;
    for path in chat_files(&config_dir, workdir) {
        let Some(summary) = parse_session(cli_kind, &path, workdir)? else {
            continue;
        };
        if summary.id == session_id {
            return index_conversation(&path, known_fingerprint, is_current);
        }
    }
    Err("未找到指定的 Gemini CLI 会话".to_string())
}

fn index_conversation(
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
    let mut conversation = IndexConversation::default();
    scan_json_records_background(path, "索引 Gemini CLI 会话正文", is_current, |_line_index, line| {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return false;
        };
        conversation.observe(&value);
        false
    })?;
    Ok(SessionIndexLoadResult::Updated {
        fingerprint,
        source_bytes,
        messages: conversation.finish(),
    })
}

fn search_conversation(
    path: &Path,
    request: &SessionContentSearchRequest,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String> {
    let mut conversation = SearchConversation::default();
    scan_json_records(
        path,
        "检索 Gemini CLI 会话正文",
        is_current,
        |_line_index, line| {
            let Ok(value) = serde_json::from_slice::<Value>(line) else {
                return false;
            };
            conversation.observe(&value, request);
            false
        },
    )?;
    Ok(conversation.finish())
}

fn chat_files(config_dir: &Path, workdir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for project_id in project_ids(config_dir, workdir) {
        let chats_dir = config_dir.join("tmp").join(project_id).join("chats");
        let Ok(entries) = fs::read_dir(&chats_dir) else {
            continue;
        };
        files.extend(entries.flatten().map(|entry| entry.path()).filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| matches!(extension, "json" | "jsonl"))
        }));
    }
    files
}

fn list_from_config_dir(
    cli_kind: AgentCliKind,
    config_dir: &Path,
    workdir: &Path,
) -> Result<Vec<CliSessionSummary>, String> {
    let files = chat_files(config_dir, workdir);

    let mut sessions = Vec::<CliSessionSummary>::new();
    let mut indexes = HashMap::<String, usize>::new();
    let mut last_error = None;
    let mut failed_files = 0usize;
    for path in &files {
        match parse_session(cli_kind, path, workdir) {
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
        return Err(last_error.unwrap_or_else(|| "读取 Gemini CLI 历史会话失败".to_string()));
    }
    Ok(sessions)
}

fn project_ids(config_dir: &Path, workdir: &Path) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let expected = path_key(workdir);
    if let Ok(file) = File::open(config_dir.join("projects.json")) {
        if let Ok(value) = serde_json::from_reader::<_, Value>(file) {
            if let Some(projects) = value.get("projects").and_then(Value::as_object) {
                for (project_path, project_id) in projects {
                    if path_key(Path::new(project_path)) == expected {
                        if let Some(project_id) = project_id
                            .as_str()
                            .map(str::trim)
                            .filter(|project_id| !project_id.is_empty())
                        {
                            ids.insert(project_id.to_string());
                        }
                    }
                }
            }
        }
    }

    let tmp_dir = config_dir.join("tmp");
    if let Ok(entries) = fs::read_dir(&tmp_dir) {
        for entry in entries.flatten().filter(|entry| entry.path().is_dir()) {
            let marker = entry.path().join(".project_root");
            let Ok(project_root) = fs::read_to_string(marker) else {
                continue;
            };
            if path_key(Path::new(project_root.trim())) == expected {
                if let Some(project_id) = entry.file_name().to_str() {
                    ids.insert(project_id.to_string());
                }
            }
        }
    }

    // 兼容 Gemini CLI 自己仍会迁移的旧 SHA-256 项目目录。这里仅只读探测，
    // 不创建 projects.json，也不移动官方状态文件。
    for path in [workdir.to_path_buf(), canonical_or_original(workdir)] {
        let project_id = format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()));
        if tmp_dir.join(&project_id).join("chats").is_dir() {
            ids.insert(project_id);
        }
    }
    ids
}

fn parse_session(
    cli_kind: AgentCliKind,
    path: &Path,
    workdir: &Path,
) -> Result<Option<CliSessionSummary>, String> {
    let file = File::open(path)
        .map_err(|err| format!("打开 Gemini CLI 会话记录失败：{}：{err}", path.display()))?;
    let mut conversation = ConversationSummary::default();
    for line in BufReader::new(file).lines() {
        let line =
            line.map_err(|err| format!("读取 Gemini CLI 会话记录失败：{}：{err}", path.display()))?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        conversation.observe(&value);
    }

    // 旧版本可能把完整会话保存为单个 JSON 对象；JSONL 解析没有拿到会话 ID
    // 时再走一次兼容读取，不影响当前增量格式的流式内存占用。
    if conversation.session_id.is_none() {
        if let Ok(file) = File::open(path) {
            if let Ok(value) = serde_json::from_reader::<_, Value>(file) {
                conversation.observe(&value);
            }
        }
    }

    if conversation.kind.as_deref() == Some("subagent") {
        return Ok(None);
    }
    let Some(id) = conversation
        .session_id
        .take()
        .filter(|id| !id.trim().is_empty())
    else {
        return Ok(None);
    };
    if !conversation
        .messages
        .iter()
        .any(|message| message.resumable)
    {
        return Ok(None);
    }

    let first_user_message = conversation
        .messages
        .iter()
        .find(|message| message.kind == "user" && message.resumable)
        .and_then(|message| message.text.clone());
    let title = first_non_empty([
        conversation
            .summary
            .and_then(|value| clean_text(value, 100)),
        first_user_message
            .clone()
            .and_then(|value| clean_text(value, 100)),
    ]);
    let preview = first_user_message.and_then(|value| clean_text(value, 240));
    let mut models = BTreeSet::new();
    let mut model = None;
    for message in &conversation.messages {
        if message.kind != "gemini" {
            continue;
        }
        if let Some(value) = message
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            models.insert(value.to_string());
            model = Some(value.to_string());
        }
    }
    let file_timestamp = modified_timestamp(path);
    let created_at =
        normalize_timestamp(conversation.start_time.as_deref()).or_else(|| file_timestamp.clone());
    let updated_at = normalize_timestamp(conversation.last_updated.as_deref()).or(file_timestamp);

    Ok(Some(CliSessionSummary {
        id,
        title,
        preview,
        model,
        models: models.into_iter().collect(),
        cli_kind,
        created_at,
        updated_at,
        workdir: workdir.to_string_lossy().to_string(),
        cli_version: None,
        archived: false,
        can_resume: true,
        metadata_source: "geminiTranscript".to_string(),
    }))
}

fn load_conversation_limited(
    path: &Path,
    limits: SessionReadLimits,
) -> Result<(ConversationSummary, bool), String> {
    let mut conversation = ConversationSummary::default();
    let source_truncated = read_json_lines_limited(
        path,
        limits.max_file_bytes,
        "读取 Gemini CLI 会话正文",
        |_line_index, value| conversation.observe(&value),
    )?;
    if conversation.session_id.is_none() {
        if let Ok(file) = File::open(path) {
            if let Ok(value) = serde_json::from_reader::<_, Value>(file) {
                conversation.observe(&value);
            }
        }
    }
    Ok((conversation, source_truncated))
}

#[derive(Default)]
struct ConversationSummary {
    session_id: Option<String>,
    summary: Option<String>,
    start_time: Option<String>,
    last_updated: Option<String>,
    kind: Option<String>,
    messages: Vec<MessageSummary>,
    message_positions: HashMap<String, usize>,
}

impl ConversationSummary {
    fn observe(&mut self, value: &Value) {
        if let Some(rewind_id) = value.get("$rewindTo").and_then(Value::as_str) {
            self.rewind_to(rewind_id);
            return;
        }
        if let Some(updates) = value.get("$set").and_then(Value::as_object) {
            if let Some(messages) = updates.get("messages").and_then(Value::as_array) {
                self.replace_messages(messages);
            }
            self.observe_metadata(&Value::Object(updates.clone()));
            return;
        }
        if value.get("id").and_then(Value::as_str).is_some() {
            self.upsert_message(value);
            return;
        }
        self.observe_metadata(value);
        if let Some(messages) = value.get("messages").and_then(Value::as_array) {
            self.replace_messages(messages);
        }
    }

    fn observe_metadata(&mut self, value: &Value) {
        replace_string(&mut self.session_id, value, "sessionId");
        replace_string(&mut self.summary, value, "summary");
        replace_string(&mut self.start_time, value, "startTime");
        replace_string(&mut self.last_updated, value, "lastUpdated");
        replace_string(&mut self.kind, value, "kind");
    }

    fn replace_messages(&mut self, messages: &[Value]) {
        self.messages.clear();
        self.message_positions.clear();
        for message in messages {
            self.upsert_message(message);
        }
    }

    fn upsert_message(&mut self, value: &Value) {
        let Some(message) = MessageSummary::from_value(value, ToolContentMode::Display) else {
            return;
        };
        if let Some(index) = self.message_positions.get(&message.id).copied() {
            self.messages[index] = message;
        } else {
            self.message_positions
                .insert(message.id.clone(), self.messages.len());
            self.messages.push(message);
        }
    }

    fn rewind_to(&mut self, message_id: &str) {
        let Some(index) = self.message_positions.get(message_id).copied() else {
            self.messages.clear();
            self.message_positions.clear();
            return;
        };
        for message in self.messages.drain(index..) {
            self.message_positions.remove(&message.id);
        }
    }
}

struct MessageSummary {
    id: String,
    kind: String,
    text: Option<String>,
    model: Option<String>,
    resumable: bool,
    timestamp: Option<String>,
    tool_calls: Vec<ToolSummary>,
}

#[derive(Default)]
struct SearchConversation {
    messages: Vec<SearchMessage>,
    message_positions: HashMap<String, usize>,
}

struct SearchMessage {
    id: String,
    result: SessionContentSearchResult,
}

#[derive(Default)]
struct IndexConversation {
    messages: Vec<SessionIndexMessage>,
    message_positions: HashMap<String, usize>,
}

impl IndexConversation {
    fn observe(&mut self, value: &Value) {
        if let Some(rewind_id) = value.get("$rewindTo").and_then(Value::as_str) {
            self.rewind_to(rewind_id);
            return;
        }
        if let Some(updates) = value.get("$set").and_then(Value::as_object) {
            if let Some(messages) = updates.get("messages").and_then(Value::as_array) {
                self.replace_messages(messages);
            }
            return;
        }
        if value.get("id").and_then(Value::as_str).is_some() {
            self.upsert_message(value);
            return;
        }
        if let Some(messages) = value.get("messages").and_then(Value::as_array) {
            self.replace_messages(messages);
        }
    }

    fn replace_messages(&mut self, messages: &[Value]) {
        self.messages.clear();
        self.message_positions.clear();
        for message in messages {
            self.upsert_message(message);
        }
    }

    fn upsert_message(&mut self, value: &Value) {
        let Some(message) = MessageSummary::from_value(value, ToolContentMode::Ignore) else {
            return;
        };
        let role = match message.kind.as_str() {
            "user" => CliSessionMessageRole::User,
            "gemini" => CliSessionMessageRole::Assistant,
            _ => return,
        };
        let Some(content) = message
            .text
            .map(|value| value.trim().to_string())
            .filter(|value| {
                !value.is_empty()
                    && (role != CliSessionMessageRole::User
                        || !is_ignored_user_content(value))
            })
        else {
            return;
        };
        let item = SessionIndexMessage {
            id: format!("gemini-{}", message.id),
            role,
            content,
        };
        if let Some(index) = self.message_positions.get(&message.id).copied() {
            self.messages[index] = item;
        } else {
            self.message_positions
                .insert(message.id, self.messages.len());
            self.messages.push(item);
        }
    }

    fn rewind_to(&mut self, message_id: &str) {
        let Some(index) = self.message_positions.get(message_id).copied() else {
            self.messages.clear();
            self.message_positions.clear();
            return;
        };
        let removed = self
            .message_positions
            .iter()
            .filter_map(|(id, position)| (*position >= index).then_some(id.clone()))
            .collect::<Vec<_>>();
        self.messages.truncate(index);
        for id in removed {
            self.message_positions.remove(&id);
        }
    }

    fn finish(self) -> Vec<SessionIndexMessage> {
        self.messages
    }
}

impl SearchConversation {
    fn observe(&mut self, value: &Value, request: &SessionContentSearchRequest) {
        if let Some(rewind_id) = value.get("$rewindTo").and_then(Value::as_str) {
            self.rewind_to(rewind_id);
            return;
        }
        if let Some(updates) = value.get("$set").and_then(Value::as_object) {
            if let Some(messages) = updates.get("messages").and_then(Value::as_array) {
                self.replace_messages(messages, request);
            }
            return;
        }
        if value.get("id").and_then(Value::as_str).is_some() {
            self.upsert_message(value, request);
            return;
        }
        if let Some(messages) = value.get("messages").and_then(Value::as_array) {
            self.replace_messages(messages, request);
        }
    }

    fn replace_messages(&mut self, messages: &[Value], request: &SessionContentSearchRequest) {
        self.messages.clear();
        self.message_positions.clear();
        for message in messages {
            self.upsert_message(message, request);
        }
    }

    fn upsert_message(&mut self, value: &Value, request: &SessionContentSearchRequest) {
        let Some(message) = SearchMessage::from_value(value, request) else {
            return;
        };
        if let Some(index) = self.message_positions.get(&message.id).copied() {
            self.messages[index] = message;
        } else {
            self.message_positions
                .insert(message.id.clone(), self.messages.len());
            self.messages.push(message);
        }
    }

    fn rewind_to(&mut self, message_id: &str) {
        let Some(index) = self.message_positions.get(message_id).copied() else {
            self.messages.clear();
            self.message_positions.clear();
            return;
        };
        for message in self.messages.drain(index..) {
            self.message_positions.remove(&message.id);
        }
    }

    fn finish(self) -> SessionContentSearchResult {
        combine_content_search_results(self.messages.into_iter().map(|message| message.result))
    }
}

impl SearchMessage {
    fn from_value(value: &Value, request: &SessionContentSearchRequest) -> Option<Self> {
        let message = MessageSummary::from_value(value, ToolContentMode::Ignore)?;
        let mut collector = SessionContentSearchCollector::new(request);
        let role = match message.kind.as_str() {
            "user" => Some(CliSessionMessageRole::User),
            "gemini" => Some(CliSessionMessageRole::Assistant),
            _ => None,
        };
        if let (Some(role), Some(text)) = (role, message.text) {
            if role != CliSessionMessageRole::User || !is_ignored_user_content(text.trim()) {
                collector.observe(&text);
            }
        }
        Some(Self {
            id: message.id,
            result: collector.finish(),
        })
    }
}

struct ToolSummary {
    name: String,
    content: String,
}

impl MessageSummary {
    fn from_value(value: &Value, tool_content_mode: ToolContentMode) -> Option<Self> {
        let id = value.get("id")?.as_str()?.trim();
        if id.is_empty() {
            return None;
        }
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let text = value.get("content").and_then(content_text);
        let tool_calls = value
            .get("toolCalls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|tool| ToolSummary {
                name: tool
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("工具")
                    .to_string(),
                content: match tool_content_mode {
                    ToolContentMode::Display => compact_json(tool, 16 * 1024),
                    ToolContentMode::Ignore => String::new(),
                },
            })
            .filter(|tool| tool_content_mode != ToolContentMode::Ignore || !tool.content.is_empty())
            .collect::<Vec<_>>();
        let resumable = match kind.as_str() {
            "user" => text
                .as_deref()
                .map(str::trim)
                .is_some_and(|text| !is_ignored_user_content(text)),
            "gemini" => {
                text.as_deref().is_some_and(|text| !text.trim().is_empty())
                    || non_empty_array(value, "toolCalls")
                    || non_empty_array(value, "thoughts")
            }
            _ => false,
        };
        Some(Self {
            id: id.to_string(),
            kind,
            text,
            model: value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string),
            resumable,
            timestamp: value
                .get("timestamp")
                .and_then(Value::as_str)
                .map(str::to_string),
            tool_calls,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolContentMode {
    Display,
    Ignore,
}

fn replace_string(target: &mut Option<String>, value: &Value, field: &str) {
    if let Some(value) = value.get(field).and_then(Value::as_str) {
        *target = Some(value.to_string());
    }
}

fn content_text(value: &Value) -> Option<String> {
    let mut parts = Vec::new();
    collect_text(value, &mut parts);
    let text = parts.join(" ");
    (!text.trim().is_empty()).then_some(text)
}

fn collect_text(value: &Value, parts: &mut Vec<String>) {
    match value {
        Value::String(value) => parts.push(value.to_string()),
        Value::Array(values) => {
            for value in values {
                collect_text(value, parts);
            }
        }
        Value::Object(value) => {
            if value
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(is_hidden_content_kind)
            {
                return;
            }
            if let Some(text) = value.get("text").and_then(Value::as_str) {
                parts.push(text.to_string());
            }
        }
        _ => {}
    }
}

fn is_hidden_content_kind(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    ["tool", "thought", "thinking", "reasoning", "analysis"]
        .iter()
        .any(|kind| value.contains(kind))
}

fn is_ignored_user_content(value: &str) -> bool {
    value.is_empty()
        || value.starts_with('/')
        || value.starts_with('?')
        || value.starts_with("<session_context>")
        || value.starts_with("<hook_context>")
}

fn non_empty_array(value: &Value, field: &str) -> bool {
    value
        .get(field)
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
}

fn modified_timestamp(path: &Path) -> Option<String> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(chrono::DateTime::<chrono::Utc>::from)
        .map(|timestamp| timestamp.to_rfc3339())
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn path_key(path: &Path) -> String {
    let mut value = canonical_or_original(path)
        .to_string_lossy()
        .replace('\\', "/");
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        value.make_ascii_lowercase();
    }
    value
}

#[cfg(test)]
mod tests;
