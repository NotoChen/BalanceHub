use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{add_optional, extract_u64, token_sum},
};
use serde_json::{Value, json};

const MODEL_PREFIX: &str = "custom:balancehub/";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let root = request.isolated_home.join("droid");
    let settings_path = root.join("settings.json");
    let model = request.model.trim();
    let settings = serde_json::to_string_pretty(&json!({
        "customModels": [{
            "id": format!("{MODEL_PREFIX}{model}"),
            "displayName": model,
            "model": model,
            "provider": "generic-chat-completion-api",
            "baseUrl": super::super::config_support::openai_base_url(
                request.base_url.trim(),
            ),
            "apiKey": request.api_key.trim(),
            "noImageSupport": true,
        }],
        "sessionDefaultSettings": { "model": format!("{MODEL_PREFIX}{model}") },
    }))
    .map_err(|err| format!("生成 Droid 测活设置失败: {err}"))?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "FACTORY_HOME_OVERRIDE",
        "FACTORY_API_KEY",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    Ok(LivenessPlan {
        args: vec![
            "exec".to_string(),
            "-m".to_string(),
            format!("{MODEL_PREFIX}{model}"),
            "--settings".to_string(),
            settings_path.to_string_lossy().into_owned(),
            "-o".to_string(),
            "json".to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: vec![AgentFilePlan {
            path: settings_path,
            content: settings,
        }],
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `exec -o json` prints one result object. A failed turn still carries the
/// reason in `result` with `is_error: true` and exit code 1.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let Ok(value) = serde_json::from_str::<Value>(source.trim()) else {
        return ParsedLivenessOutput::default();
    };
    let usage = value.get("usage").map(usage_from).unwrap_or_default();
    let text = value
        .get("result")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let is_error = value
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || value.get("subtype").and_then(Value::as_str) == Some("failure");
    if is_error {
        return ParsedLivenessOutput {
            response: String::new(),
            error: Some(if text.is_empty() {
                "Droid 测活执行失败".to_string()
            } else {
                text.to_string()
            }),
            usage,
        };
    }
    ParsedLivenessOutput {
        response: text.chars().take(240).collect(),
        error: None,
        usage,
    }
}

fn usage_from(value: &Value) -> ParsedTokenUsage {
    let input = extract_u64(value, "input_tokens");
    let cache_read = extract_u64(value, "cache_read_input_tokens");
    let cache_write = extract_u64(value, "cache_creation_input_tokens");
    let output = extract_u64(value, "output_tokens");
    let mut cached = None;
    add_optional(&mut cached, cache_read);
    add_optional(&mut cached, cache_write);
    ParsedTokenUsage {
        input_tokens: input,
        cached_input_tokens: cached,
        output_tokens: output,
        reasoning_output_tokens: extract_u64(value, "thinking_tokens"),
        total_tokens: token_sum(&[input, cached, output]),
        total_cost_usd: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-droid-test",
            base_url: "https://relay.example.com/v1",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 60,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_uses_the_generic_chat_completion_provider() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            crate::test_support::slash_args(&plan.args),
            [
                "exec",
                "-m",
                "custom:balancehub/relay-model",
                "--settings",
                "/tmp/<liveness>/droid/settings.json",
                "-o",
                "json",
                "Say OK"
            ]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-droid-test")));
        let file = plan.files.first().unwrap();
        let value: serde_json::Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(
            value["customModels"][0]["provider"],
            "generic-chat-completion-api"
        );
        assert_eq!(
            value["customModels"][0]["baseUrl"],
            "https://relay.example.com/v1"
        );
    }

    #[test]
    fn parse_output_reads_the_success_result_and_usage() {
        let stdout = concat!(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"BH_OK","#,
            r#""session_id":"5d3b","usage":{"input_tokens":7,"output_tokens":2,"#,
            r#""cache_read_input_tokens":3,"cache_creation_input_tokens":1,"thinking_tokens":4}}"#
        );
        let parsed = parse_output(stdout, "");
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(4));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.reasoning_output_tokens, Some(4));
        assert_eq!(parsed.usage.total_tokens, Some(13));
    }

    #[test]
    fn parse_output_surfaces_a_failure_result() {
        let stdout = concat!(
            r#"{"type":"result","subtype":"failure","is_error":true,"result":"Connection error.","#,
            r#""usage":{"input_tokens":0,"output_tokens":0}}"#
        );
        let parsed = parse_output(stdout, "");
        assert!(parsed.response.is_empty());
        assert_eq!(parsed.error.as_deref(), Some("Connection error."));
    }
}
