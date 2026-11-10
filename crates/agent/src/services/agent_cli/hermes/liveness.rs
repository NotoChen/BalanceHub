use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{extract_f64, extract_u64, token_sum},
};
use serde_json::Value;

const API_KEY_ENV: &str = "BALANCEHUB_HERMES_API_KEY";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let home = request.isolated_home.join("hermes-home");
    let config = serde_yaml_ng::to_string(&serde_json::json!({
        "model": {
            "provider": "custom",
            "default": request.model.trim(),
            "base_url": request.base_url.trim(),
            "api_key": format!("${{{API_KEY_ENV}}}"),
            "api_mode": "chat_completions",
        },
        "mcp_servers": {},
    }))
    .map_err(|err| format!("生成 Hermes 测活配置失败: {err}"))?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "HERMES_HOME",
        "HERMES_CONFIG",
        "HERMES_CONFIG_PATH",
        "HERMES_INFERENCE_PROVIDER",
        "HERMES_INFERENCE_MODEL",
        "HERMES_CUSTOM_API_KEY",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set_path("HERMES_HOME", &home);
    environment.set(API_KEY_ENV, request.api_key.trim());
    Ok(LivenessPlan {
        args: vec![
            "-z".to_string(),
            request.prompt.to_string(),
            "--usage-file".to_string(),
            request.output_path.to_string_lossy().into_owned(),
        ],
        environment,
        files: vec![AgentFilePlan {
            path: home.join("config.yaml"),
            content: config,
        }],
        response_source: LivenessResponseSource::File(request.output_path.to_path_buf()),
    })
}

/// `-z` prints only the final answer to stdout; the usage report keeps token
/// counts even though the response itself is not JSON.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let response = stdout.trim().chars().take(240).collect::<String>();
    let Ok(value) = serde_json::from_str::<Value>(response_output.trim()) else {
        return ParsedLivenessOutput {
            response,
            ..ParsedLivenessOutput::default()
        };
    };
    let mut usage = ParsedTokenUsage {
        input_tokens: extract_u64(&value, "input_tokens"),
        cached_input_tokens: extract_u64(&value, "cache_read_tokens"),
        output_tokens: extract_u64(&value, "output_tokens"),
        reasoning_output_tokens: extract_u64(&value, "reasoning_tokens"),
        total_tokens: extract_u64(&value, "total_tokens"),
        total_cost_usd: None,
    };
    if usage.total_tokens.is_none() {
        usage.total_tokens = token_sum(&[
            usage.input_tokens,
            usage.output_tokens,
            usage.reasoning_output_tokens,
        ]);
    }
    if value
        .get("cost_status")
        .and_then(Value::as_str)
        .is_some_and(|status| status != "unknown")
    {
        usage.total_cost_usd = extract_f64(&value, "estimated_cost_usd");
    }
    let failed = value
        .get("failed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let error = failed.then(|| {
        value
            .get("turn_exit_reason")
            .and_then(Value::as_str)
            .filter(|reason| !reason.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| "Hermes 测活执行失败".to_string())
    });
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
    fn plan_isolates_the_home_and_references_the_key_by_env() {
        let plan = build_plan(request("sk-hermes-test", "https://relay.example.com/v1")).unwrap();
        assert_eq!(
            plan.args,
            ["-z", "Say OK", "--usage-file", "/tmp/<liveness>/response.txt"]
        );
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "BALANCEHUB_HERMES_API_KEY" && value == "sk-hermes-test"));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, _)| name == "HERMES_HOME"));
        assert!(!plan.files[0].content.contains("sk-hermes-test"));
        let config: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&plan.files[0].content).unwrap();
        let model = config
            .get(serde_yaml_ng::Value::String("model".into()))
            .unwrap();
        assert_eq!(
            model
                .get(serde_yaml_ng::Value::String("api_key".into()))
                .and_then(serde_yaml_ng::Value::as_str),
            Some("${BALANCEHUB_HERMES_API_KEY}")
        );
    }

    #[test]
    fn usage_report_supplies_tokens_and_unknown_cost_stays_unset() {
        let parsed = parse_output(
            r#"{
              "input_tokens": 7,
              "output_tokens": 2,
              "cache_read_tokens": 3,
              "reasoning_tokens": 1,
              "total_tokens": 9,
              "estimated_cost_usd": 0.0,
              "cost_status": "unknown",
              "failed": false,
              "completed": true
            }"#,
            "OK\n",
        );
        assert_eq!(parsed.response, "OK");
        assert_eq!(parsed.error, None);
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(3));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.reasoning_output_tokens, Some(1));
        assert_eq!(parsed.usage.total_tokens, Some(9));
        assert_eq!(parsed.usage.total_cost_usd, None);
    }

    #[test]
    fn failed_usage_report_surfaces_the_turn_reason() {
        let parsed = parse_output(
            r#"{"failed": true, "completed": false, "turn_exit_reason": "provider_error"}"#,
            "",
        );
        assert_eq!(parsed.error.as_deref(), Some("provider_error"));
        assert_eq!(parsed.usage.total_tokens, None);
    }
}
