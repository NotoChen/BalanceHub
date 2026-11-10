use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput,
    },
    pi_family,
};
use super::config;

const API_KEY_ENV: &str = "BALANCEHUB_OMO_API_KEY";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let model = request.model.trim();
    let agent_dir = request.isolated_home.join(".omo/agent");
    let settings = config::temporary_settings(model)?;
    let models = config::temporary_models(
        model,
        request.base_url.trim(),
        &format!("${{{API_KEY_ENV}}}"),
    )?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "OMO_CODING_AGENT_DIR",
        "SENPI_CODING_AGENT_DIR",
        "PI_CODING_AGENT_DIR",
        "OMO_CODING_AGENT_SESSION_DIR",
        "SENPI_CODING_AGENT_SESSION_DIR",
        "PI_CODING_AGENT_SESSION_DIR",
        "PI_PROFILE",
        "OMP_PROFILE",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set(API_KEY_ENV, request.api_key.trim());
    environment.set("NO_COLOR", "1");
    Ok(LivenessPlan {
        args: vec![
            "-p".to_string(),
            "--mode".to_string(),
            "json".to_string(),
            "--offline".to_string(),
            "--model".to_string(),
            format!("{}/{}", config::PROVIDER_ID, model),
            "--no-tools".to_string(),
            "--no-session".to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: vec![
            AgentFilePlan {
                path: agent_dir.join("settings.json"),
                content: settings,
            },
            AgentFilePlan {
                path: agent_dir.join("models.json"),
                content: models,
            },
        ],
        response_source: LivenessResponseSource::Stdout,
    })
}

pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    pi_family::parse_message_end_events("OmO", response_output, stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-omo-test",
            base_url: "https://relay.example.com",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 30,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_writes_isolated_pi_files_and_keeps_the_key_out_of_argv() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            [
                "-p",
                "--mode",
                "json",
                "--offline",
                "--model",
                "balancehub/relay-model",
                "--no-tools",
                "--no-session",
                "Say OK"
            ]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-omo-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == API_KEY_ENV && value == "sk-omo-test"));
        assert!(plan
            .environment
            .removed_names()
            .any(|name| name == "OMO_CODING_AGENT_DIR"));
        let settings = plan
            .files
            .iter()
            .find(|file| file.path.ends_with("settings.json"))
            .unwrap();
        assert_eq!(
            settings.path,
            Path::new("/tmp/<liveness>/.omo/agent/settings.json")
        );
        let value: serde_json::Value = serde_json::from_str(&settings.content).unwrap();
        assert_eq!(value["defaultProvider"], "balancehub");
        assert_eq!(value["defaultModel"], "relay-model");
        let models = plan
            .files
            .iter()
            .find(|file| file.path.ends_with("models.json"))
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&models.content).unwrap();
        assert_eq!(
            value["providers"]["balancehub"]["baseUrl"],
            "https://relay.example.com/v1"
        );
        assert_eq!(
            value["providers"]["balancehub"]["apiKey"],
            format!("${{{API_KEY_ENV}}}")
        );
    }

    #[test]
    fn parse_output_reads_the_pi_event_stream_and_usage() {
        let stdout = concat!(
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"BH_OK"}],"#,
            r#""usage":{"input":7,"output":2,"cacheRead":3,"cacheWrite":1,"totalTokens":13,"#,
            r#""cost":{"total":0.01}},"stopReason":"stop"}}"#,
            "\n",
            r#"{"type":"agent_settled"}"#
        );
        let parsed = parse_output(stdout, "");
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(3));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(13));
        assert_eq!(parsed.usage.total_cost_usd, Some(0.01));
    }
}
