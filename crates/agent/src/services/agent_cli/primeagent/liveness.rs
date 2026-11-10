use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput,
    },
    pi_family::parse_message_end_events,
};
use serde_json::json;

const PROVIDER_ID: &str = "balancehub";
const API_KEY_ENV: &str = "BALANCEHUB_PRIME_API_KEY";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let agent_dir = request.isolated_home.join("agent");
    let models = serde_json::to_string_pretty(&json!({
        "providers": {
            PROVIDER_ID: {
                "name": "BalanceHub",
                "baseUrl": super::super::config_support::openai_base_url(
                    request.base_url.trim(),
                ),
                "api": "openai-completions",
                "apiKey": format!("${{{API_KEY_ENV}}}"),
                "models": [{ "id": request.model.trim(), "name": request.model.trim() }],
            },
        },
    }))
    .map_err(|err| format!("生成 Prime Agent 测活模型配置失败: {err}"))?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "PRIME_AGENT_CODING_AGENT_DIR",
        "PRIME_AGENT_SESSION_DIR",
        "PRIME_AGENT_CODING_AGENT_SESSION_DIR",
        "PRIME_AGENT_KERNEL_PYTHON",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set_path("PRIME_AGENT_CODING_AGENT_DIR", &agent_dir);
    environment.set(API_KEY_ENV, request.api_key.trim());
    Ok(LivenessPlan {
        args: vec![
            "--offline".to_string(),
            "--provider".to_string(),
            PROVIDER_ID.to_string(),
            "--model".to_string(),
            request.model.trim().to_string(),
            "--print".to_string(),
            "--mode".to_string(),
            "json".to_string(),
            "--no-session".to_string(),
            "--no-skills".to_string(),
            "--no-prompt-templates".to_string(),
            "--no-themes".to_string(),
            "--no-context-files".to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: vec![AgentFilePlan {
            path: agent_dir.join("models.json"),
            content: models,
        }],
        response_source: LivenessResponseSource::Stdout,
    })
}

pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    parse_message_end_events("Prime Agent", response_output, stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-prime-test",
            base_url: "https://relay.example.com/v1",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 60,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_isolates_the_agent_directory_and_references_the_key_by_env() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            [
                "--offline",
                "--provider",
                "balancehub",
                "--model",
                "relay-model",
                "--print",
                "--mode",
                "json",
                "--no-session",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "--no-context-files",
                "Say OK",
            ]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-prime-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "BALANCEHUB_PRIME_API_KEY" && value == "sk-prime-test"));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "PRIME_AGENT_CODING_AGENT_DIR"
                && crate::test_support::same_path(value, "/tmp/<liveness>/agent")));
        let models = &plan.files[0];
        assert_eq!(models.path, request().isolated_home.join("agent/models.json"));
        assert!(!models.content.contains("sk-prime-test"));
        let parsed: serde_json::Value = serde_json::from_str(&models.content).unwrap();
        assert_eq!(
            parsed["providers"][PROVIDER_ID]["baseUrl"],
            "https://relay.example.com/v1"
        );
        assert_eq!(
            parsed["providers"][PROVIDER_ID]["apiKey"],
            "${BALANCEHUB_PRIME_API_KEY}"
        );
    }

    #[test]
    fn parse_output_reads_the_committed_message() {
        let parsed = parse_output(
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"BH_OK"}],"usage":{"input":7,"output":2,"totalTokens":9},"stopReason":"stop"}}"#,
            "",
        );
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(9));
    }

    #[test]
    fn parse_output_surfaces_a_failed_stop_reason() {
        let parsed = parse_output(
            r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"Connection error."}}"#,
            "",
        );
        assert!(parsed.response.is_empty());
        assert_eq!(parsed.error.as_deref(), Some("Connection error."));
    }
}
