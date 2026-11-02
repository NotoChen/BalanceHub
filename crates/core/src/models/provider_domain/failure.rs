//! 自动刷新失败的分级、退避与冷却。
//!
//! 旧实现把所有失败一视同仁：固定等待 60 秒，或者在重试额度用完后按正常
//! 间隔继续。凭据被拒时这等于持续拿错误凭据打站点；站点限流时重试只会让
//! 情况更糟。这里只区分「重试有没有意义」：
//!
//! - 凭据被拒：用户不修改凭据就会一直失败，自动刷新进入长冷却；
//! - 站点限流：按站点给出的 `Retry-After` 或统一回退值等待；
//! - 瞬时失败：网络、超时、5xx，按连续失败次数指数退避。
//!
//! 冷却写在 `runtime.cooldown_until` 上：调度器据此跳过该站点，卡片据此
//! 展示暂停原因。手动刷新永远不受冷却限制，成功一次即解除。

use std::collections::HashMap;

use crate::models::{AppSettings, Provider};

use super::automation;

/// 一次刷新失败的来源分类。适配器只在能结构化判断时声明，其余走默认的
/// 瞬时失败，避免把展示文案当成业务信号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFailureKind {
    /// 凭据或登录被上游拒绝：重试没有意义。
    Credential,
    /// 上游限流，可能带 `Retry-After`。
    RateLimited { retry_after_secs: Option<u64> },
    /// 网络、超时、5xx 等稍后可能自愈的问题。
    Transient,
}

/// 凭据失效后的冷却时长：久到不会反复拿错误凭据打站点，又不至于让用户
/// 修正凭据后还要等太久（手动刷新本就绕过冷却）。
const CREDENTIAL_COOLDOWN_SECS: u64 = 6 * 60 * 60;
/// 瞬时失败的退避起点：1、2、4……分钟。
const TRANSIENT_BASE_SECS: u64 = 60;
/// 瞬时失败的退避上限，避免一次长故障把下一次尝试推得太远。
const TRANSIENT_MAX_SECS: u64 = 15 * 60;
/// 限流但没有 `Retry-After` 时的回退等待。
const RATE_LIMIT_FALLBACK_SECS: u64 = 5 * 60;
/// `Retry-After` 的上限：站点偶尔会给出离谱的值，超过正常刷新间隔太多
/// 就失去了「自动刷新」的意义。
const RATE_LIMIT_MAX_SECS: u64 = 30 * 60;

/// 本次刷新结束后记录成败：失败写入冷却，成功清零。
///
/// `retry_limit` 是用户设置的「失败后重试」次数：瞬时失败在额度内指数退避，
/// 用完后回到正常刷新间隔。凭据失败不消耗重试额度，直接长冷却。
pub fn record_attempt(
    provider: &mut Provider,
    failure: Option<ProviderFailureKind>,
    retry_limit: u32,
    now_secs: u64,
) {
    let Some(kind) = failure else {
        provider.runtime.consecutive_failures = 0;
        provider.runtime.cooldown_until = None;
        provider.runtime.cooldown_reason = None;
        return;
    };
    let failures = provider.runtime.consecutive_failures.saturating_add(1);
    provider.runtime.consecutive_failures = failures;
    let Some((wait_secs, reason)) = cooldown(kind, failures, retry_limit) else {
        provider.runtime.cooldown_until = None;
        provider.runtime.cooldown_reason = None;
        return;
    };
    let until = now_secs.saturating_add(wait_secs);
    provider.runtime.cooldown_until = i64::try_from(until).ok();
    provider.runtime.cooldown_reason = Some(reason.to_string());
}

fn cooldown(
    kind: ProviderFailureKind,
    failures: u32,
    retry_limit: u32,
) -> Option<(u64, &'static str)> {
    match kind {
        ProviderFailureKind::Credential => {
            Some((CREDENTIAL_COOLDOWN_SECS, "凭据被拒绝，已暂停自动刷新"))
        }
        ProviderFailureKind::RateLimited { retry_after_secs } => {
            let wait = retry_after_secs
                .unwrap_or(RATE_LIMIT_FALLBACK_SECS)
                .clamp(TRANSIENT_BASE_SECS, RATE_LIMIT_MAX_SECS);
            Some((wait, "站点限流，稍后自动重试"))
        }
        ProviderFailureKind::Transient => {
            if failures > retry_limit {
                return None;
            }
            let exponent = failures.saturating_sub(1).min(16);
            let wait = TRANSIENT_BASE_SECS
                .saturating_mul(1_u64 << exponent)
                .min(TRANSIENT_MAX_SECS);
            Some((wait, "同步失败，稍后自动重试"))
        }
    }
}

/// 冷却尚未结束：自动刷新应跳过该站点。
pub fn cooling_down(provider: &Provider, now_secs: u64) -> bool {
    provider
        .runtime
        .cooldown_until
        .is_some_and(|until| i64::try_from(now_secs).unwrap_or(i64::MAX) < until)
}

/// 调度器使用的到期判定：冷却优先于刷新间隔，冷却结束后立即补一次尝试。
pub fn refresh_due(
    provider: &Provider,
    settings: &AppSettings,
    now_secs: u64,
    attempts: &HashMap<String, u64>,
) -> bool {
    if provider.runtime.cooldown_until.is_some() {
        // 冷却中跳过；冷却结束后立即补一次尝试，不等正常间隔。
        return !cooling_down(provider, now_secs);
    }
    automation::refresh_due(provider, settings, now_secs, attempts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderInput;

    fn provider() -> Provider {
        Provider::from_input(ProviderInput::default(), "p1".to_string())
    }

    #[test]
    fn transient_failures_back_off_exponentially_within_the_retry_budget() {
        let mut provider = provider();
        for (failures, wait) in [(1, 60), (2, 120), (3, 240)] {
            record_attempt(
                &mut provider,
                Some(ProviderFailureKind::Transient),
                3,
                1_000,
            );
            assert_eq!(provider.runtime.consecutive_failures, failures);
            assert_eq!(provider.runtime.cooldown_until, Some(1_000 + wait));
            assert!(provider.runtime.cooldown_reason.is_some());
        }
        // 重试额度用尽：交给正常刷新间隔。
        record_attempt(
            &mut provider,
            Some(ProviderFailureKind::Transient),
            3,
            1_000,
        );
        assert_eq!(provider.runtime.consecutive_failures, 4);
        assert_eq!(provider.runtime.cooldown_until, None);
        assert_eq!(provider.runtime.cooldown_reason, None);
    }

    #[test]
    fn credential_failures_pause_longer_and_ignore_the_retry_budget() {
        let mut provider = provider();
        record_attempt(
            &mut provider,
            Some(ProviderFailureKind::Credential),
            0,
            1_000,
        );
        assert_eq!(
            provider.runtime.cooldown_until,
            Some((1_000 + CREDENTIAL_COOLDOWN_SECS) as i64)
        );
        assert!(provider.runtime.cooldown_reason.is_some());
    }

    #[test]
    fn success_clears_the_cooldown_and_failure_count() {
        let mut provider = provider();
        record_attempt(
            &mut provider,
            Some(ProviderFailureKind::Credential),
            3,
            1_000,
        );
        record_attempt(&mut provider, None, 3, 2_000);
        assert_eq!(provider.runtime.consecutive_failures, 0);
        assert_eq!(provider.runtime.cooldown_until, None);
        assert_eq!(provider.runtime.cooldown_reason, None);
    }

    #[test]
    fn rate_limits_honour_retry_after_within_bounds() {
        let mut provider = provider();
        record_attempt(
            &mut provider,
            Some(ProviderFailureKind::RateLimited {
                retry_after_secs: Some(90),
            }),
            0,
            1_000,
        );
        assert_eq!(provider.runtime.cooldown_until, Some(1_090));

        record_attempt(
            &mut provider,
            Some(ProviderFailureKind::RateLimited {
                retry_after_secs: Some(86_400),
            }),
            0,
            1_000,
        );
        assert_eq!(
            provider.runtime.cooldown_until,
            Some((1_000 + RATE_LIMIT_MAX_SECS) as i64)
        );
    }

    #[test]
    fn cooldown_overrides_the_refresh_interval_until_it_expires() {
        let mut provider = provider();
        provider.automation.last_synced_at = Some("1000".to_string());
        let settings = AppSettings::default();
        let attempts = HashMap::new();
        assert!(refresh_due(&provider, &settings, 2_000, &attempts));

        record_attempt(
            &mut provider,
            Some(ProviderFailureKind::Transient),
            3,
            2_000,
        );
        assert!(!refresh_due(&provider, &settings, 2_030, &attempts));
        assert!(refresh_due(&provider, &settings, 2_060, &attempts));
    }
}
