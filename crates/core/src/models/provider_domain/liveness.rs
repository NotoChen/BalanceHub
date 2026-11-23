use crate::models::{provider_domain::auth, AppSettings, Provider};

pub fn automatic_enabled(provider: &Provider, settings: &AppSettings) -> bool {
    provider.runtime.enabled && auth::has_api_key(provider) && effective_enabled(provider, settings)
}

pub fn effective_enabled(provider: &Provider, settings: &AppSettings) -> bool {
    if provider.liveness.use_global {
        settings.liveness_enabled
    } else {
        provider.liveness.enabled
    }
}

/// `next_at` 是「上次运行时间 + 当时的间隔」，正常不会超过「现在 + 允许的
/// 最大间隔」。更远的未来时间只可能是系统时钟回拨后留下的旧值；继续等待
/// 会让自动测活静默停摆，所以按到期处理。
pub fn is_due(provider: &Provider, now_millis: u128, max_interval_millis: u128) -> bool {
    let next = next_at_millis(provider);
    if next <= now_millis {
        return true;
    }
    next > now_millis
        .saturating_add(max_interval_millis)
        .saturating_add(u128::from(super::CLOCK_ROLLBACK_TOLERANCE_SECS) * 1000)
}

pub fn next_at_millis(provider: &Provider) -> u128 {
    provider
        .liveness
        .next_at
        .as_ref()
        .and_then(|value| value.trim().parse::<u128>().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderInput;

    #[test]
    fn a_clock_rollback_far_beyond_the_interval_counts_as_due() {
        let mut provider =
            Provider::from_input(ProviderInput::default(), "liveness-fixture".into());
        let now = 1_000_000_000_000_u128;
        let hour = 3_600_000_u128;

        provider.liveness.next_at = Some((now + 60_000).to_string());
        assert!(!is_due(&provider, now, hour));

        // 比「现在 + 最大间隔 + 容差」更远的计划只可能是时钟回拨。
        provider.liveness.next_at = Some((now + hour + 11 * 60_000).to_string());
        assert!(is_due(&provider, now, hour));
    }
}
