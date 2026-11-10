use super::super::{
    contracts::{
        EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{extract_u64, token_sum},
};
use serde_json::Value;

const MODE_ENV: &str = "GOOSE_MODE";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let model = request.model.trim();
    let mut environment = EnvironmentPatch::default();
    for name in [
        "GOOSE_PROVIDER",
        "GOOSE_MODEL",
        "GOOSE_PATH_ROOT",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "OPENAI_HOST",
        "OPENAI_BASE_PATH",
    ] {
        environment.remove(name);
    }
    environment.set("GOOSE_PROVIDER", "openai");
    environment.set("GOOSE_MODEL", model);
    environment.set("OPENAI_API_KEY", request.api_key.trim());
    // `OPENAI_HOST` accepts the provider base URL as-is; the base path stays
    // relative so a `/v1` suffix is not duplicated.
    environment.set("OPENAI_HOST", request.base_url.trim());
    environment.set("OPENAI_BASE_PATH", "chat/completions");
    // A probe must not touch the login keychain or report anonymous usage
    // from an installation it does not own.
    environment.set("GOOSE_DISABLE_KEYRING", "1");
    environment.set("GOOSE_TELEMETRY_OFF", "1");
    environment.set(MODE_ENV, "chat");
    Ok(LivenessPlan {
        args: vec![
            "run".to_string(),
            "-t".to_string(),
            request.prompt.to_string(),
            "--no-session".to_string(),
            "--no-profile".to_string(),
            "-q".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
            "--provider".to_string(),
            "openai".to_string(),
            "--model".to_string(),
            model.to_string(),
        ],
        environment,
        files: Vec::new(),
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `--output-format json` prints one object with the full message list and a
/// run-level `metadata` block. The last assistant text is the answer; an
/// assistant `error` block and `status: error` carry the failure detail.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let Ok(value) = serde_json::from_str::<Value>(source.trim()) else {
        return ParsedLivenessOutput::default();
    };
    let mut parsed = ParsedLivenessOutput::default();
    let mut response = None;
    let mut error = None;
    if let Some(messages) = value.get("messages").and_then(Value::as_array) {
        for message in messages {
            if message.get("role").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            let Some(content) = message.get("content").and_then(Value::as_array) else {
                continue;
            };
            let mut text = String::new();
            for block in content {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(chunk) = block.get("text").and_then(Value::as_str) {
                            text.push_str(chunk);
                        }
                    }
                    Some("error") => {
                        error = block
                            .get("message")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|message| !message.is_empty())
                            .map(str::to_owned);
                    }
                    _ => {}
                }
            }
            if !text.trim().is_empty() {
                response = Some(text.trim().to_string());
            }
        }
    }
    let metadata = value.get("metadata");
    if let Some(metadata) = metadata {
        parsed.usage = usage_from(metadata);
        let status = metadata
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !status.is_empty() && status != "completed" && error.is_none() {
            error = Some(format!("Goose 对话未完成: {status}"));
        }
    }
    parsed.error = error;
    if parsed.error.is_none() {
        parsed.response = response
            .map(|value| value.chars().take(240).collect())
            .unwrap_or_default();
    }
    parsed
}

fn usage_from(value: &Value) -> ParsedTokenUsage {
    let input = extract_u64(value, "input_tokens");
    let cached = extract_u64(value, "cache_read_input_tokens");
    let cache_write = extract_u64(value, "cache_write_input_tokens");
    let output = extract_u64(value, "output_tokens");
    let total = extract_u64(value, "total_tokens")
        .or_else(|| token_sum(&[input, cached, cache_write, output]));
    ParsedTokenUsage {
        input_tokens: input,
        cached_input_tokens: cached,
        output_tokens: output,
        reasoning_output_tokens: None,
        total_tokens: total,
        total_cost_usd: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-goose-test",
            base_url: "https://relay.example.com/v1",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 30,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_targets_the_provider_endpoint_without_exposing_the_key() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            [
                "run",
                "-t",
                "Say OK",
                "--no-session",
                "--no-profile",
                "-q",
                "--output-format",
                "json",
                "--provider",
                "openai",
                "--model",
                "relay-model"
            ]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-goose-test")));
        let env: std::collections::HashMap<_, _> = plan.environment.set_values().collect();
        assert_eq!(env.get("OPENAI_HOST"), Some(&"https://relay.example.com/v1"));
        assert_eq!(env.get("OPENAI_BASE_PATH"), Some(&"chat/completions"));
        assert_eq!(env.get("GOOSE_PROVIDER"), Some(&"openai"));
        assert_eq!(env.get("GOOSE_MODEL"), Some(&"relay-model"));
        assert_eq!(env.get("OPENAI_API_KEY"), Some(&"sk-goose-test"));
        assert!(plan.files.is_empty());
    }

    #[test]
    fn parse_output_reads_the_last_assistant_text_and_usage() {
        let stdout = concat!(
            r#"{"messages":["#,
            r#"{"role":"user","content":[{"type":"text","text":"Say OK"}]},"#,
            r#"{"role":"assistant","content":[{"type":"text","text":"BH_OK"}]}"#,
            r#"],"metadata":{"total_tokens":9,"input_tokens":7,"output_tokens":2,"#,
            r#""cache_read_input_tokens":3,"cache_write_input_tokens":1,"status":"completed"}}"#
        );
        let parsed = parse_output(stdout, "");
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(3));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(9));
    }

    #[test]
    fn parse_output_surfaces_an_error_message() {
        let stdout = concat!(
            r#"{"messages":[{"role":"assistant","content":[{"type":"error","kind":"other","#,
            r#""message":"Network error: Could not connect"}]}],"#,
            r#""metadata":{"total_tokens":0,"input_tokens":0,"output_tokens":0,"status":"error"}}"#
        );
        let parsed = parse_output(stdout, "");
        assert!(parsed.response.is_empty());
        assert_eq!(
            parsed.error.as_deref(),
            Some("Network error: Could not connect")
        );
    }

    #[test]
    fn parse_output_reports_a_non_completed_status_without_an_error_block() {
        let stdout = concat!(
            r#"{"messages":[{"role":"assistant","content":[{"type":"text","text":"partial"}]}],"#,
            r#""metadata":{"status":"interrupted"}}"#
        );
        let parsed = parse_output(stdout, "");
        assert!(parsed.response.is_empty());
        assert_eq!(parsed.error.as_deref(), Some("Goose 对话未完成: interrupted"));
    }
}
