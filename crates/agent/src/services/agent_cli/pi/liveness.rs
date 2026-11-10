use super::launch::temporary_models;
use super::super::contracts::{
    AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
    ParsedLivenessOutput,
};

const PROVIDER_ID: &str = "balancehub";
const API_KEY_ENV: &str = "BALANCEHUB_PI_API_KEY";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let agent_dir = request.isolated_home.join("agent");
    let mut environment = EnvironmentPatch::default();
    for name in [
        "PI_CODING_AGENT_DIR",
        "PI_PACKAGE_DIR",
        "PI_OFFLINE",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set_path("PI_CODING_AGENT_DIR", &agent_dir);
    environment.set(API_KEY_ENV, request.api_key.trim());
    // Pi's startup catalog refresh reaches the network before a one-shot run;
    // offline mode keeps the probe bounded to the provider request.
    environment.set("PI_OFFLINE", "1");
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
            "--no-tools".to_string(),
            "--no-session".to_string(),
            "--no-extensions".to_string(),
            "--no-skills".to_string(),
            "--no-prompt-templates".to_string(),
            "--no-themes".to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: vec![AgentFilePlan {
            path: agent_dir.join("models.json"),
            content: temporary_models(request.model.trim(), request.base_url.trim())?,
        }],
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `--mode json` emits one event per line. Committed assistant `message_end`
/// events carry the answer; the last one carries usage and the stop reason.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    super::super::pi_family::parse_message_end_events("Pi", response_output, stdout)
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
    fn plan_isolates_the_agent_directory_and_references_the_key_by_env() {
        let plan = build_plan(request("sk-pi-test", "https://relay.example.com/v1")).unwrap();
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
                "--no-tools",
                "--no-session",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "Say OK",
            ]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-pi-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "BALANCEHUB_PI_API_KEY" && value == "sk-pi-test"));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "PI_CODING_AGENT_DIR"
                && crate::test_support::same_path(value, "/tmp/<liveness>/agent")));
        assert!(!plan.files[0].content.contains("sk-pi-test"));
    }

    #[test]
    fn committed_assistant_message_supplies_the_answer_and_usage() {
        let parsed = parse_output(
            r#"{"type":"message_start","message":{"role":"assistant","content":[]}}
{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"OK"}}
{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"OK"}],"usage":{"input":7,"output":2,"cacheRead":3,"cacheWrite":1,"totalTokens":9,"cost":{"total":0}},"stopReason":"stop"}}"#,
            "",
        );
        assert_eq!(parsed.response, "OK");
        assert_eq!(parsed.error, None);
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(3));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(9));
        assert_eq!(parsed.usage.total_cost_usd, None);
    }

    #[test]
    fn failed_stop_reason_surfaces_the_message() {
        let parsed = parse_output(
            r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"provider unavailable"}}"#,
            "",
        );
        assert_eq!(parsed.response, "");
        assert_eq!(parsed.error.as_deref(), Some("provider unavailable"));
    }
}
