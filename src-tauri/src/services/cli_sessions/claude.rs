use super::{clean_text, first_non_empty, normalize_timestamp};
use crate::models::{CliSessionMetadataSource, CliSessionSummary, LivenessCliKind};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    env,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

pub(super) fn list(workdir: &Path) -> Result<Vec<CliSessionSummary>, String> {
    let projects = home_dir()
        .map(|home| home.join(".claude").join("projects"))
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
        match parse_transcript(path, workdir) {
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

fn parse_transcript(
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
        cli_kind: LivenessCliKind::ClaudeCode,
        created_at: normalize_timestamp(summary.created_at.as_deref()),
        updated_at: normalize_timestamp(summary.updated_at.as_deref()),
        workdir,
        cli_version: summary.cli_version.and_then(|value| clean_text(value, 50)),
        archived: false,
        can_resume: true,
        metadata_source: CliSessionMetadataSource::ClaudeTranscript,
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

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from).or_else(|| {
            let drive = env::var_os("HOMEDRIVE")?;
            let path = env::var_os("HOMEPATH")?;
            Some(PathBuf::from(drive).join(path))
        })
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::{encode_project_path, message_text, parse_transcript};
    use serde_json::json;
    use std::{fs, path::Path};

    #[test]
    fn project_path_encoding_matches_claude_layout() {
        assert_eq!(
            encode_project_path(Path::new("/Users/example/project")),
            "-Users-example-project"
        );
    }

    #[test]
    fn message_text_handles_string_and_content_blocks() {
        assert_eq!(
            message_text(&json!({"message": {"content": "hello"}})).as_deref(),
            Some("hello")
        );
        assert_eq!(
            message_text(&json!({
                "message": {"content": [{"type": "text", "text": "hello"}, {"text": "world"}]}
            }))
            .as_deref(),
            Some("hello world")
        );
    }

    #[test]
    fn message_text_ignores_tool_results() {
        assert_eq!(
            message_text(&json!({
                "message": {
                    "content": [{"type": "tool_result", "content": "command output"}]
                }
            })),
            None
        );
        assert_eq!(
            message_text(&json!({
                "message": {
                    "content": [
                        {"type": "tool_result", "content": "command output"},
                        {"type": "text", "text": "actual request"}
                    ]
                }
            }))
            .as_deref(),
            Some("actual request")
        );
    }

    #[test]
    fn transcript_uses_the_first_real_user_message() {
        let root = std::env::temp_dir().join(format!(
            "balancehub-cli-session-claude-user-message-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        let lines = [
            json!({
                "type": "user",
                "sessionId": "session-1",
                "cwd": root.to_string_lossy(),
                "message": {"content": [{"type": "tool_result", "content": "command output"}]}
            }),
            json!({
                "type": "user",
                "isMeta": true,
                "message": {"content": "internal context"}
            }),
            json!({
                "type": "user",
                "message": {"content": "actual request"}
            }),
        ]
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&transcript, lines).unwrap();
        let summary = parse_transcript(&transcript, &root).unwrap().unwrap();
        assert_eq!(summary.preview.as_deref(), Some("actual request"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transcript_without_cwd_uses_the_selected_project_path() {
        let root = std::env::temp_dir().join(format!(
            "balancehub-cli-session-claude-workdir-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        fs::write(
            &transcript,
            json!({
                "type": "user",
                "sessionId": "session-1",
                "message": {"content": "actual request"}
            })
            .to_string(),
        )
        .unwrap();
        let summary = parse_transcript(&transcript, &root).unwrap().unwrap();
        assert_eq!(summary.workdir, root.to_string_lossy());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transcript_prefers_latest_ai_title_and_collects_models() {
        let root = std::env::temp_dir().join(format!(
            "balancehub-cli-session-claude-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        let lines = [
            json!({
                "type": "user",
                "sessionId": "session-1",
                "cwd": root.to_string_lossy(),
                "timestamp": "2026-08-06T08:00:00Z",
                "message": {"content": "first request"}
            }),
            json!({
                "type": "assistant",
                "timestamp": "2026-08-06T08:01:00Z",
                "message": {"model": "claude-sonnet-4-5"}
            }),
            json!({
                "type": "ai-title",
                "aiTitle": "修复历史会话",
                "timestamp": "2026-08-06T08:02:00Z"
            }),
            json!({
                "type": "assistant",
                "timestamp": "2026-08-06T08:03:00Z",
                "message": {"model": "claude-opus-4-1"}
            }),
        ]
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&transcript, lines).unwrap();
        let summary = parse_transcript(&transcript, &root).unwrap().unwrap();
        assert_eq!(summary.id, "session-1");
        assert_eq!(summary.title, "修复历史会话");
        assert_eq!(summary.preview.as_deref(), Some("first request"));
        assert_eq!(summary.model.as_deref(), Some("claude-opus-4-1"));
        assert_eq!(summary.models.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }
}
