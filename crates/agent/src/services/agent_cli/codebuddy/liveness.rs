use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{extract_u64, token_sum},
};
use serde_json::{Value, json};

const VENDOR: &str = "BalanceHub";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let root = request.isolated_home.join("codebuddy");
    let model = request.model.trim();
    let settings = serde_json::to_string_pretty(&json!({ "model": model }))
        .map_err(|err| format!("生成 CodeBuddy 测活设置失败: {err}"))?;
    let models = serde_json::to_string_pretty(&json!({
        "models": [{
            "id": model,
            "name": model,
            "vendor": VENDOR,
            "apiKey": request.api_key.trim(),
            "url": format!("{}/chat/completions", request.base_url.trim().trim_end_matches('/')),
            "supportsToolCall": true,
            "supportsImages": false,
            "supportsReasoning": false,
        }],
    }))
    .map_err(|err| format!("生成 CodeBuddy 测活模型失败: {err}"))?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "CODEBUDDY_CONFIG_DIR",
        "CODEBUDDY_API_KEY",
        "CODEBUDDY_BASE_URL",
        "CODEBUDDY_MODEL",
        "CODEBUDDY_BIG_SLOW_MODEL",
        "CODEBUDDY_SMALL_FAST_MODEL",
        "CODEBUDDY_CODE_SUBAGENT_MODEL",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set_path("CODEBUDDY_CONFIG_DIR", &root);
    Ok(LivenessPlan {
        args: vec![
            "--print".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
            "--tools".to_string(),
            String::new(),
            "--no-session-persistence".to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: vec![
            AgentFilePlan {
                path: root.join("settings.json"),
                content: settings,
            },
            AgentFilePlan {
                path: root.join("models.json"),
                content: models,
            },
        ],
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `--output-format json` prints the full message array; its terminal `result`
/// item carries the answer, error flag and token usage.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let Ok(value) = serde_json::from_str::<Value>(source.trim()) else {
        return ParsedLivenessOutput::default();
    };
    let items = match value {
        Value::Array(items) => items,
        value => vec![value],
    };
    let mut fallback = String::new();
    for item in &items {
        if item.get("type").and_then(Value::as_str) != Some("result") {
            if item.get("type").and_then(Value::as_str) == Some("message")
                && item.get("role").and_then(Value::as_str) == Some("assistant")
            {
                if let Some(text) = message_text(item) {
                    fallback = text;
                }
            }
            continue;
        }
        let text = item
            .get("result")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let usage = item.get("usage").map(usage_from).unwrap_or_default();
        if item
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return ParsedLivenessOutput {
                response: String::new(),
                error: Some(if text.is_empty() {
                    "CodeBuddy 测活执行失败".to_string()
                } else {
                    text
                }),
                usage,
            };
        }
        return ParsedLivenessOutput {
            response: if text.is_empty() { fallback } else { text },
            error: None,
            usage,
        };
    }
    ParsedLivenessOutput {
        response: fallback,
        ..ParsedLivenessOutput::default()
    }
}

fn message_text(item: &Value) -> Option<String> {
    let content = item.get("content")?.as_array()?;
    let text = content
        .iter()
        .filter_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<String>();
    (!text.trim().is_empty()).then(|| text.trim().to_string())
}

fn usage_from(value: &Value) -> ParsedTokenUsage {
    let input = extract_u64(value, "input_tokens");
    let cache_read = extract_u64(value, "cache_read_input_tokens");
    let cache_write = extract_u64(value, "cache_creation_input_tokens");
    let output = extract_u64(value, "output_tokens");
    ParsedTokenUsage {
        input_tokens: input,
        cached_input_tokens: cache_read,
        output_tokens: output,
        reasoning_output_tokens: None,
        total_tokens: token_sum(&[input, cache_read, cache_write, output]),
        total_cost_usd: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-codebuddy-test",
            base_url: "https://relay.example.com/v1",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 30,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_writes_an_isolated_provider_config() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            [
                "--print",
                "--output-format",
                "json",
                "--tools",
                "",
                "--no-session-persistence",
                "Say OK"
            ]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-codebuddy-test")));
        assert!(plan.environment.set_values().any(|(name, value)| {
            name == "CODEBUDDY_CONFIG_DIR" && value.ends_with("/codebuddy")
        }));
        // 原生第三方模型变量会覆盖隔离的 settings.json / models.json。
        let removed = plan.environment.removed_names().collect::<Vec<_>>();
        assert!(removed.contains(&"CODEBUDDY_BASE_URL"));
        assert!(removed.contains(&"CODEBUDDY_MODEL"));
        let models = plan
            .files
            .iter()
            .find(|file| file.path.ends_with("models.json"))
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&models.content).unwrap();
        assert_eq!(
            value["models"][0]["url"],
            "https://relay.example.com/v1/chat/completions"
        );
        assert_eq!(value["models"][0]["vendor"], VENDOR);
    }

    #[test]
    fn parse_output_reads_the_result_and_usage() {
        let stdout = concat!(
            r#"[{"type":"message","role":"assistant","content":[{"type":"text","text":"BH_OK"}]},"#,
            r#"{"type":"result","result":"BH_OK","is_error":false,"usage":{"input_tokens":7,"output_tokens":2,"cache_creation_input_tokens":1,"cache_read_input_tokens":3}}]"#
        );
        let parsed = parse_output(stdout, "");
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(3));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(13));
    }

    #[test]
    fn parse_output_surfaces_an_error_result() {
        let stdout =
            r#"[{"type":"result","result":"invalid api key","is_error":true,"usage":{}}]"#;
        let parsed = parse_output(stdout, "");
        assert!(parsed.response.is_empty());
        assert_eq!(parsed.error.as_deref(), Some("invalid api key"));
    }
}
