use crate::{
    limits,
    models::{AgentCliKind, CliSessionSummary},
    services::cli_sessions::{
        clean_text, first_non_empty, normalize_timestamp, session_sort_key,
    },
    util::read_text_file_limited,
};
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

const MAX_SCAN_DIRECTORIES: usize = 10_000;
const MAX_SUMMARY_FILES: usize = 2_000;
const MAX_SUMMARY_FILE_BYTES: usize = 256 * 1024;

pub(super) fn list(
    cli_kind: AgentCliKind,
    workdir: &Path,
) -> Result<Vec<CliSessionSummary>, String> {
    let grok_home = super::config::config_dir()
        .ok_or_else(|| "无法定位用户目录，无法读取 Grok Build 历史会话".to_string())?;
    list_from_home(cli_kind, &grok_home, workdir)
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
mod tests {
    use super::*;
    use serde_json::json;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "balancehub-grok-session-{name}-{}",
            std::process::id()
        ))
    }

    fn write_summary(root: &Path, name: &str, value: Value) {
        let directory = root.join("grok/sessions/project").join(name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("summary.json"),
            serde_json::to_string_pretty(&value).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn official_summary_fields_drive_title_model_time_and_resume_id() {
        let root = test_root("metadata");
        let _ = fs::remove_dir_all(&root);
        let workdir = root.join("workspace");
        fs::create_dir_all(&workdir).unwrap();
        write_summary(
            &root,
            "session",
            json!({
                "info": {"id": "019c-grok-session", "cwd": workdir},
                "generated_title": "BalanceHub Grok 接入",
                "session_summary": "分析并接入 Grok Build",
                "created_at": "2026-08-14T08:00:00Z",
                "updated_at": "2026-08-14T08:01:00Z",
                "last_active_at": "2026-08-14T08:02:00Z",
                "num_messages": 4,
                "num_chat_messages": 2,
                "current_model_id": "grok-code-fast-1"
            }),
        );

        let sessions = list_from_home(AgentCliKind::Grok, &root.join("grok"), &workdir).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "019c-grok-session");
        assert_eq!(sessions[0].title, "BalanceHub Grok 接入");
        assert_eq!(sessions[0].preview.as_deref(), Some("分析并接入 Grok Build"));
        assert_eq!(sessions[0].model.as_deref(), Some("grok-code-fast-1"));
        assert_eq!(sessions[0].updated_at.as_deref(), Some("2026-08-14T08:02:00+00:00"));
        assert_eq!(sessions[0].metadata_source, "grokSummary");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hidden_subagent_empty_and_other_workspace_sessions_are_filtered() {
        let root = test_root("filtered");
        let _ = fs::remove_dir_all(&root);
        let workdir = root.join("workspace");
        fs::create_dir_all(&workdir).unwrap();
        write_summary(
            &root,
            "hidden",
            json!({
                "info": {"id": "hidden", "cwd": workdir},
                "hidden": true,
                "session_summary": "hidden",
                "num_messages": 1
            }),
        );
        write_summary(
            &root,
            "subagent",
            json!({
                "info": {"id": "subagent", "cwd": workdir},
                "session_kind": "subagent_resume",
                "session_summary": "subagent",
                "num_messages": 1
            }),
        );
        write_summary(
            &root,
            "empty",
            json!({
                "info": {"id": "empty", "cwd": workdir},
                "session_summary": "empty",
                "num_messages": 0,
                "num_chat_messages": 0
            }),
        );
        write_summary(
            &root,
            "other",
            json!({
                "info": {"id": "other", "cwd": root.join("other")},
                "session_summary": "other",
                "num_messages": 1
            }),
        );

        assert!(list_from_home(AgentCliKind::Grok, &root.join("grok"), &workdir)
            .unwrap()
            .is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_session_ids_keep_the_most_recent_summary() {
        let root = test_root("duplicate");
        let _ = fs::remove_dir_all(&root);
        let workdir = root.join("workspace");
        fs::create_dir_all(&workdir).unwrap();
        for (name, title, updated_at) in [
            ("older", "旧标题", "2026-08-14T08:00:00Z"),
            ("newer", "新标题", "2026-08-14T09:00:00Z"),
        ] {
            write_summary(
                &root,
                name,
                json!({
                    "info": {"id": "same-session", "cwd": workdir},
                    "generated_title": title,
                    "updated_at": updated_at,
                    "num_messages": 1,
                    "current_model_id": "grok-code-fast-1"
                }),
            );
        }

        let sessions = list_from_home(AgentCliKind::Grok, &root.join("grok"), &workdir).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "新标题");
        fs::remove_dir_all(root).unwrap();
    }
}
