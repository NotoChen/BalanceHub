use crate::{
    models::{
        default_liveness_placeholder_pools, AppSettings, LivenessCliKind, LivenessHttpProtocol,
        LivenessIntervalMode, LivenessMethod, LivenessPlaceholderPool, LivenessPromptMode,
        Provider,
    },
    util::unix_millis as now_millis,
};

const MIN_LIVENESS_INTERVAL_SECS: u64 = 1;

pub fn openai_base_url(provider: &Provider) -> String {
    let raw = if provider.liveness.openai_base_url.trim().is_empty() {
        provider.identity.base_url.trim()
    } else {
        provider.liveness.openai_base_url.trim()
    };
    let normalized = raw.trim_end_matches('/').to_string();
    if normalized.ends_with("/v1") {
        normalized
    } else {
        format!("{normalized}/v1")
    }
}

pub fn anthropic_base_url(provider: &Provider) -> String {
    let raw = if provider.liveness.anthropic_base_url.trim().is_empty() {
        provider.identity.base_url.trim()
    } else {
        provider.liveness.anthropic_base_url.trim()
    };
    raw.trim_end_matches('/').to_string()
}

pub fn effective_interval(settings: &AppSettings, provider: &Provider) -> u64 {
    let (mode, fixed, min, max) = if provider.liveness.use_global {
        (
            settings.liveness_interval_mode,
            settings.liveness_interval,
            settings.liveness_random_min_interval,
            settings.liveness_random_max_interval,
        )
    } else {
        (
            provider.liveness.interval_mode,
            provider.liveness.interval,
            provider.liveness.random_min_interval,
            provider.liveness.random_max_interval,
        )
    };

    match mode {
        LivenessIntervalMode::Fixed => fixed.max(MIN_LIVENESS_INTERVAL_SECS),
        LivenessIntervalMode::Random => {
            let min = min.max(MIN_LIVENESS_INTERVAL_SECS);
            let max = max.max(min);
            min + (mix_seed(now_millis()) as u64 % (max - min + 1))
        }
    }
}

pub(super) fn effective_model(settings: &AppSettings, provider: &Provider) -> String {
    if provider.liveness.model.trim().is_empty() {
        settings.liveness_model.trim().to_string()
    } else {
        provider.liveness.model.trim().to_string()
    }
}

pub(super) fn effective_cli_kind(settings: &AppSettings, provider: &Provider) -> LivenessCliKind {
    provider
        .liveness
        .cli_kind
        .unwrap_or(settings.liveness_cli_kind)
}

pub(super) fn effective_method(settings: &AppSettings, provider: &Provider) -> LivenessMethod {
    provider.liveness.method.unwrap_or(settings.liveness_method)
}

pub(super) fn effective_http_protocol(
    settings: &AppSettings,
    provider: &Provider,
) -> LivenessHttpProtocol {
    provider
        .liveness
        .http_protocol
        .unwrap_or(settings.liveness_http_protocol)
}

pub(super) fn effective_timeout(settings: &AppSettings, provider: &Provider) -> u64 {
    if provider.liveness.use_global {
        settings.liveness_timeout.max(10)
    } else {
        provider.liveness.timeout.max(10)
    }
}

pub(super) fn select_prompt(settings: &AppSettings, provider: &Provider) -> String {
    let mode = if provider.liveness.use_global {
        settings.liveness_prompt_mode
    } else {
        provider.liveness.prompt_mode
    };
    let fixed = if provider.liveness.use_global {
        settings.liveness_fixed_prompt.trim()
    } else {
        provider.liveness.fixed_prompt.trim()
    };
    if matches!(mode, LivenessPromptMode::Fixed) && !fixed.is_empty() {
        return inject_dynamic_values(settings, fixed, prompt_seed(provider, 0));
    }

    let library: Vec<&str> = settings
        .liveness_prompt_library
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .collect();
    if library.is_empty() {
        return inject_dynamic_values(settings, &default_codex_prompt(), prompt_seed(provider, 1));
    }

    let index = match mode {
        LivenessPromptMode::RoundRobin => provider.liveness.prompt_cursor as usize % library.len(),
        _ => mixed_index(prompt_seed(provider, 2), library.len()),
    };
    inject_dynamic_values(
        settings,
        library[index],
        prompt_seed(provider, provider.liveness.prompt_cursor + index as u64),
    )
}

pub fn preview_prompts(settings: &AppSettings, count: usize) -> Vec<String> {
    let count = count.clamp(1, 20);
    let library: Vec<&str> = settings
        .liveness_prompt_library
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .collect();
    let templates = if library.is_empty() {
        vec![default_codex_prompt()]
    } else {
        library.iter().map(|item| item.to_string()).collect()
    };
    let fixed = settings.liveness_fixed_prompt.trim();

    (0..count)
        .map(|index| {
            let seed = mix_seed(now_millis() + index as u128 * 7919);
            let template = if matches!(settings.liveness_prompt_mode, LivenessPromptMode::Fixed)
                && !fixed.is_empty()
            {
                fixed
            } else {
                &templates[mixed_index(seed, templates.len())]
            };
            inject_dynamic_values(settings, template, seed)
        })
        .collect()
}

fn inject_dynamic_values(settings: &AppSettings, prompt: &str, seed: u128) -> String {
    let min = settings
        .liveness_number_min
        .min(settings.liveness_number_max);
    let max = settings
        .liveness_number_max
        .max(settings.liveness_number_min);
    let a = number_in_range(seed, min, max);
    let b = number_in_range(seed / 7 + 17, min, max);
    let mut output = prompt
        .replace("{time}", &seed.to_string())
        .replace("{nonce}", &(seed % 100_000).to_string())
        .replace("{a}", &a.to_string())
        .replace("{b}", &b.to_string())
        .replace(
            "{number}",
            &number_in_range(seed / 13 + 29, min, max).to_string(),
        );

    for pool in effective_placeholder_pools(settings) {
        let key = pool.key.trim();
        if key.is_empty() {
            continue;
        }
        let values = pool
            .values
            .iter()
            .map(|item| item.trim())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        if values.is_empty() {
            continue;
        }
        let token = format!("{{{key}}}");
        output = output.replace(&token, pick_str(seed_for_key(seed, key), &values));
    }
    output
}

fn effective_placeholder_pools(settings: &AppSettings) -> Vec<LivenessPlaceholderPool> {
    let mut pools = settings
        .liveness_placeholder_pools
        .iter()
        .filter(|pool| {
            !pool.key.trim().is_empty() && pool.values.iter().any(|value| !value.trim().is_empty())
        })
        .cloned()
        .collect::<Vec<_>>();
    let configured_keys = pools
        .iter()
        .map(|pool| pool.key.trim().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    for fallback in default_liveness_placeholder_pools() {
        if !configured_keys.contains(fallback.key.trim()) {
            pools.push(fallback);
        }
    }
    pools
}

fn number_in_range(seed: u128, min: u64, max: u64) -> u64 {
    let range = max.saturating_sub(min).saturating_add(1);
    min.saturating_add((mix_seed(seed) as u64) % range.max(1))
}

fn prompt_seed(provider: &Provider, salt: u64) -> u128 {
    let mut seed = now_millis() + salt as u128 * 104_729;
    for byte in provider
        .identity
        .id
        .as_bytes()
        .iter()
        .chain(provider.identity.base_url.as_bytes())
    {
        seed = seed.wrapping_mul(131).wrapping_add(*byte as u128);
    }
    mix_seed(seed)
}

fn seed_for_key(seed: u128, key: &str) -> u128 {
    let mut value = seed;
    for byte in key.as_bytes() {
        value = value.wrapping_mul(167).wrapping_add(*byte as u128);
    }
    mix_seed(value)
}

pub(super) fn mix_seed(seed: u128) -> u128 {
    let mut value = seed ^ (seed >> 33);
    value = value.wrapping_mul(0xff51afd7ed558ccd);
    value ^= value >> 29;
    value = value.wrapping_mul(0xc4ceb9fe1a85ec53);
    value ^ (value >> 32)
}

fn mixed_index(seed: u128, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (mix_seed(seed) as usize) % len
    }
}

fn pick_str<'a>(seed: u128, values: &'a [&str]) -> &'a str {
    values[mixed_index(seed, values.len())]
}

fn default_codex_prompt() -> String {
    "Explain: ls -la".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AuthMode, ProviderInput};

    fn provider_with_liveness(
        use_global: bool,
        cli_kind: Option<LivenessCliKind>,
        model: &str,
    ) -> Provider {
        Provider::from_input(
            ProviderInput {
                identity: crate::models::ProviderIdentityInput {
                    name: "Relay".to_string(),
                    base_url: "https://relay.example.com".to_string(),
                },
                auth: crate::models::ProviderAuth {
                    mode: AuthMode::ApiKey,
                    api_key: "sk-test".to_string(),
                    ..ProviderInput::default().auth
                },
                liveness: crate::models::ProviderLivenessInput {
                    use_global,
                    enabled: true,
                    cli_kind,
                    model: model.to_string(),
                    ..ProviderInput::default().liveness
                },
                ..ProviderInput::default()
            },
            "provider-test".to_string(),
        )
    }

    #[test]
    fn provider_model_overrides_global_even_when_schedule_uses_global() {
        let settings = AppSettings::default();
        let provider = provider_with_liveness(true, None, "claude-opus-4-6");

        assert_eq!(effective_model(&settings, &provider), "claude-opus-4-6");
    }

    #[test]
    fn empty_provider_model_falls_back_to_global_model() {
        let settings = AppSettings::default();
        let provider = provider_with_liveness(true, None, "");

        assert_eq!(
            effective_model(&settings, &provider),
            settings.liveness_model
        );
    }

    #[test]
    fn provider_cli_overrides_global_even_when_schedule_uses_global() {
        let settings = AppSettings::default();
        let provider = provider_with_liveness(true, Some(LivenessCliKind::ClaudeCode), "");

        assert_eq!(
            effective_cli_kind(&settings, &provider),
            LivenessCliKind::ClaudeCode
        );
    }
}
