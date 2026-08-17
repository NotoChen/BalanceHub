use crate::{
    models::{AgentCliKind, CliSessionSummary},
    services::agent_cli,
};
use std::path::Path;

#[cfg(test)]
mod tests;

const MAX_SESSIONS: usize = 100;

/// 读取本机 CLI 自己维护的历史索引。这里不启动 CLI、不会写入状态目录，
/// 也不依赖 Codex Desktop 的 app-server。
pub fn list(cli_kind: AgentCliKind, workdir: &Path) -> Result<Vec<CliSessionSummary>, String> {
    let definition = agent_cli::definition(cli_kind);
    let adapter = definition
        .sessions()
        .ok_or_else(|| format!("{} 当前不支持读取历史会话", definition.label))?;
    if !workdir.is_dir() {
        return Err("工作目录不存在，无法读取历史会话".to_string());
    }

    let mut sessions = adapter.list(cli_kind, workdir)?;
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

pub(crate) fn session_sort_key(value: Option<&str>) -> i64 {
    value
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis())
        .unwrap_or_default()
}

pub(crate) fn clean_text(value: impl AsRef<str>, limit: usize) -> Option<String> {
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

pub(crate) fn first_non_empty(values: impl IntoIterator<Item = Option<String>>) -> String {
    values
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "未命名会话".to_string())
}

pub(crate) fn timestamp_from_value(value: Option<i64>, milliseconds: bool) -> Option<String> {
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

pub(crate) fn normalize_timestamp(value: Option<&str>) -> Option<String> {
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
