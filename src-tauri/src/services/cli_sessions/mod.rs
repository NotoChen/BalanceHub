mod claude;
mod codex;

use crate::models::{CliSessionSummary, LivenessCliKind};
use std::path::Path;

const MAX_SESSIONS: usize = 100;

/// 读取本机 CLI 自己维护的历史索引。这里不启动 CLI、不会写入状态目录，
/// 也不依赖 Codex Desktop 的 app-server。
pub fn list(cli_kind: LivenessCliKind, workdir: &Path) -> Result<Vec<CliSessionSummary>, String> {
    if !workdir.is_dir() {
        return Err("工作目录不存在，无法读取历史会话".to_string());
    }

    let mut sessions = match cli_kind {
        LivenessCliKind::Codex => codex::list(workdir, MAX_SESSIONS)?,
        LivenessCliKind::ClaudeCode => claude::list(workdir)?,
    };
    sessions.sort_by(|left, right| {
        session_sort_key(right.updated_at.as_deref())
            .cmp(&session_sort_key(left.updated_at.as_deref()))
            .then_with(|| left.id.cmp(&right.id))
    });
    // CLI 状态目录会在进程启动后、用户尚未发送任何消息时留下空壳记录。
    // 这些记录没有可展示内容，也不能为用户提供有效的恢复目标；只在
    // BalanceHub 的读取结果中过滤，不修改 CLI 自己维护的原始索引。
    sessions.retain(|session| !is_empty_shell(session));
    sessions.truncate(MAX_SESSIONS);
    Ok(sessions)
}

fn is_empty_shell(session: &CliSessionSummary) -> bool {
    session.title == "未命名会话" && session.preview.is_none()
}

fn session_sort_key(value: Option<&str>) -> i64 {
    value
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis())
        .unwrap_or_default()
}

fn clean_text(value: impl AsRef<str>, limit: usize) -> Option<String> {
    let value = value
        .as_ref()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if value.is_empty() {
        return None;
    }
    let mut text = value.chars().take(limit).collect::<String>();
    if value.chars().count() > limit {
        text.push_str("...");
    }
    Some(text)
}

fn first_non_empty(values: impl IntoIterator<Item = Option<String>>) -> String {
    values
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "未命名会话".to_string())
}

fn timestamp_from_value(value: Option<i64>, milliseconds: bool) -> Option<String> {
    let value = value?;
    let millis = if milliseconds {
        value
    } else if value.abs() < 100_000_000_000 {
        value.saturating_mul(1000)
    } else {
        value
    };
    chrono::DateTime::from_timestamp_millis(millis).map(|date| date.to_rfc3339())
}

fn normalize_timestamp(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(number) = value.parse::<i64>() {
        return timestamp_from_value(Some(number), false);
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::{
        clean_text, first_non_empty, is_empty_shell, normalize_timestamp, timestamp_from_value,
    };
    use crate::models::{CliSessionMetadataSource, CliSessionSummary, LivenessCliKind};

    #[test]
    fn text_is_compacted_and_bounded() {
        assert_eq!(
            clean_text("  hello\n world  ", 20).as_deref(),
            Some("hello world")
        );
        assert_eq!(clean_text("abcdef", 3).as_deref(), Some("abc..."));
        assert_eq!(clean_text("  ", 20), None);
    }

    #[test]
    fn title_fallback_uses_first_non_empty_value() {
        assert_eq!(
            first_non_empty([None, Some(String::new()), Some("title".to_string())]),
            "title"
        );
        assert_eq!(first_non_empty([None]), "未命名会话");
    }

    #[test]
    fn timestamps_accept_seconds_milliseconds_and_rfc3339() {
        assert!(timestamp_from_value(Some(1_725_000_000), false).is_some());
        assert!(timestamp_from_value(Some(1_725_000_000_000), true).is_some());
        assert_eq!(
            normalize_timestamp(Some("2026-08-06T08:00:00Z")),
            Some("2026-08-06T08:00:00+00:00".to_string())
        );
    }

    #[test]
    fn empty_shell_sessions_are_filtered_without_touching_source_data() {
        let empty = CliSessionSummary {
            id: "empty".to_string(),
            title: "未命名会话".to_string(),
            preview: None,
            model: None,
            models: Vec::new(),
            cli_kind: LivenessCliKind::Codex,
            created_at: None,
            updated_at: None,
            workdir: "/tmp/project".to_string(),
            cli_version: None,
            archived: false,
            can_resume: true,
            metadata_source: CliSessionMetadataSource::CodexStateDb,
        };
        let named = CliSessionSummary {
            id: "named".to_string(),
            title: "BalanceHub".to_string(),
            ..empty.clone()
        };
        assert!(is_empty_shell(&empty));
        assert!(!is_empty_shell(&named));
    }
}
