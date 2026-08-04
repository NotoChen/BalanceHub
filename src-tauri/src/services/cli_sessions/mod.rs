mod claude;
mod codex;

use crate::models::{CliSessionSummary, LivenessCliKind};
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_SESSION_FILES: usize = 500;
const MAX_SESSION_RESULTS: usize = 50;
const MAX_SESSION_FILE_BYTES: u64 = 512 * 1024;
const MAX_SCAN_DEPTH: usize = 6;

/// 读取指定工作目录的 CLI 历史会话。所有解析都在调用方的阻塞线程中执行，
/// 这里只做本地只读扫描，不修改 CLI 的配置或历史文件。
pub fn list(cli_kind: LivenessCliKind, workdir: &Path) -> Result<Vec<CliSessionSummary>, String> {
    if !workdir.is_dir() {
        return Err("工作目录不存在".to_string());
    }

    let workdir = canonical_or_original(workdir);
    let sessions = match cli_kind {
        LivenessCliKind::Codex => codex::scan(&workdir),
        LivenessCliKind::ClaudeCode => claude::scan(&workdir),
    };

    let mut sessions = sessions;
    sessions.retain(|session| valid_resume_id(&session.id));
    sessions.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.id.cmp(&right.id))
    });
    sessions.truncate(MAX_SESSION_RESULTS);
    Ok(sessions)
}

pub fn ensure_resume_id(
    cli_kind: LivenessCliKind,
    workdir: &Path,
    resume_id: &str,
) -> Result<(), String> {
    let resume_id = resume_id.trim();
    if resume_id.is_empty() {
        return Err("继续会话缺少会话 ID".to_string());
    }
    if !valid_resume_id(resume_id) {
        return Err("会话 ID 无效".to_string());
    }
    if list(cli_kind, workdir)?
        .iter()
        .any(|session| session.id == resume_id)
    {
        Ok(())
    } else {
        Err("未找到属于当前工作目录的历史会话，请刷新后重试".to_string())
    }
}

fn valid_resume_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.starts_with('-')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

pub(super) fn home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env::var_os("USERPROFILE")
            .or_else(|| {
                let mut home = env::var_os("HOMEDRIVE")?;
                home.push(env::var_os("HOMEPATH")?);
                Some(home)
            })
            .or_else(|| env::var_os("HOME"))
            .map(PathBuf::from)
    }

    #[cfg(not(target_os = "windows"))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

pub(super) fn canonical_or_original(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn same_workdir(left: &str, expected: &Path) -> bool {
    let left_path = canonical_or_original(Path::new(left));
    let expected = canonical_or_original(expected);
    let left = comparable_path(&left_path);
    let expected = comparable_path(&expected);
    if cfg!(target_os = "windows") {
        left.eq_ignore_ascii_case(&expected)
    } else {
        left == expected
    }
}

fn comparable_path(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('\\', "/");
    while value.len() > 1 && value.ends_with('/') {
        value.pop();
    }
    value
}

#[derive(Debug)]
pub(super) struct FileCandidate {
    pub(super) path: PathBuf,
    pub(super) modified: Option<SystemTime>,
}

pub(super) fn collect_jsonl_files(roots: &[PathBuf]) -> Vec<FileCandidate> {
    let mut files = Vec::new();
    for root in roots {
        collect_jsonl_files_from(root, MAX_SCAN_DEPTH, &mut files);
    }
    files.sort_by_key(|candidate| std::cmp::Reverse(candidate.modified));
    files.truncate(MAX_SESSION_FILES);
    files
}

fn collect_jsonl_files_from(path: &Path, depth: usize, files: &mut Vec<FileCandidate>) {
    if files.len() >= MAX_SESSION_FILES.saturating_mul(2) {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&entry_path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() {
            if entry_path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
            {
                files.push(FileCandidate {
                    path: entry_path,
                    modified: metadata.modified().ok(),
                });
            }
            continue;
        }
        if metadata.is_dir() && depth > 0 {
            // Claude 的 subagents 是独立的侧链，不应混进用户可恢复的主会话列表。
            if entry_path
                .file_name()
                .is_some_and(|name| name == "subagents")
            {
                continue;
            }
            collect_jsonl_files_from(&entry_path, depth - 1, files);
        }
    }
}

pub(super) fn for_each_json_line(
    path: &Path,
    mut visit: impl FnMut(&serde_json::Value),
) -> Option<SystemTime> {
    use std::io::{BufRead, BufReader, Read};

    let file = fs::File::open(path).ok()?;
    let modified = file
        .metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok());
    let mut reader = BufReader::new(file.take(MAX_SESSION_FILE_BYTES.saturating_add(1)));
    let mut line = String::new();
    let mut consumed = 0u64;
    loop {
        line.clear();
        let read = reader.read_line(&mut line).ok()?;
        if read == 0 {
            break;
        }
        consumed = consumed.saturating_add(read as u64);
        if consumed > MAX_SESSION_FILE_BYTES {
            break;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) {
            visit(&value);
        }
    }
    Some(modified.unwrap_or(UNIX_EPOCH))
}

pub(super) fn fallback_timestamp(time: SystemTime) -> Option<String> {
    let duration = time.duration_since(UNIX_EPOCH).ok()?;
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(duration.as_millis() as i64)
        .map(|value| value.to_rfc3339())
}

pub(super) fn text_value(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(text) => clean_text(text),
        serde_json::Value::Array(values) => values.iter().find_map(|item| {
            if item.get("type").and_then(serde_json::Value::as_str) == Some("text")
                || item.get("type").and_then(serde_json::Value::as_str) == Some("input_text")
            {
                clean_text(
                    item.get("text")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default(),
                )
            } else {
                text_value(Some(item))
            }
        }),
        serde_json::Value::Object(object) => clean_text(
            object
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
        ),
        _ => None,
    }
}

pub(super) fn clean_text(value: &str) -> Option<String> {
    let value = value
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
        .collect::<String>();
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let mut title = value.chars().take(160).collect::<String>();
    if value.chars().count() > 160 {
        title.push('…');
    }
    Some(title)
}

pub(super) fn valid_model(value: Option<&serde_json::Value>) -> Option<String> {
    let model = value?.as_str()?.trim();
    if model.is_empty() || model.eq_ignore_ascii_case("<synthetic>") {
        return None;
    }
    Some(model.to_string())
}

pub(super) fn string_field<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn timestamp_field(value: &serde_json::Value) -> Option<String> {
    string_field(value, "timestamp")
        .or_else(|| {
            value
                .get("payload")
                .and_then(|payload| string_field(payload, "timestamp"))
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::{clean_text, comparable_path, valid_resume_id};
    use std::path::Path;

    #[test]
    fn cleans_titles_without_unbounded_text() {
        let title = clean_text("  first\n\tsecond  ").unwrap();
        assert_eq!(title, "first second");
        assert_eq!(clean_text(" "), None);
        assert_eq!(clean_text(&"x".repeat(200)).unwrap().chars().count(), 161);
    }

    #[test]
    fn normalizes_path_separators() {
        assert_eq!(comparable_path(Path::new("/tmp/example/")), "/tmp/example");
    }

    #[test]
    fn accepts_cli_session_ids_without_option_ambiguity() {
        assert!(valid_resume_id("019f6001-4f21-7340-ad24-d5a1457b156b"));
        assert!(valid_resume_id("session_1"));
        assert!(!valid_resume_id("--last"));
        assert!(!valid_resume_id("session 1"));
        assert!(!valid_resume_id("会话-1"));
    }
}
