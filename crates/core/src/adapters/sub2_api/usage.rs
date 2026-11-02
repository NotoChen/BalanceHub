use crate::models::ProviderRequestLog;
use chrono::Local;
use serde_json::Value;

use super::json::{integer_field, number_field, string_field};

pub(super) fn normalize_log(item: Value) -> ProviderRequestLog {
    let prompt = integer_field(&item, &["input_tokens"]);
    let completion = integer_field(&item, &["output_tokens"]);
    ProviderRequestLog {
        id: string_field(&item, &["id"]).unwrap_or_default(),
        created_at: string_field(&item, &["created_at", "createdAt"]).unwrap_or_default(),
        token_name: item
            .get("api_key")
            .and_then(|key| {
                if key.is_object() {
                    string_field(key, &["name", "masked_key", "key"])
                } else {
                    key.as_str().map(str::to_string)
                }
            })
            .or_else(|| string_field(&item, &["api_key_name", "apiKeyName"]))
            .unwrap_or_default(),
        model_name: string_field(&item, &["model"]).unwrap_or_default(),
        request_id: string_field(&item, &["request_id", "requestId"]).unwrap_or_default(),
        status: string_field(&item, &["status", "request_type", "requestType"]).unwrap_or_default(),
        prompt_tokens: prompt,
        completion_tokens: completion,
        token_used: integer_field(&item, &["total_tokens"]).max(prompt + completion),
        quota: number_field(&item, &["actual_cost", "total_cost"]),
        channel: item
            .get("group")
            .and_then(|group| {
                if group.is_object() {
                    string_field(group, &["name", "id"])
                } else {
                    group.as_str().map(str::to_string)
                }
            })
            .or_else(|| string_field(&item, &["group_name", "groupName"]))
            .unwrap_or_default(),
        duration_ms: Some(integer_field(&item, &["duration_ms", "durationMs"])),
        content: String::new(),
        raw: item,
    }
}

pub(super) fn usage_dates(period: &str) -> (String, String) {
    let days = match period.trim() {
        "24h" | "1d" => 1,
        "7d" => 7,
        _ => 30,
    };
    let end = Local::now().date_naive();
    let start = end - chrono::Duration::days(days - 1);
    (
        start.format("%Y-%m-%d").to_string(),
        end.format("%Y-%m-%d").to_string(),
    )
}

pub(super) fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                vec![byte as char]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect()
}
