use std::time::{SystemTime, UNIX_EPOCH};

/// 自 Unix 纪元以来的毫秒数。
pub fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

/// 自 Unix 纪元以来的秒数。
pub fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

/// 当前本地月份，格式 `YYYY-MM`。
pub fn current_month() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

/// NewAPI 额度单位（quota_per_unit）的默认值。
pub const DEFAULT_QUOTA_PER_UNIT: f64 = 500_000.0;
