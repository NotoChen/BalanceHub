use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{extract_u64, token_sum},
};
use serde_json::{Map, Value, json};

const PROVIDER_ID: &str = "balancehub";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "Model ID")?;
    let model = request.model.trim();
    let root = request.isolated_home.join(".mothx");
    let mut provider = Map::new();
    provider.insert(
        "baseUrl".into(),
        Value::String(super::super::config_support::openai_base_url(
            request.base_url.trim(),
        )),
    );
    provider.insert("api".into(), Value::String("openai-chat".into()));
    provider.insert(
        "apiKey".into(),
        Value::String(request.api_key.trim().to_string()),
    );
    provider.insert("thinkingFormat".into(), Value::String("openai".into()));
    provider.insert(
        "models".into(),
        json!([{ "id": model, "name": model, "input": ["text"] }]),
    );
    let mut providers = Map::new();
    providers.insert(PROVIDER_ID.to_string(), Value::Object(provider));
    let settings = serde_json::to_string_pretty(&json!({
        "defaultProvider": PROVIDER_ID,
        "defaultModel": model,
        "providers": providers,
    }))
    .map_err(|err| format!("生成 MothX 测活设置失败: {err}"))?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "MOTHX_DIR",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "DEEPSEEK_API_KEY",
    ] {
        environment.remove(name);
    }
    environment.set_path("MOTHX_DIR", &root);
    Ok(LivenessPlan {
        args: vec![
            "-P".to_string(),
            "--json".to_string(),
            "-p".to_string(),
            PROVIDER_ID.to_string(),
            "-m".to_string(),
            model.to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: vec![AgentFilePlan {
            path: root.join("settings.json"),
            content: settings,
        }],
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `-P --json` streams NDJSON: `text_delta` events carry the answer,
/// `usage`/`done` carry token counts, and `done.stop_reason` reports the end.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let mut text = String::new();
    let mut usage = ParsedTokenUsage::default();
    let mut error = None;
    for line in source.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("text_delta") => {
                if let Some(chunk) = event.get("text").and_then(Value::as_str) {
                    text.push_str(chunk);
                }
            }
            Some("usage") | Some("done") => {
                if let Some(tokens) = event.get("usage") {
                    add_usage(&mut usage, tokens);
                }
                if event.get("type").and_then(Value::as_str) == Some("done") {
                    if let Some(reason) = event.get("stop_reason").and_then(Value::as_str) {
                        if matches!(reason, "error" | "aborted") {
                            error = Some(format!("MothX 对话未完成: {reason}"));
                        }
                    }
                }
            }
            Some("error") => {
                error = event
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| Some("MothX 测活执行失败".to_string()));
            }
            _ => {}
        }
    }
    if usage.total_tokens.is_none() {
        usage.total_tokens = token_sum(&[
            usage.input_tokens,
            usage.cached_input_tokens,
            usage.output_tokens,
            usage.reasoning_output_tokens,
        ]);
    }
    ParsedLivenessOutput {
        response: text.trim().chars().take(240).collect(),
        error,
        usage,
    }
}

fn add_usage(usage: &mut ParsedTokenUsage, tokens: &Value) {
    if let Some(value) = extract_u64(tokens, "input") {
        usage.input_tokens = Some(value);
    }
    if let Some(value) = extract_u64(tokens, "output") {
        usage.output_tokens = Some(value);
    }
    if let Some(value) = extract_u64(tokens, "cacheRead") {
        usage.cached_input_tokens = Some(value);
    }
    if let Some(value) = extract_u64(tokens, "totalTokens") {
        usage.total_tokens = Some(value);
    }
    if let Some(cost) = tokens
        .get("cost")
        .and_then(|cost| cost.get("total"))
        .and_then(Value::as_f64)
        .filter(|cost| *cost > 0.0)
    {
        usage.total_cost_usd = Some(cost);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-mothx-test",
            base_url: "https://relay.example.com/v1",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 60,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_writes_the_isolated_settings_and_keeps_the_key_out_of_argv() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            ["-P", "--json", "-p", "balancehub", "-m", "relay-model", "Say OK"]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-mothx-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "MOTHX_DIR" && crate::test_support::same_path(value, "/tmp/<liveness>/.mothx")));
        let file = &plan.files[0];
        assert_eq!(file.path, request().isolated_home.join(".mothx/settings.json"));
        let parsed: Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(parsed["defaultProvider"], "balancehub");
        assert_eq!(
            parsed["providers"]["balancehub"]["baseUrl"],
            "https://relay.example.com/v1"
        );
        assert_eq!(
            parsed["providers"]["balancehub"]["apiKey"],
            "sk-mothx-test"
        );
    }

    #[test]
    fn parse_output_collects_text_and_usage() {
        let parsed = parse_output(
            concat!(
                r#"{"type":"start","provider":"openai","model":"relay-model"}"#,
                "\n",
                r#"{"type":"text_delta","text":"BH_"}"#,
                "\n",
                r#"{"type":"text_delta","text":"OK"}"#,
                "\n",
                r#"{"type":"usage","usage":{"input":7,"output":2,"cacheRead":3,"totalTokens":9}}"#,
                "\n",
                r#"{"type":"done","usage":{"input":7,"output":2,"cacheRead":3,"totalTokens":9},"stop_reason":"stop"}"#,
                "\n",
            ),
            "",
        );
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(3));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(9));
    }

    #[test]
    fn parse_output_surfaces_a_failed_stop_reason() {
        let parsed = parse_output(
            r#"{"type":"done","usage":{"input":0,"output":0,"totalTokens":0},"stop_reason":"error"}"#,
            "",
        );
        assert!(parsed.response.is_empty());
        assert_eq!(parsed.error.as_deref(), Some("MothX 对话未完成: error"));
    }
}
