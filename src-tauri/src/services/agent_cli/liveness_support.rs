use serde_json::Value;

pub(super) fn extract_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

pub(super) fn extract_f64(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

pub(super) fn token_sum(values: &[Option<u64>]) -> Option<u64> {
    let mut found = false;
    let mut total = 0u64;
    for value in values.iter().flatten() {
        found = true;
        total = total.saturating_add(*value);
    }
    found.then_some(total)
}

pub(super) fn add_optional(target: &mut Option<u64>, value: Option<u64>) {
    let Some(value) = value else {
        return;
    };
    *target = Some(target.unwrap_or_default().saturating_add(value));
}
