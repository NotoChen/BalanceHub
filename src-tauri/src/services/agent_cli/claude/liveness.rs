use super::super::{
    contracts::{
        EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{extract_f64, extract_u64, token_sum},
};
use serde_json::Value;

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    let mut environment = EnvironmentPatch::default();
    for name in [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "GOOGLE_APPLICATION_CREDENTIALS",
    ] {
        environment.remove(name);
    }
    environment.set("ANTHROPIC_API_KEY", request.api_key.trim());
    environment.set("ANTHROPIC_BASE_URL", request.base_url);
    environment.set("DISABLE_TELEMETRY", "1");
    environment.set("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1");
    environment.set("CLAUDE_CODE_ATTRIBUTION_HEADER", "0");
    environment.set("CLAUDE_CODE_MAX_OUTPUT_TOKENS", "128");
    environment.set("CLAUDE_CODE_MAX_RETRIES", "0");
    environment.set(
        "API_TIMEOUT_MS",
        request.timeout_seconds.saturating_mul(1000).to_string(),
    );
    environment.set(
        "CLAUDE_CONFIG_DIR",
        request.isolated_home.join(".claude").to_string_lossy().to_string(),
    );

    let mut args = vec![
        "--bare".to_string(),
        "-p".to_string(),
        request.prompt.to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "--max-budget-usd".to_string(),
        "0.02".to_string(),
        "--no-session-persistence".to_string(),
        "--tools".to_string(),
        String::new(),
    ];
    if !request.model.trim().is_empty() {
        args.extend(["--model".to_string(), request.model.trim().to_string()]);
    }

    Ok(LivenessPlan {
        args,
        environment,
        files: Vec::new(),
        response_source: LivenessResponseSource::Stdout,
    })
}

pub(super) fn parse_output(response_output: &str, _stdout: &str) -> ParsedLivenessOutput {
    let Ok(value) = serde_json::from_str::<Value>(response_output.trim()) else {
        return ParsedLivenessOutput {
            response: response_output.trim().chars().take(240).collect(),
            ..ParsedLivenessOutput::default()
        };
    };
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
    let usage = if let Some(metrics) = value.get("usage") {
        let input_tokens = extract_u64(metrics, "input_tokens");
        let cache_creation = extract_u64(metrics, "cache_creation_input_tokens");
        let cache_read = extract_u64(metrics, "cache_read_input_tokens");
        let cached_input_tokens = token_sum(&[cache_creation, cache_read]);
        let output_tokens = extract_u64(metrics, "output_tokens");
        ParsedTokenUsage {
            input_tokens,
            cached_input_tokens,
            output_tokens,
            reasoning_output_tokens: None,
            total_tokens: token_sum(&[input_tokens, cached_input_tokens, output_tokens]),
            total_cost_usd: extract_f64(&value, "total_cost_usd"),
        }
    } else {
        ParsedTokenUsage {
            total_cost_usd: extract_f64(&value, "total_cost_usd"),
            ..ParsedTokenUsage::default()
        }
    };
    ParsedLivenessOutput {
        response: if is_error {
            String::new()
        } else {
            result.chars().take(240).collect()
        },
        error: is_error.then_some(result),
        usage,
    }
}
