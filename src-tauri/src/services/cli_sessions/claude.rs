use super::{
    collect_jsonl_files, fallback_timestamp, for_each_json_line, home_dir, same_workdir,
    string_field, text_value, timestamp_field, valid_model,
};
use crate::models::{CliSessionSummary, LivenessCliKind};
use serde_json::Value;
use std::{path::Path, time::SystemTime};

pub(super) fn scan(workdir: &Path) -> Vec<CliSessionSummary> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    let root = home.join(".claude").join("projects");
    if !root.is_dir() {
        return Vec::new();
    }
    collect_jsonl_files(&[root])
        .into_iter()
        .filter_map(|candidate| parse_file(&candidate.path, candidate.modified, workdir))
        .collect()
}

fn parse_file(
    path: &Path,
    modified: Option<SystemTime>,
    workdir: &Path,
) -> Option<CliSessionSummary> {
    let mut id = None;
    let mut cwd = None;
    let mut title = None;
    let mut model = None;
    let mut created_at = None;
    let mut updated_at = None;
    let mut sidechain = None;
    let file_modified = for_each_json_line(path, |value| {
        if sidechain.is_none() {
            sidechain = value.get("isSidechain").and_then(Value::as_bool);
        }
        id = id
            .take()
            .or_else(|| string_field(value, "sessionId").map(str::to_string));
        cwd = cwd
            .take()
            .or_else(|| string_field(value, "cwd").map(str::to_string));
        let record_timestamp = timestamp_field(value);
        if record_timestamp.is_some() {
            updated_at = record_timestamp;
        }
        if created_at.is_none() {
            created_at = updated_at.clone();
        }

        if let Some(next_model) = valid_model(
            value
                .get("message")
                .and_then(|message| message.get("model")),
        ) {
            model = Some(next_model);
        }
        if title.is_none() {
            title = string_field(value, "aiTitle").map(str::to_string);
        }
        if title.is_none()
            && string_field(value, "type") == Some("user")
            && value.get("isMeta").and_then(Value::as_bool) != Some(true)
            && value.get("isSidechain").and_then(Value::as_bool) != Some(true)
        {
            title = value
                .get("message")
                .and_then(|message| text_value(message.get("content")));
        }
    })
    .or(modified);

    if sidechain == Some(true) {
        return None;
    }
    let id = id.or_else(|| {
        path.file_stem()
            .and_then(|name| name.to_str())
            .map(str::to_string)
    })?;
    let cwd = cwd?;
    if !same_workdir(&cwd, workdir) {
        return None;
    }
    let fallback = file_modified.and_then(fallback_timestamp);
    Some(CliSessionSummary {
        id,
        title: title.unwrap_or_else(|| "未命名会话".to_string()),
        model,
        cli_kind: LivenessCliKind::ClaudeCode,
        created_at: created_at.or(fallback.clone()),
        updated_at: updated_at.or(fallback),
    })
}

#[cfg(test)]
mod tests {
    use super::parse_file;
    use serde_json::json;
    use std::{fs, path::PathBuf, time::SystemTime};

    fn fixture_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "balancehub-cli-session-claude-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn reads_claude_session_metadata_and_ignores_synthetic_model() {
        let root = fixture_root();
        let workdir = root.join("workspace");
        let file = root.join("session-1.jsonl");
        fs::create_dir_all(&workdir).unwrap();
        let records = [
            json!({
                "type": "user",
                "sessionId": "session-1",
                "cwd": workdir,
                "timestamp": "2026-08-04T01:00:00Z",
                "message": { "content": "继续实现" }
            }),
            json!({
                "type": "assistant",
                "sessionId": "session-1",
                "cwd": workdir,
                "timestamp": "2026-08-04T01:01:00Z",
                "message": { "model": "<synthetic>" }
            }),
            json!({
                "type": "assistant",
                "sessionId": "session-1",
                "cwd": workdir,
                "timestamp": "2026-08-04T01:02:00Z",
                "message": { "model": "claude-sonnet-4-5" }
            }),
        ];
        fs::write(
            &file,
            records
                .iter()
                .map(serde_json::to_string)
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("\n")
                + "\n",
        )
        .unwrap();

        let session = parse_file(&file, None, &workdir).unwrap();
        assert_eq!(session.id, "session-1");
        assert_eq!(session.title, "继续实现");
        assert_eq!(session.model.as_deref(), Some("claude-sonnet-4-5"));

        let _ = fs::remove_dir_all(root);
    }
}
