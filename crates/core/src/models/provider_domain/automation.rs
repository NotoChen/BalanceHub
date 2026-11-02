use std::collections::HashMap;

use chrono::Local;

use crate::models::{AppSettings, Provider};

pub fn refresh_interval(provider: &Provider, settings: &AppSettings) -> u64 {
    if provider.automation.refresh_interval > 0 {
        provider.automation.refresh_interval
    } else {
        settings.refresh_interval
    }
}

pub fn refresh_due(
    provider: &Provider,
    settings: &AppSettings,
    now_secs: u64,
    attempts: &HashMap<String, u64>,
) -> bool {
    let interval = refresh_interval(provider, settings);
    if interval == 0 {
        return false;
    }

    let last_attempt = attempts.get(&provider.identity.id).copied().unwrap_or(0);
    let last_success = synced_at_secs(provider.automation.last_synced_at.as_deref());
    let baseline = last_attempt.max(last_success);
    if baseline == 0 {
        return true;
    }

    now_secs >= baseline.saturating_add(interval)
}

/// `last_synced_at` is a serialized timestamp, and adapters have written both
/// seconds and milliseconds into it. The scheduler compares against seconds, so
/// a millisecond value would read as a date far in the future and the account
/// would never become due again; normalize at the boundary that owns the unit.
fn synced_at_secs(value: Option<&str>) -> u64 {
    let Some(raw) = value.and_then(|value| value.trim().parse::<u128>().ok()) else {
        return 0;
    };
    let secs = if raw > MILLIS_THRESHOLD {
        raw / 1000
    } else {
        raw
    };
    u64::try_from(secs).unwrap_or(u64::MAX)
}

/// Any epoch-second timestamp is below 1e12 until the year 33658, while a
/// millisecond timestamp passed 1e12 in 2001.
const MILLIS_THRESHOLD: u128 = 1_000_000_000_000;

pub fn effective_check_in_time<'a>(provider: &'a Provider, settings: &'a AppSettings) -> &'a str {
    let provider_time = provider.automation.check_in_time.trim();
    if !provider_time.is_empty() {
        return provider_time;
    }

    let global_time = settings.check_in_time.trim();
    if !global_time.is_empty() {
        return global_time;
    }

    "00:00"
}

pub fn check_in_due_now(provider: &Provider, settings: &AppSettings) -> bool {
    Local::now().format("%H:%M").to_string().as_str() >= effective_check_in_time(provider, settings)
}

#[cfg(test)]
mod tests {
    use super::synced_at_secs;

    #[test]
    fn sync_timestamps_are_read_as_seconds_even_when_written_as_milliseconds() {
        assert_eq!(synced_at_secs(None), 0);
        assert_eq!(synced_at_secs(Some("  ")), 0);
        assert_eq!(synced_at_secs(Some("not a time")), 0);
        assert_eq!(synced_at_secs(Some("1700000000")), 1_700_000_000);
        assert_eq!(synced_at_secs(Some("1700000000123")), 1_700_000_000);
    }
}
