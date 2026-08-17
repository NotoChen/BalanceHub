use crate::services::cli_sessions::{
    clean_text, first_non_empty, normalize_timestamp, session_sort_key,
};
use crate::models::{AgentCliKind, CliSessionSummary};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

pub(super) fn list(
    cli_kind: AgentCliKind,
    workdir: &Path,
) -> Result<Vec<CliSessionSummary>, String> {
    let config_dir = super::config::config_dir()
        .ok_or_else(|| "无法定位用户目录，无法读取 Gemini CLI 历史会话".to_string())?;
    list_from_config_dir(cli_kind, &config_dir, workdir)
}

fn list_from_config_dir(
    cli_kind: AgentCliKind,
    config_dir: &Path,
    workdir: &Path,
) -> Result<Vec<CliSessionSummary>, String> {
    let project_ids = project_ids(config_dir, workdir);
    if project_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for project_id in project_ids {
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
        let Some(message) = MessageSummary::from_value(value) else {
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
}

impl MessageSummary {
    fn from_value(value: &Value) -> Option<Self> {
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
        })
    }
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
            if let Some(text) = value.get("text").and_then(Value::as_str) {
                parts.push(text.to_string());
            }
        }
        _ => {}
    }
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
mod tests {
    use super::list_from_config_dir;
    use crate::models::AgentCliKind;
    use serde_json::json;
    use std::fs;

    fn test_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "balancehub-gemini-session-{name}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn official_jsonl_metadata_drives_title_model_and_resume_id() {
        let root = test_root("metadata");
        let _ = fs::remove_dir_all(&root);
        let workdir = root.join("workspace");
        let chats = root.join("config/tmp/workspace/chats");
        fs::create_dir_all(&workdir).unwrap();
        fs::create_dir_all(&chats).unwrap();
        fs::write(
            root.join("config/projects.json"),
            json!({"projects": {workdir.to_string_lossy(): "workspace"}}).to_string(),
        )
        .unwrap();
        let lines = [
            json!({
                "sessionId": "019c-gemini-session",
                "projectHash": "hash",
                "startTime": "2026-08-14T08:00:00Z",
                "lastUpdated": "2026-08-14T08:01:00Z"
            }),
            json!({"id": "user-1", "type": "user", "content": [{"text": "接入 Gemini CLI"}]}),
            json!({"id": "gemini-1", "type": "gemini", "content": "处理中", "model": "gemini-2.5-pro"}),
            json!({"id": "gemini-1", "type": "gemini", "content": "完成", "model": "gemini-3-pro"}),
            json!({"$set": {"summary": "BalanceHub Gemini 接入", "lastUpdated": "2026-08-14T08:02:00Z"}}),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(chats.join("session.jsonl"), lines).unwrap();

        let sessions = list_from_config_dir(
            AgentCliKind::Gemini,
            &root.join("config"),
            &workdir,
        )
        .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "019c-gemini-session");
        assert_eq!(sessions[0].title, "BalanceHub Gemini 接入");
        assert_eq!(sessions[0].preview.as_deref(), Some("接入 Gemini CLI"));
        assert_eq!(sessions[0].model.as_deref(), Some("gemini-3-pro"));
        assert_eq!(sessions[0].models.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rewinds_and_ignored_commands_do_not_create_wrong_titles() {
        let root = test_root("rewind");
        let _ = fs::remove_dir_all(&root);
        let workdir = root.join("workspace");
        let project = root.join("config/tmp/workspace");
        let chats = project.join("chats");
        fs::create_dir_all(&workdir).unwrap();
        fs::create_dir_all(&chats).unwrap();
        fs::write(
            project.join(".project_root"),
            workdir.to_string_lossy().as_bytes(),
        )
        .unwrap();
        let lines = [
            json!({"sessionId": "rewind-session", "projectHash": "hash"}),
            json!({"id": "command", "type": "user", "content": "/help"}),
            json!({"id": "discarded", "type": "user", "content": "应该被回退"}),
            json!({"$rewindTo": "discarded"}),
            json!({"id": "kept", "type": "user", "content": "最终问题"}),
            json!({"id": "answer", "type": "gemini", "content": "完成", "model": "gemini-2.5-flash"}),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(chats.join("session.jsonl"), lines).unwrap();

        let sessions = list_from_config_dir(
            AgentCliKind::Gemini,
            &root.join("config"),
            &workdir,
        )
        .unwrap();
        assert_eq!(sessions[0].title, "最终问题");
        assert_eq!(sessions[0].preview.as_deref(), Some("最终问题"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn subagent_and_empty_sessions_are_filtered() {
        let root = test_root("filtered");
        let _ = fs::remove_dir_all(&root);
        let workdir = root.join("workspace");
        let chats = root.join("config/tmp/workspace/chats");
        fs::create_dir_all(&workdir).unwrap();
        fs::create_dir_all(&chats).unwrap();
        fs::write(
            root.join("config/projects.json"),
            json!({"projects": {workdir.to_string_lossy(): "workspace"}}).to_string(),
        )
        .unwrap();
        fs::write(
            chats.join("subagent.jsonl"),
            concat!(
                "{\"sessionId\":\"subagent\",\"projectHash\":\"hash\",\"kind\":\"subagent\"}\n",
                "{\"id\":\"user\",\"type\":\"user\",\"content\":\"hidden\"}\n"
            ),
        )
        .unwrap();
        fs::write(
            chats.join("empty.jsonl"),
            "{\"sessionId\":\"empty\",\"projectHash\":\"hash\"}\n",
        )
        .unwrap();

        assert!(list_from_config_dir(
            AgentCliKind::Gemini,
            &root.join("config"),
            &workdir
        )
            .unwrap()
            .is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
