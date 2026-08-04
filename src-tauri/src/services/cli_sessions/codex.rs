use super::{
    collect_jsonl_files, fallback_timestamp, for_each_json_line, home_dir, same_workdir,
    string_field, text_value, timestamp_field, valid_model,
};
use crate::models::{CliSessionSummary, LivenessCliKind};
use std::{path::Path, time::SystemTime};

pub(super) fn scan(workdir: &Path) -> Vec<CliSessionSummary> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    let roots = [
        home.join(".codex").join("sessions"),
        home.join(".codex").join("archived_sessions"),
    ]
    .into_iter()
    .filter(|path| path.is_dir())
    .collect::<Vec<_>>();
    let files = collect_jsonl_files(&roots);
    let mut sessions = Vec::new();
    for candidate in files {
        if let Some(session) = parse_file(&candidate.path, candidate.modified, workdir) {
            sessions.push(session);
        }
    }
    sessions
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

    let file_modified = for_each_json_line(path, |value| {
        let record_type = string_field(value, "type").unwrap_or_default();
        let record_timestamp = timestamp_field(value);
        if record_timestamp.is_some() {
            updated_at = record_timestamp.clone();
        }
        match record_type {
            "session_meta" => {
                if let Some(payload) = value.get("payload") {
                    id = string_field(payload, "id")
                        .or_else(|| string_field(payload, "session_id"))
                        .map(str::to_string);
                    cwd = string_field(payload, "cwd").map(str::to_string);
                    created_at = string_field(payload, "timestamp").map(str::to_string);
                    model = valid_model(payload.get("model"));
                }
            }
            "event_msg" => {
                let payload = value.get("payload");
                if payload.and_then(|item| string_field(item, "type")) == Some("user_message")
                    && title.is_none()
                {
                    title = payload.and_then(|item| text_value(item.get("message")));
                }
            }
            "turn_context" => {
                if let Some(payload) = value.get("payload") {
                    if let Some(next_model) = valid_model(payload.get("model")) {
                        model = Some(next_model);
                    }
                    if cwd.is_none() {
                        cwd = string_field(payload, "cwd").map(str::to_string);
                    }
                }
            }
            "response_item" => {
                if title.is_none()
                    && value
                        .get("payload")
                        .and_then(|payload| string_field(payload, "role"))
                        == Some("user")
                {
                    title = value
                        .get("payload")
                        .and_then(|payload| text_value(payload.get("content")));
                }
            }
            _ => {
                if cwd.is_none() {
                    cwd = string_field(value, "cwd").map(str::to_string);
                }
            }
        }
    })
    .or(modified);

    let id = id.or_else(|| {
        path.file_stem()
            .and_then(|name| name.to_str())
            .map(|name| name.strip_prefix("rollout-").unwrap_or(name).to_string())
    })?;
    let cwd = cwd?;
    if !same_workdir(&cwd, workdir) {
        return None;
    }
    let fallback = file_modified.and_then(fallback_timestamp);
    let updated_at = updated_at.or(fallback.clone());
    Some(CliSessionSummary {
        id,
        title: title.unwrap_or_else(|| "未命名会话".to_string()),
        model,
        cli_kind: LivenessCliKind::Codex,
        created_at: created_at.or(fallback),
        updated_at,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_file;
    use serde_json::json;
    use std::{fs, path::PathBuf, time::SystemTime};

    fn fixture_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "balancehub-cli-session-codex-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn reads_codex_metadata_for_matching_workdir() {
        let root = fixture_root();
        let workdir = root.join("workspace");
        let file = root.join("rollout-abc-123.jsonl");
        fs::create_dir_all(&workdir).unwrap();
        let records = [
            json!({
                "type": "session_meta",
                "payload": {
                    "id": "abc-123",
                    "cwd": workdir,
                    "timestamp": "2026-08-04T01:00:00Z"
                }
            }),
            json!({
                "type": "turn_context",
                "payload": { "model": "gpt-5.6" }
            }),
            json!({
                "type": "event_msg",
                "payload": { "type": "user_message", "message": "继续修复" }
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
        assert_eq!(session.id, "abc-123");
        assert_eq!(session.title, "继续修复");
        assert_eq!(session.model.as_deref(), Some("gpt-5.6"));

        let _ = fs::remove_dir_all(root);
    }
}
