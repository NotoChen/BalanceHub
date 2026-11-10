use super::launch::temporary_config;
use super::super::contracts::{
    EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource, ParsedLivenessOutput,
};

const PROVIDER_ID: &str = "balancehub";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let config = temporary_config(request.model.trim(), request.base_url.trim(), request.api_key.trim())?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "OPENCODE_CONFIG",
        "OPENCODE_CONFIG_CONTENT",
        "OPENCODE_DB",
        "OPENCODE_MODEL",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set("OPENCODE_CONFIG_CONTENT", config);
    Ok(LivenessPlan {
        args: vec![
            "run".to_string(),
            "--pure".to_string(),
            "--model".to_string(),
            format!("{PROVIDER_ID}/{}", request.model.trim()),
            "--format".to_string(),
            "json".to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: Vec::new(),
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `opencode run --format json` prints one event per line; assistant text and
/// token usage arrive on separate events.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    super::super::opencode_family::parse_run_events("OpenCode", response_output, stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
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
    fn plan_keeps_the_key_out_of_argv_and_sets_inline_config() {
        let plan = build_plan(request("sk-opencode-test", "https://relay.example.com/v1")).unwrap();
        assert_eq!(
            plan.args,
            [
                "run",
                "--pure",
                "--model",
                "balancehub/relay-model",
                "--format",
                "json",
                "Say OK",
            ]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-opencode-test")));
        let (_, config) = plan
            .environment
            .set_values()
            .find(|(name, _)| *name == "OPENCODE_CONFIG_CONTENT")
            .expect("inline config");
        assert!(config.contains("sk-opencode-test"));
        let parsed: Value = serde_json::from_str(config).unwrap();
        assert_eq!(parsed["model"], "balancehub/relay-model");
        // OPENCODE_DB 会绕过隔离的 HOME 指向原生会话库。
        assert!(plan
            .environment
            .removed_names()
            .any(|name| name == "OPENCODE_DB"));
    }

    #[test]
    fn text_and_step_finish_events_supply_the_answer_and_usage() {
        let parsed = parse_output(
            r#"{"type":"step_start"}
{"type":"text","part":{"type":"text","text":"OK"}}
{"type":"step_finish","part":{"type":"step-finish","tokens":{"total":9,"input":7,"output":2,"reasoning":1,"cache":{"write":0,"read":3}},"cost":0}}"#,
            "",
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
    fn error_event_surfaces_the_message() {
        let parsed = parse_output(
            r#"{"type":"error","error":{"data":{"message":"provider unavailable"}}}"#,
            "",
        );
        assert_eq!(parsed.response, "");
        assert_eq!(parsed.error.as_deref(), Some("provider unavailable"));
    }
}
