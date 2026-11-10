use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::extract_u64,
};
use serde_json::Value;

const PROVIDER_ID: &str = "balancehub";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let model = request.model.trim();
    let config = super::config::temporary_config(model, request.base_url.trim(), Some(1))?;
    let config_path = request.isolated_home.join("config.toml");
    let mut environment = EnvironmentPatch::default();
    for name in [
        "ATOMCODE_HOME",
        "ATOMCODE_API_KEY",
        "ATOMCODE_SEED_CONFIG",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "OPENAI_API_BASE",
        "ANTHROPIC_API_KEY",
        "OLLAMA_API_KEY",
    ] {
        environment.remove(name);
    }
    // 隔离的 ATOMCODE_HOME 让 `--ephemeral` 之外也不会碰到原生会话与插件。
    environment.set_path("ATOMCODE_HOME", request.isolated_home);
    environment.set("BALANCEHUB_ATOMCODE_API_KEY", request.api_key.trim());
    Ok(LivenessPlan {
        args: vec![
            "--config".to_string(),
            config_path.to_string_lossy().into_owned(),
            "-p".to_string(),
            request.prompt.to_string(),
            "--output-format".to_string(),
            "jsonl".to_string(),
            "--ephemeral".to_string(),
            "--no-tools".to_string(),
            "--dev".to_string(),
            "--no-telemetry".to_string(),
            "--provider".to_string(),
            PROVIDER_ID.to_string(),
            "--model".to_string(),
            model.to_string(),
        ],
        environment,
        files: vec![AgentFilePlan {
            path: config_path,
            content: config,
        }],
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `--output-format jsonl` emits one JSON object per line. `message.delta`
/// carries the answer, `usage` the per-round tokens, `error` the failure and
/// `turn.completed` the terminal stop reason.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if stdout.trim().is_empty() {
        response_output
    } else {
        stdout
    };
    let mut text = String::new();
    let mut usage = ParsedTokenUsage::default();
    let mut error = None;
    let mut stop_reason = None;
    let mut completed = false;
    for line in source.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("message.delta") => {
                if let Some(chunk) = event.get("text").and_then(Value::as_str) {
                    text.push_str(chunk);
                }
            }
            Some("usage") => {
                add_total(&mut usage.input_tokens, extract_u64(&event, "prompt_tokens"));
                add_total(
                    &mut usage.output_tokens,
                    extract_u64(&event, "completion_tokens"),
                );
                add_total(
                    &mut usage.cached_input_tokens,
                    extract_u64(&event, "cached_tokens"),
                );
            }
            Some("error") => {
                error = Some(
                    event
                        .get("message")
                        .and_then(Value::as_str)
                        .map(|message| format!("AtomCode 测活失败: {message}"))
                        .unwrap_or_else(|| "AtomCode 测活执行失败".to_string()),
                );
            }
            Some("turn.completed") => {
                completed = true;
                stop_reason = event
                    .get("stop_reason")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if let Some(value) = extract_u64(&event, "prompt_tokens") {
                    usage.input_tokens = Some(value);
                }
                if let Some(value) = extract_u64(&event, "completion_tokens") {
                    usage.output_tokens = Some(value);
                }
                if let Some(value) = extract_u64(&event, "cached_tokens") {
                    usage.cached_input_tokens = Some(value);
                }
                if let Some(value) = extract_u64(&event, "total_tokens") {
                    usage.total_tokens = Some(value);
                }
            }
            _ => {}
        }
    }
    if error.is_none() {
        match stop_reason.as_deref() {
            Some("Stopped") => {}
            Some(reason) => error = Some(format!("AtomCode 对话未完成: {reason}")),
            None if completed => error = Some("AtomCode 对话未返回结束原因".to_string()),
            None => error = Some("AtomCode 测活未返回完成事件".to_string()),
        }
    }
    if usage.total_tokens.is_none() {
        usage.total_tokens = match (usage.input_tokens, usage.output_tokens) {
            (Some(input), Some(output)) => Some(input.saturating_add(output)),
            (input, output) => input.or(output),
        };
    }
    ParsedLivenessOutput {
        response: text.trim().chars().take(240).collect(),
        error,
        usage,
    }
}

fn add_total(total: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        *total = Some(total.unwrap_or(0).saturating_add(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-atomcode-test",
            base_url: "https://relay.example.com",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 60,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_writes_an_isolated_account_config_and_keeps_the_key_in_env() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            crate::test_support::slash_args(&plan.args),
            [
                "--config",
                "/tmp/<liveness>/config.toml",
                "-p",
                "Say OK",
                "--output-format",
                "jsonl",
                "--ephemeral",
                "--no-tools",
                "--dev",
                "--no-telemetry",
                "--provider",
                "balancehub",
                "--model",
                "relay-model",
            ]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-atomcode-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "ATOMCODE_HOME" && value == "/tmp/<liveness>"));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "BALANCEHUB_ATOMCODE_API_KEY"
                && value == "sk-atomcode-test"));
        assert!(plan
            .environment
            .removed_names()
            .any(|name| name == "OPENAI_API_KEY"));
        let parsed: toml::Value = plan.files[0].content.parse().unwrap();
        assert_eq!(
            parsed["default_provider"].as_str(),
            Some("balancehub/relay-model")
        );
        assert_eq!(
            parsed["provider_accounts"]["balancehub"]["base_url"].as_str(),
            Some("https://relay.example.com/v1")
        );
        assert_eq!(
            parsed["provider_accounts"]["balancehub"]["api_key"].as_str(),
            Some("${BALANCEHUB_ATOMCODE_API_KEY}")
        );
        assert_eq!(
            parsed["models"]["balancehub/relay-model"]["retry_max_attempts"].as_integer(),
            Some(1)
        );
    }

    #[test]
    fn parse_output_collects_text_usage_and_success() {
        let parsed = parse_output(
            "",
            concat!(
                r#"{"type":"run.started","schema_version":1,"provider":"balancehub","model":"relay-model"}"#,
                "\n",
                r#"{"type":"message.delta","text":"BH_"}"#,
                "\n",
                r#"{"type":"message.delta","text":"OK"}"#,
                "\n",
                r#"{"type":"usage","round":1,"prompt_tokens":7,"completion_tokens":2,"cached_tokens":3}"#,
                "\n",
                r#"{"type":"turn.completed","stop_reason":"Stopped","exit_code":0,"total_tokens":9,"prompt_tokens":7,"completion_tokens":2,"cached_tokens":3}"#,
                "\n",
            ),
        );
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(3));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(9));
    }

    #[test]
    fn parse_output_surfaces_provider_errors() {
        let parsed = parse_output(
            "",
            concat!(
                r#"{"type":"error","message":"open failed: connection refused","code":null}"#,
                "\n",
                r#"{"type":"turn.completed","stop_reason":"ProviderError","exit_code":1,"total_tokens":0}"#,
                "\n",
            ),
        );
        assert!(parsed.response.is_empty());
        assert_eq!(
            parsed.error.as_deref(),
            Some("AtomCode 测活失败: open failed: connection refused")
        );
    }

    #[test]
    fn parse_output_rejects_a_non_success_stop_reason() {
        let parsed = parse_output(
            r#"{"type":"turn.completed","stop_reason":"MaxRounds","exit_code":0,"total_tokens":4}"#,
            "",
        );
        assert_eq!(parsed.error.as_deref(), Some("AtomCode 对话未完成: MaxRounds"));
    }

    #[test]
    fn parse_output_requires_a_completion_event() {
        let parsed = parse_output(
            r#"{"type":"message.delta","text":"partial"}"#,
            "",
        );
        assert_eq!(parsed.response, "partial");
        assert_eq!(
            parsed.error.as_deref(),
            Some("AtomCode 测活未返回完成事件")
        );
    }
}
