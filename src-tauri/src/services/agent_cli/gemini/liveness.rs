use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{add_optional, extract_u64, token_sum},
};
use serde_json::Value;

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    let settings_path = request.isolated_home.join("system-settings.json");
    let settings = serde_json::to_string_pretty(&serde_json::json!({
        "security": {
            "auth": {
                "selectedType": "gemini-api-key"
            }
        }
    }))
    .map_err(|err| format!("生成 Gemini CLI 测活配置失败: {err}"))?;

    let mut environment = EnvironmentPatch::default();
    for name in [
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GOOGLE_GEMINI_BASE_URL",
        "GOOGLE_GENAI_USE_VERTEXAI",
        "GOOGLE_CLOUD_PROJECT",
        "GOOGLE_CLOUD_LOCATION",
        "GOOGLE_APPLICATION_CREDENTIALS",
    ] {
        environment.remove(name);
    }
    environment.set("GEMINI_API_KEY", request.api_key.trim());
    environment.set("GOOGLE_GEMINI_BASE_URL", request.base_url);
    environment.set(
        "GEMINI_CLI_HOME",
        request.isolated_home.to_string_lossy().to_string(),
    );
    environment.set(
        "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
        settings_path.to_string_lossy().to_string(),
    );

    let mut args = vec![
        "-p".to_string(),
        request.prompt.to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "--approval-mode".to_string(),
        "plan".to_string(),
        "--skip-trust".to_string(),
    ];
    if !request.model.trim().is_empty() {
        args.extend(["--model".to_string(), request.model.trim().to_string()]);
    }

    Ok(LivenessPlan {
        args,
        environment,
        files: vec![AgentFilePlan {
            path: settings_path,
            content: settings,
        }],
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
    let response = value
        .get("response")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .chars()
        .take(240)
        .collect::<String>();
    let error = response.is_empty().then(|| {
        value.get("error").and_then(|error| {
            error.as_str().map(str::to_string).or_else(|| {
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
    }).flatten();

    let mut usage = ParsedTokenUsage::default();
    if let Some(models) = value
        .get("stats")
        .and_then(|stats| stats.get("models"))
        .and_then(Value::as_object)
    {
        for metrics in models.values() {
            let Some(tokens) = metrics.get("tokens") else {
                continue;
            };
            add_optional(
                &mut usage.input_tokens,
                extract_u64(tokens, "input").or_else(|| extract_u64(tokens, "prompt")),
            );
            add_optional(&mut usage.cached_input_tokens, extract_u64(tokens, "cached"));
            add_optional(&mut usage.output_tokens, extract_u64(tokens, "candidates"));
            add_optional(
                &mut usage.reasoning_output_tokens,
                extract_u64(tokens, "thoughts"),
            );
            add_optional(&mut usage.total_tokens, extract_u64(tokens, "total"));
        }
    }
    if usage.total_tokens.is_none() {
        usage.total_tokens = token_sum(&[
            usage.input_tokens,
            usage.output_tokens,
            usage.reasoning_output_tokens,
        ]);
    }
    ParsedLivenessOutput {
        response,
        error,
        usage,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_output;

    #[test]
    fn json_output_exposes_response_and_aggregated_tokens() {
        let output = r#"{
          "response": "pong",
          "stats": {
            "models": {
              "gemini-2.5-pro": {"tokens": {"input": 3, "candidates": 1, "total": 4, "cached": 0}},
              "gemini-2.5-flash": {"tokens": {"prompt": 2, "candidates": 2, "thoughts": 1, "total": 5, "cached": 1}}
            }
          }
        }"#;

        let parsed = parse_output(output, output);
        assert_eq!(parsed.response, "pong");
        assert_eq!(parsed.error, None);
        assert_eq!(parsed.usage.input_tokens, Some(5));
        assert_eq!(parsed.usage.cached_input_tokens, Some(1));
        assert_eq!(parsed.usage.output_tokens, Some(3));
        assert_eq!(parsed.usage.reasoning_output_tokens, Some(1));
        assert_eq!(parsed.usage.total_tokens, Some(9));
    }
}
