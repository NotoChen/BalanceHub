use crate::models::{LivenessCliKind, LivenessRecord};
use serde_json::Value;

#[derive(Default)]
pub(super) struct ParsedTokenUsage {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub total_cost_usd: Option<f64>,
}

pub(super) fn parse_liveness_output(
    cli_kind: LivenessCliKind,
    output: &str,
) -> (String, Option<String>) {
    match cli_kind {
        LivenessCliKind::Codex => (output.trim().chars().take(240).collect::<String>(), None),
        LivenessCliKind::ClaudeCode => match serde_json::from_str::<Value>(output.trim()) {
            Ok(value) => {
                let is_error = value
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let result = value
                    .get("result")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if is_error {
                    (String::new(), Some(result))
                } else {
                    (result.chars().take(240).collect(), None)
                }
            }
            Err(_) => (output.trim().chars().take(240).collect::<String>(), None),
        },
    }
}

pub(super) fn parse_token_usage(cli_kind: LivenessCliKind, output: &str) -> ParsedTokenUsage {
    match cli_kind {
        LivenessCliKind::Codex => parse_codex_token_usage(output),
        LivenessCliKind::ClaudeCode => parse_claude_token_usage(output),
    }
}

pub(super) fn failure_record(
    checked_at: String,
    source: String,
    message: String,
    command_preview: String,
    prompt: String,
) -> LivenessRecord {
    LivenessRecord {
        checked_at,
        source,
        cli_kind: String::new(),
        ok: false,
        latency_ms: 0,
        model: String::new(),
        base_url: String::new(),
        prompt,
        response_preview: String::new(),
        response_raw: String::new(),
        input_tokens: None,
        cached_input_tokens: None,
        output_tokens: None,
        reasoning_output_tokens: None,
        total_tokens: None,
        total_cost_usd: None,
        message,
        command_preview,
    }
}

pub(super) fn cli_kind_value(kind: LivenessCliKind) -> String {
    match kind {
        LivenessCliKind::Codex => "codex".to_string(),
        LivenessCliKind::ClaudeCode => "claudeCode".to_string(),
    }
}

pub(super) fn sanitize_output(value: &str) -> String {
    value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(1000)
        .collect()
}

fn parse_codex_token_usage(output: &str) -> ParsedTokenUsage {
    let mut parsed = ParsedTokenUsage::default();
    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(usage) = value.get("usage") else {
            continue;
        };
        parsed.input_tokens = extract_u64(usage, "input_tokens");
        parsed.cached_input_tokens = extract_u64(usage, "cached_input_tokens");
        parsed.output_tokens = extract_u64(usage, "output_tokens");
        parsed.reasoning_output_tokens = extract_u64(usage, "reasoning_output_tokens");
        parsed.total_tokens = token_sum(&[parsed.input_tokens, parsed.output_tokens]);
    }
    parsed
}

fn parse_claude_token_usage(output: &str) -> ParsedTokenUsage {
    let Ok(value) = serde_json::from_str::<Value>(output.trim()) else {
        return ParsedTokenUsage::default();
    };
    let Some(usage) = value.get("usage") else {
        return ParsedTokenUsage {
            total_cost_usd: extract_f64(&value, "total_cost_usd"),
            ..ParsedTokenUsage::default()
        };
    };

    let input_tokens = extract_u64(usage, "input_tokens");
    let cache_creation = extract_u64(usage, "cache_creation_input_tokens");
    let cache_read = extract_u64(usage, "cache_read_input_tokens");
    let cached_input_tokens = token_sum(&[cache_creation, cache_read]);
    let output_tokens = extract_u64(usage, "output_tokens");
    ParsedTokenUsage {
        input_tokens,
        cached_input_tokens,
        output_tokens,
        reasoning_output_tokens: None,
        total_tokens: token_sum(&[input_tokens, cached_input_tokens, output_tokens]),
        total_cost_usd: extract_f64(&value, "total_cost_usd"),
    }
}

fn extract_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn extract_f64(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn token_sum(values: &[Option<u64>]) -> Option<u64> {
    let mut found = false;
    let mut total = 0u64;
    for value in values.iter().flatten() {
        found = true;
        total = total.saturating_add(*value);
    }
    found.then_some(total)
}
