use crate::models::{provider_domain::auth, AppSettings, Provider};

pub fn automatic_enabled(provider: &Provider, settings: &AppSettings) -> bool {
    provider.runtime.enabled
        && auth::has_api_key(provider)
        && settings.liveness_consent_accepted_at.is_some()
        && effective_enabled(provider, settings)
}

pub fn effective_enabled(provider: &Provider, settings: &AppSettings) -> bool {
    if provider.liveness.use_global {
        settings.liveness_enabled
    } else {
        provider.liveness.enabled
    }
}

pub fn is_due(provider: &Provider, now_millis: u128) -> bool {
    next_at_millis(provider) <= now_millis
}

pub fn next_at_millis(provider: &Provider) -> u128 {
    provider
        .liveness
        .next_at
        .as_ref()
        .and_then(|value| value.trim().parse::<u128>().ok())
        .unwrap_or(0)
}
