use crate::models::{Provider, ProviderCheckInRecord};
use crate::util::DEFAULT_QUOTA_PER_UNIT;
use serde_json::Value;

use super::super::newapi_response::{extract_bool_field, extract_string_field};

pub(super) fn normalize_month(month: &str) -> Result<String, String> {
    let value = month.trim();
    let bytes = value.as_bytes();
    if bytes.len() == 7
        && bytes[4] == b'-'
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
    {
        let month_value = value[5..7].parse::<u32>().unwrap_or_default();
        if (1..=12).contains(&month_value) {
            return Ok(value.to_string());
        }
    }
    Err("月份格式应为 YYYY-MM".to_string())
}

pub(super) fn collect(
    value: &Value,
    month: &str,
    provider: &Provider,
) -> Vec<ProviderCheckInRecord> {
    let mut records = Vec::new();
    collect_recursive(value, month, provider, &mut records);
    records.sort_by(|left, right| left.date.cmp(&right.date));
    records.dedup_by(|left, right| {
        left.date == right.date
            && left.checked_at == right.checked_at
            && left.message == right.message
            && left.quota_delta == right.quota_delta
    });
    records
}

fn collect_recursive(
    value: &Value,
    month: &str,
    provider: &Provider,
    records: &mut Vec<ProviderCheckInRecord>,
) {
    match value {
        Value::Object(map) => {
            if let Some(record) = parse_record(value, month, provider) {
                records.push(record);
            }
            for child in map.values() {
                collect_recursive(child, month, provider, records);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_recursive(item, month, provider, records);
            }
        }
        _ => {}
    }
}

fn parse_record(value: &Value, month: &str, provider: &Provider) -> Option<ProviderCheckInRecord> {
    let map = value.as_object()?;
    let checked = extract_bool_field(
        value,
        &["checked", "checked_in", "checkedIn", "signed", "signIn"],
    )
    .unwrap_or(true);
    if !checked {
        return None;
    }

    let checked_at = extract_string_field(
        value,
        &[
            "checked_at",
            "checkedAt",
            "checkin_at",
            "checkInAt",
            "signed_at",
            "signedAt",
            "created_at",
            "createdAt",
            "time",
            "datetime",
        ],
    );
    let date_source = extract_string_field(
        value,
        &[
            "date",
            "day",
            "checkin_date",
            "checkInDate",
            "sign_date",
            "signDate",
        ],
    )
    .or_else(|| checked_at.clone())?;
    let date = normalize_record_date(&date_source, month)?;

    let quota_delta = extract_quota_delta(value, provider);
    let message = extract_string_field(value, &["message", "msg", "remark", "desc", "description"])
        .unwrap_or_default();

    if quota_delta.is_none()
        && message.trim().is_empty()
        && !map
            .keys()
            .any(|key| key.to_ascii_lowercase().contains("check"))
    {
        return None;
    }

    Some(ProviderCheckInRecord {
        date,
        checked_at,
        quota_delta,
        message,
    })
}

fn normalize_record_date(value: &str, month: &str) -> Option<String> {
    let trimmed = value.trim();
    // 用 get(0..10) 而非 &trimmed[0..10]：date 字段来自服务器返回的 JSON，
    // 若第 10 字节落在多字节 UTF-8 字符中间，按字节切片会 panic；get 在非字符边界处返回 None。
    if let Some(candidate) = trimmed.get(0..10) {
        if is_date(candidate) && candidate.starts_with(month) {
            return Some(candidate.to_string());
        }
    }
    if trimmed.len() <= 2 && trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        let day = trimmed.parse::<u32>().ok()?;
        if (1..=31).contains(&day) {
            return Some(format!("{month}-{day:02}"));
        }
    }
    None
}

fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

fn extract_quota_delta(value: &Value, provider: &Provider) -> Option<f64> {
    if let Some(raw) = extract_named_f64_field(
        value,
        &[
            "quota",
            "reward_quota",
            "rewardQuota",
            "quota_delta",
            "quotaDelta",
            "quota_awarded",
            "quotaAwarded",
            "awarded_quota",
            "awardedQuota",
            "add_quota",
            "addQuota",
        ],
    ) {
        return Some(convert_raw_quota_delta(raw, provider));
    }

    if let Some(display_value) = extract_named_f64_field(value, &["amount", "reward", "value"]) {
        return Some(display_value);
    }

    let text_value = extract_quota_from_text(&extract_string_field(value, &["message", "msg"])?)?;
    Some(normalize_text_quota_delta(text_value, provider))
}

fn extract_named_f64_field(value: &Value, field_names: &[&str]) -> Option<f64> {
    field_names.iter().find_map(|field_name| {
        value.get(*field_name).and_then(|item| {
            item.as_f64().or_else(|| {
                item.as_str()
                    .and_then(|text| text.trim().parse::<f64>().ok())
            })
        })
    })
}

fn convert_raw_quota_delta(value: f64, provider: &Provider) -> f64 {
    if provider.quota.display_type.eq_ignore_ascii_case("tokens") {
        return value;
    }

    let unit = if provider.quota.per_unit > 0.0 {
        provider.quota.per_unit
    } else {
        DEFAULT_QUOTA_PER_UNIT
    };
    let exchange_rate = if provider.quota.currency_exchange_rate > 0.0 {
        provider.quota.currency_exchange_rate
    } else {
        1.0
    };
    value / unit * exchange_rate
}

fn normalize_text_quota_delta(value: f64, provider: &Provider) -> f64 {
    if provider.quota.display_type.eq_ignore_ascii_case("tokens") {
        return value;
    }

    if value.fract() == 0.0 && value.abs() >= 1000.0 {
        convert_raw_quota_delta(value, provider)
    } else {
        value
    }
}

fn extract_quota_from_text(value: &str) -> Option<f64> {
    let mut current = String::new();
    let mut numbers = Vec::new();
    for ch in value.chars() {
        if ch.is_ascii_digit() || ch == '.' || ch == '-' {
            current.push(ch);
        } else if !current.is_empty() {
            if let Ok(number) = current.parse::<f64>() {
                numbers.push(number);
            }
            current.clear();
        }
    }
    if !current.is_empty() {
        if let Ok(number) = current.parse::<f64>() {
            numbers.push(number);
        }
    }
    numbers.into_iter().find(|number| number.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderInput;
    use serde_json::json;

    fn currency_provider() -> Provider {
        let mut provider =
            Provider::from_input(ProviderInput::default(), "provider-test".to_string());
        provider.quota.per_unit = 500_000.0;
        provider.quota.display_type = "currency".to_string();
        provider.quota.currency_exchange_rate = 1.0;
        provider
    }

    #[test]
    fn collect_converts_newapi_raw_check_in_quota_awarded() {
        let provider = currency_provider();
        let value = json!({
            "stats": {
                "checked_in_today": true,
                "checkin_count": 2,
                "total_quota": 370147
            },
            "records": [
                {
                    "checkin_date": "2026-06-26",
                    "quota_awarded": 253175
                },
                {
                    "checkin_date": "2026-06-27",
                    "quota_awarded": 116972
                }
            ]
        });

        let records = collect(&value, "2026-06", &provider);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].date, "2026-06-26");
        assert_eq!(records[0].quota_delta, Some(0.50635));
        assert_eq!(records[1].date, "2026-06-27");
        assert_eq!(records[1].quota_delta, Some(0.233944));
    }

    #[test]
    fn collect_keeps_display_amount_without_unit_conversion() {
        let provider = currency_provider();
        let value = json!({
            "records": [
                {
                    "checkin_date": "2026-06-26",
                    "amount": 0.51
                }
            ]
        });

        let records = collect(&value, "2026-06", &provider);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].quota_delta, Some(0.51));
    }
}
