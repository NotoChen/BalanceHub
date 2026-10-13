mod index;
mod io;
mod metadata;
pub(crate) mod presentation;
mod record_stream;
pub(crate) mod resume;
mod search;
pub(crate) mod source;
#[cfg(test)]
mod tests;
pub(crate) mod usage;
pub(crate) mod workbench;

#[cfg(test)]
pub(crate) use index::test_support::{
    read_history_detail, read_indexed_messages, search_history, search_sources,
};
pub(crate) use index::{
    clear as clear_index, config as index_config, reconfigure as reconfigure_index,
    status as index_status,
};
pub(crate) use io::{
    json_text, read_json_lines_limited, scan_json_records, session_index_source_fingerprint,
};
pub(crate) use metadata::read_session_header;
pub(crate) use record_stream::read_json_messages_limited;
pub(crate) use search::{SearchAccumulator, SearchQuery};
const MAX_SEARCH_QUERY_CHARS: usize = 200;
const MAX_SEARCH_TERMS: usize = 8;
pub(crate) fn normalize_records(
    mut records: Vec<crate::services::agent_cli::contracts::SessionHistoryRecord>,
) -> Vec<crate::services::agent_cli::contracts::SessionHistoryRecord> {
    records.sort_by(|left, right| {
        session_sort_key(right.summary.updated_at.as_deref())
            .cmp(&session_sort_key(left.summary.updated_at.as_deref()))
            .then_with(|| left.record_key.cmp(&right.record_key))
    });
    let mut seen = std::collections::HashSet::new();
    records.retain(|record| seen.insert(record.record_key.clone()));
    records
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

pub(crate) fn timestamp_from_unix(value: Option<i64>) -> Option<String> {
    timestamp_from_value(value, false)
}
