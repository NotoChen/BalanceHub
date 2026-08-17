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
        "GROK_HOME",
        "GROK_MODELS_BASE_URL",
        "GROK_MODELS_LIST_URL",
        "XAI_API_KEY",
        "GROK_DISABLE_AUTOUPDATER",
        "GROK_TELEMETRY_ENABLED",
        "GROK_TELEMETRY_TRACE_UPLOAD",
        "GROK_TRACE_UPLOAD",
        "DISABLE_TELEMETRY",
    ] {
        environment.remove(name);
    }
    environment.set(
        "GROK_HOME",
        request.isolated_home.to_string_lossy().to_string(),
    );
    environment.set("GROK_MODELS_BASE_URL", request.base_url.trim());
    environment.set("XAI_API_KEY", request.api_key.trim());
    environment.set("GROK_DISABLE_AUTOUPDATER", "1");
    environment.set("GROK_TELEMETRY_ENABLED", "false");
    environment.set("GROK_TELEMETRY_TRACE_UPLOAD", "false");
    environment.set("GROK_TRACE_UPLOAD", "false");
    environment.set("DISABLE_TELEMETRY", "1");

    let mut args = vec![
        "--cwd".to_string(),
        request.isolated_home.to_string_lossy().to_string(),
        "-p".to_string(),
        request.prompt.to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "--permission-mode".to_string(),
        "plan".to_string(),
        "--max-turns".to_string(),
        "1".to_string(),
        "--no-subagents".to_string(),
        "--disable-web-search".to_string(),
        "--no-memory".to_string(),
        "--no-auto-update".to_string(),
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
    let trimmed = response_output.trim();
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return ParsedLivenessOutput {
            response: trimmed.chars().take(240).collect(),
            ..ParsedLivenessOutput::default()
        };
    };

    let error = (value.get("type").and_then(Value::as_str) == Some("error"))
        .then(|| {
            value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Grok Build 测活失败")
                .to_string()
        });
    let response = value
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .chars()
        .take(240)
        .collect::<String>();
    let mut usage = ParsedTokenUsage::default();
    if let Some(metrics) = value.get("usage") {
        usage.input_tokens = extract_u64(metrics, "input_tokens");
        usage.cached_input_tokens = token_sum(&[
            extract_u64(metrics, "cache_read_input_tokens"),
            extract_u64(metrics, "cache_creation_input_tokens"),
        ]);
        usage.output_tokens = extract_u64(metrics, "output_tokens");
        usage.reasoning_output_tokens = extract_u64(metrics, "reasoning_tokens");
        usage.total_tokens = extract_u64(metrics, "total_tokens").or_else(|| {
            token_sum(&[
                usage.input_tokens,
                usage.cached_input_tokens,
                usage.output_tokens,
            ])
        });
    }
    usage.total_cost_usd = extract_f64(&value, "total_cost_usd");

    ParsedLivenessOutput {
        response,
        error,
        usage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn liveness_plan_is_isolated_and_non_mutating() {
        let root = Path::new("/tmp/balancehub-grok-liveness");
        let plan = build_plan(LivenessRequest {
            api_key: "xai-test",
            base_url: "https://relay.example.com/v1",
            model: "grok-code-fast-1",
            prompt: "pong",
            timeout_seconds: 30,
            isolated_home: root,
            output_path: &root.join("response.txt"),
        })
        .unwrap();

        assert!(plan.args.windows(2).any(|args| args == ["--cwd", "/tmp/balancehub-grok-liveness"]));
        assert!(plan.args.windows(2).any(|args| args == ["--permission-mode", "plan"]));
        assert!(plan.args.windows(2).any(|args| args == ["--tools", ""]));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "GROK_HOME" && value == root.to_string_lossy()));
    }

    #[test]
    fn json_output_exposes_text_cost_and_disjoint_token_buckets() {
        let parsed = parse_output(
            r#"{
              "text": "pong",
              "usage": {
                "input_tokens": 3,
                "cache_read_input_tokens": 5,
                "cache_creation_input_tokens": 7,
                "output_tokens": 2,
                "reasoning_tokens": 1,
                "total_tokens": 17
              },
              "total_cost_usd": 0.0125
            }"#,
            "",
        );

        assert_eq!(parsed.response, "pong");
        assert_eq!(parsed.error, None);
        assert_eq!(parsed.usage.input_tokens, Some(3));
        assert_eq!(parsed.usage.cached_input_tokens, Some(12));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.reasoning_output_tokens, Some(1));
        assert_eq!(parsed.usage.total_tokens, Some(17));
        assert_eq!(parsed.usage.total_cost_usd, Some(0.0125));
    }

    #[test]
    fn error_object_does_not_become_a_successful_response() {
        let parsed = parse_output(
            r#"{"type":"error","message":"invalid api key"}"#,
            "",
        );

        assert!(parsed.response.is_empty());
        assert_eq!(parsed.error.as_deref(), Some("invalid api key"));
    }
}
