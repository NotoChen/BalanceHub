use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{extract_f64, extract_u64, token_sum},
};
use serde_json::{Value, json};

const PROVIDER_ID: &str = "openai-compatible";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let config_dir = request.isolated_home.join("cline");
    let data_dir = config_dir.join("data");
    let providers = serde_json::to_string_pretty(&json!({
        "version": 1,
        "lastUsedProvider": PROVIDER_ID,
        "modes": {},
        "providers": {
            PROVIDER_ID: {
                "settings": {
                    "provider": PROVIDER_ID,
                    "apiKey": request.api_key.trim(),
                    "model": request.model.trim(),
                    "baseUrl": request.base_url.trim(),
                },
                "updatedAt": "2000-01-01T00:00:00.000Z",
                "tokenSource": "manual",
            }
        },
    }))
    .map_err(|err| format!("生成 Cline 测活配置失败: {err}"))?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "CLINE_DIR",
        "CLINE_DATA_DIR",
        "CLINE_SESSION_DATA_DIR",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set_path("CLINE_DIR", &config_dir);
    environment.set_path("CLINE_DATA_DIR", &data_dir);
    Ok(LivenessPlan {
        args: vec![
            "--json".to_string(),
            "--plan".to_string(),
            "-P".to_string(),
            PROVIDER_ID.to_string(),
            "-m".to_string(),
            request.model.trim().to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: vec![AgentFilePlan {
            path: super::config::providers_path(&data_dir),
            content: providers,
        }],
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `--json` emits one event per line. `run_result` is terminal and carries the
/// final answer plus its usage; a non-completed reason keeps its text as the
/// failure detail instead of pretending the probe answered.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let mut parsed = ParsedLivenessOutput::default();
    for line in source.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("run_result") => {
                let reason = event
                    .get("finishReason")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let text = event.get("text").and_then(Value::as_str).unwrap_or_default();
                if let Some(usage) = event.get("usage") {
                    parsed.usage = usage_from(usage);
                }
                if reason == "completed" {
                    if !text.trim().is_empty() {
                        parsed.response = text.trim().to_string();
                    }
                } else {
                    parsed.response.clear();
                    parsed.error = Some(if text.trim().is_empty() {
                        format!("Cline 对话未完成: {reason}")
                    } else {
                        text.trim().to_string()
                    });
                }
            }
            Some("agent_event") => {
                let Some(inner) = event.get("event") else {
                    continue;
                };
                match inner.get("type").and_then(Value::as_str) {
                    Some("done") => {
                        fallback_response(&mut parsed, inner);
                        if let Some(usage) = inner.get("usage") {
                            let usage = usage_from(usage);
                            if usage.total_tokens.is_some() || usage.output_tokens.is_some() {
                                parsed.usage = usage;
                            }
                        }
                    }
                    Some("content_end") => fallback_response(&mut parsed, inner),
                    Some("error") => {
                        parsed.error = Some(error_message(inner));
                    }
                    _ => {}
                }
            }
            Some("error") => {
                parsed.error = Some(error_message(&event));
            }
            _ => {}
        }
    }
    parsed
}

fn fallback_response(parsed: &mut ParsedLivenessOutput, value: &Value) {
    if parsed.response.is_empty() {
        if let Some(text) = value.get("text").and_then(Value::as_str) {
            parsed.response = text.trim().to_string();
        }
    }
}

fn usage_from(value: &Value) -> ParsedTokenUsage {
    let input = extract_u64(value, "inputTokens");
    let cached = extract_u64(value, "cacheReadTokens");
    let cache_write = extract_u64(value, "cacheWriteTokens");
    let output = extract_u64(value, "outputTokens");
    let total = token_sum(&[input, cached, cache_write, output]);
    ParsedTokenUsage {
        input_tokens: input,
        cached_input_tokens: cached,
        output_tokens: output,
        reasoning_output_tokens: None,
        total_tokens: total,
        total_cost_usd: extract_f64(value, "totalCost").filter(|value| *value > 0.0),
    }
}

fn error_message(value: &Value) -> String {
    value
        .get("message")
        .or_else(|| value.get("error").and_then(|error| error.get("message")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .unwrap_or("Cline 测活执行失败")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-cline-test",
            base_url: "https://relay.example.com/v1",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 30,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_isolates_the_cli_directories_and_keeps_the_key_out_of_argv() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            ["--json", "--plan", "-P", PROVIDER_ID, "-m", "relay-model", "Say OK"]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-cline-test")));
        assert!(plan.environment.set_values().any(|(name, value)| {
            name == "CLINE_DATA_DIR" && value.ends_with("/cline/data")
        }));
        let file = plan.files.first().unwrap();
        assert_eq!(
            file.path,
            Path::new("/tmp/<liveness>/cline/data/settings/providers.json")
        );
        let value: serde_json::Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(
            value["providers"][PROVIDER_ID]["settings"]["baseUrl"],
            "https://relay.example.com/v1"
        );
    }

    #[test]
    fn parse_output_reads_the_final_answer_and_usage() {
        let stdout = concat!(
            r#"{"type":"agent_event","event":{"type":"content_end","contentType":"text","text":"BH_OK"}}"#,
            "\n",
            r#"{"type":"agent_event","event":{"type":"done","reason":"completed","text":"BH_OK","usage":{"inputTokens":7,"outputTokens":2,"totalCost":0}}}"#,
            "\n",
            r#"{"type":"run_result","finishReason":"completed","usage":{"inputTokens":7,"outputTokens":2,"cacheReadTokens":3,"cacheWriteTokens":1,"totalCost":0.01},"text":"BH_OK"}"#,
            "\n"
        );
        let parsed = parse_output(stdout, "");
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(3));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(13));
        assert_eq!(parsed.usage.total_cost_usd, Some(0.01));
    }

    #[test]
    fn parse_output_surfaces_a_failed_run() {
        let stdout = concat!(
            r#"{"type":"agent_event","event":{"type":"error","error":{"message":"Cannot connect to API"},"recoverable":false}}"#,
            "\n",
            r#"{"type":"run_result","finishReason":"error","text":"Cannot connect to API","usage":{}}"#,
            "\n",
            r#"{"type":"error","message":"Cannot connect to API"}"#,
            "\n"
        );
        let parsed = parse_output(stdout, "");
        assert!(parsed.response.is_empty());
        assert_eq!(parsed.error.as_deref(), Some("Cannot connect to API"));
    }
}
