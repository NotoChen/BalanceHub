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
    let last_success = provider
        .automation
        .last_synced_at
        .as_ref()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let baseline = last_attempt.max(last_success);
    if baseline == 0 {
        return true;
    }

    now_secs >= baseline.saturating_add(interval)
}

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
