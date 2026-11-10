use super::super::{
    contracts::{
        EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{extract_u64, token_sum},
};
use serde_json::Value;

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "QWEN_HOME",
        "QWEN_RUNTIME_DIR",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "OPENAI_MODEL",
        "QWEN_MODEL",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
    ] {
        environment.remove(name);
    }
    environment.set("OPENAI_API_KEY", request.api_key.trim());
    environment.set("OPENAI_BASE_URL", request.base_url.trim());
    Ok(LivenessPlan {
        args: vec![
            "--bare".to_string(),
            "--auth-type".to_string(),
            "openai".to_string(),
            "--openai-base-url".to_string(),
            request.base_url.trim().to_string(),
            "--model".to_string(),
            request.model.trim().to_string(),
            "--output-format".to_string(),
            "json".to_string(),
            "--max-tool-calls".to_string(),
            "0".to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: Vec::new(),
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `--output-format json` prints one JSON array of CLI events; the `result`
/// event carries the answer, error flag and token usage.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let Ok(events) = serde_json::from_str::<Value>(source.trim()) else {
        return ParsedLivenessOutput {
            response: source.trim().chars().take(240).collect(),
            ..ParsedLivenessOutput::default()
        };
    };
    let result = events
        .as_array()
        .and_then(|events| {
            events
                .iter()
                .rev()
                .find(|event| event.get("type").and_then(Value::as_str) == Some("result"))
        })
        .or_else(|| (events.get("type").and_then(Value::as_str) == Some("result")).then_some(&events));
    let Some(result) = result else {
        return ParsedLivenessOutput {
            response: source.trim().chars().take(240).collect(),
            ..ParsedLivenessOutput::default()
        };
    };
    let is_error = result
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text = result
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let usage = result
        .get("usage")
        .map(|usage| {
            let input_tokens = extract_u64(usage, "input_tokens");
            let cached_input_tokens = extract_u64(usage, "cache_read_input_tokens");
            let output_tokens = extract_u64(usage, "output_tokens");
            ParsedTokenUsage {
                input_tokens,
                cached_input_tokens,
                output_tokens,
                reasoning_output_tokens: extract_u64(usage, "thoughts_tokens"),
                total_tokens: extract_u64(usage, "total_tokens").or_else(|| {
                    token_sum(&[input_tokens, cached_input_tokens, output_tokens])
                }),
                total_cost_usd: None,
            }
        })
        .unwrap_or_default();
    ParsedLivenessOutput {
        response: if is_error {
            String::new()
        } else {
            text.chars().take(240).collect()
        },
        error: is_error.then_some(text),
        usage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>(api_key: &'a str, base_url: &'a str) -> LivenessRequest<'a> {
        LivenessRequest {
            api_key,
            base_url,
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 30,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_keeps_the_key_in_the_environment_and_disables_tools() {
        let plan = build_plan(request("sk-qwen-test", "https://relay.example.com/v1")).unwrap();
        assert_eq!(
            plan.args,
            [
                "--bare",
                "--auth-type",
                "openai",
                "--openai-base-url",
                "https://relay.example.com/v1",
                "--model",
                "relay-model",
                "--output-format",
                "json",
                "--max-tool-calls",
                "0",
                "Say OK",
            ]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-qwen-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "OPENAI_API_KEY" && value == "sk-qwen-test"));
        // QWEN_HOME / QWEN_RUNTIME_DIR 会绕过隔离的 HOME，必须显式清掉，
        // 否则测活会读写用户原生配置与会话目录。
        let removed = plan.environment.removed_names().collect::<Vec<_>>();
        assert!(removed.contains(&"QWEN_HOME"));
        assert!(removed.contains(&"QWEN_RUNTIME_DIR"));
    }

    #[test]
    fn result_event_supplies_the_answer_and_usage() {
        let parsed = parse_output(
            r#"[{"type":"system","model":"relay-model"},
{"type":"result","subtype":"success","is_error":false,"result":"OK",
 "usage":{"input_tokens":7,"output_tokens":2,"cache_read_input_tokens":3,"total_tokens":9}}]"#,
            "",
        );
        assert_eq!(parsed.response, "OK");
        assert_eq!(parsed.error, None);
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(3));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(9));
    }

    #[test]
    fn error_result_surfaces_the_message() {
        let parsed = parse_output(
            r#"[{"type":"result","subtype":"error_max_turns","is_error":true,"result":"turn limit"}]"#,
            "",
        );
        assert_eq!(parsed.response, "");
        assert_eq!(parsed.error.as_deref(), Some("turn limit"));
    }
}
