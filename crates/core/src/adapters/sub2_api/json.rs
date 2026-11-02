use chrono::DateTime;
use serde_json::Value;

pub(super) fn string_field(value: &Value, fields: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| {
        let item = value.get(*field)?;
        if let Some(text) = item.as_str() {
            let text = text.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
        if item.is_number() {
            return Some(item.to_string());
        }
        None
    })
}

pub(super) fn number_field(value: &Value, fields: &[&str]) -> f64 {
    fields
        .iter()
        .find_map(|field| value.get(*field))
        .and_then(|item| item.as_f64().or_else(|| item.as_str()?.parse().ok()))
        .unwrap_or(0.0)
}

pub(super) fn integer_field(value: &Value, fields: &[&str]) -> i64 {
    fields
        .iter()
        .find_map(|field| value.get(*field))
        .and_then(|item| {
            item.as_i64()
                .or_else(|| item.as_u64().and_then(|value| i64::try_from(value).ok()))
                .or_else(|| item.as_f64().map(|value| value as i64))
                .or_else(|| item.as_str()?.parse().ok())
        })
        .unwrap_or(0)
}

pub(super) fn object_array(value: &Value, field: &str) -> Vec<Value> {
    value
        .get(field)
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| {
            value
                .get("data")
                .and_then(Value::as_object)
                .and_then(|data| data.get(field))
                .and_then(Value::as_array)
                .cloned()
        })
        .unwrap_or_default()
}

pub(super) fn array_items(value: &Value) -> Vec<Value> {
    if let Some(items) = value.as_array() {
        return items.clone();
    }
    if let Some(items) = value.get("items").and_then(Value::as_array) {
        return items.clone();
    }
    if let Some(data) = value.get("data") {
        return array_items(data);
    }
    Vec::new()
}

pub(super) fn string_list(value: &Value, fields: &[&str]) -> Vec<String> {
    let Some(raw) = fields.iter().find_map(|field| value.get(*field)) else {
        return Vec::new();
    };
    if let Some(values) = raw.as_array() {
        return values
            .iter()
            .flat_map(|item| {
                item.as_str()
                    .map(|value| value.split(',').map(str::to_string).collect::<Vec<_>>())
                    .unwrap_or_default()
            })
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
    }
    raw.as_str()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn timestamp_millis(value: &Value, fields: &[&str]) -> Option<i64> {
    let raw_value = fields.iter().find_map(|field| value.get(*field))?;
    if raw_value.is_null() {
        return None;
    }
    let raw = string_field(value, fields)?;
    if let Ok(number) = raw.parse::<i64>() {
        if number < 0 {
            return Some(number);
        }
        return Some(if number < 1_000_000_000_000 {
            number * 1000
        } else {
            number
        });
    }
    DateTime::parse_from_rfc3339(&raw)
        .ok()
        .map(|date| date.timestamp_millis())
}

pub(super) fn value_has_field(value: &Value, fields: &[&str]) -> bool {
    fields
        .iter()
        .any(|field| value.get(*field).is_some_and(|item| !item.is_null()))
}

#[cfg(test)]
mod tests {
    use super::array_items;
    use serde_json::json;

    #[test]
    fn array_items_accepts_paginated_and_nested_shapes() {
        assert_eq!(array_items(&json!({"items": [1, 2]})).len(), 2);
        assert_eq!(array_items(&json!({"data": {"items": [1]}})).len(), 1);
        assert_eq!(array_items(&json!([1, 2, 3])).len(), 3);
    }
}
