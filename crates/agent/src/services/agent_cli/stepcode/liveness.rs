use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput,
    },
    pi_family::parse_message_end_events,
};
use serde_json::json;
use toml_edit::{value as toml_value, Document as TomlDocument};

const PROVIDER_ID: &str = "balancehub";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let model = request.model.trim();
    // StepCode resolves `config.toml` and `models.json` in the parent of the
    // agent directory it is told to use.
    let agent_dir = request.isolated_home.join("agent");
    let mut config = TomlDocument::new();
    config.as_table_mut().insert("defaultProvider", toml_value(PROVIDER_ID));
    config.as_table_mut().insert("defaultModel", toml_value(model));
    let models = serde_json::to_string_pretty(&json!({
        "providers": {
            PROVIDER_ID: {
                "name": "BalanceHub",
                "baseUrl": super::super::config_support::openai_base_url(
                    request.base_url.trim(),
                ),
                "api": "openai-completions",
                "apiKey": request.api_key.trim(),
                "models": [{ "id": model, "name": model }],
            },
        },
    }))
    .map_err(|err| format!("生成 StepCode 测活模型配置失败: {err}"))?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "STEPCODE_CONFIG_DIR",
        "STEPCODE_CONFIG_PATH",
        "STEP_CODING_AGENT_DIR",
        "STEP_CODING_AGENT_SESSION_DIR",
        "STEP_API_KEY",
        "STEPFUN_API_KEY",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set_path("STEP_CODING_AGENT_DIR", &agent_dir);
    // Catalog and update probes reach the network before the one-shot run;
    // disabling the Pi services keeps the probe bounded to the provider call.
    environment.set("STEPCODE_DISABLE_PI_SERVICES", "1");
    Ok(LivenessPlan {
        args: vec![
            "--provider".to_string(),
            PROVIDER_ID.to_string(),
            "--model".to_string(),
            model.to_string(),
            "--print".to_string(),
            "--mode".to_string(),
            "json".to_string(),
            "--no-tools".to_string(),
            "--no-session".to_string(),
            "--no-extensions".to_string(),
            "--no-skills".to_string(),
            "--no-prompt-templates".to_string(),
            "--no-themes".to_string(),
            "--no-context-files".to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: vec![
            AgentFilePlan {
                path: request.isolated_home.join("config.toml"),
                content: config.to_string(),
            },
            AgentFilePlan {
                path: request.isolated_home.join("models.json"),
                content: models,
            },
        ],
        response_source: LivenessResponseSource::Stdout,
    })
}

pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    parse_message_end_events("StepCode", response_output, stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-step-test",
            base_url: "https://relay.example.com/v1",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 60,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_writes_the_family_config_and_keeps_the_key_out_of_argv() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            [
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
                "--no-context-files",
                "Say OK",
            ]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-step-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "STEP_CODING_AGENT_DIR"
                && value == "/tmp/<liveness>/agent"));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "STEPCODE_DISABLE_PI_SERVICES" && value == "1"));
        let config = plan
            .files
            .iter()
            .find(|file| file.path.ends_with("config.toml"))
            .unwrap();
        let parsed = config.content.parse::<toml::Value>().unwrap();
        assert_eq!(parsed["defaultProvider"].as_str(), Some("balancehub"));
        assert_eq!(parsed["defaultModel"].as_str(), Some("relay-model"));
        let models = plan
            .files
            .iter()
            .find(|file| file.path.ends_with("models.json"))
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&models.content).unwrap();
        assert_eq!(
            parsed["providers"][PROVIDER_ID]["baseUrl"],
            "https://relay.example.com/v1"
        );
        assert_eq!(
            parsed["providers"][PROVIDER_ID]["models"][0]["id"],
            "relay-model"
        );
    }

    #[test]
    fn parse_output_reads_the_committed_message() {
        let parsed = parse_output(
            concat!(
                r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"BH_OK"}],"usage":{"input":7,"output":2,"cacheRead":3,"totalTokens":9},"stopReason":"stop"}}"#,
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
            r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"Connection error."}}"#,
            "",
        );
        assert!(parsed.response.is_empty());
        assert_eq!(parsed.error.as_deref(), Some("Connection error."));
    }
}
