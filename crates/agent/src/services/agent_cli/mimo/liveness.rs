use super::super::{
    contracts::{
        EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput,
    },
    opencode_family::parse_run_events,
};
use serde_json::json;

const PROVIDER_ID: &str = "balancehub";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let model = request.model.trim();
    let config = serde_json::to_string(&json!({
        "provider": {
            PROVIDER_ID: {
                "npm": "@ai-sdk/openai-compatible",
                "name": "BalanceHub",
                "options": {
                    "baseURL": super::super::config_support::openai_base_url(
                        request.base_url.trim(),
                    ),
                    "apiKey": request.api_key.trim(),
                },
                "models": { model: { "name": model } },
            },
        },
        "model": format!("{PROVIDER_ID}/{model}"),
    }))
    .map_err(|err| format!("生成 MiMo Code 测活配置失败: {err}"))?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "MIMOCODE_CONFIG",
        "MIMOCODE_CONFIG_CONTENT",
        "MIMOCODE_CONFIG_DIR",
        "MIMOCODE_HOME",
        "MIMOCODE_DB",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set("MIMOCODE_CONFIG_CONTENT", config);
    environment.set("MIMOCODE_DISABLE_AUTOUPDATE", "1");
    Ok(LivenessPlan {
        args: vec![
            "run".to_string(),
            "--pure".to_string(),
            "--format".to_string(),
            "json".to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: Vec::new(),
        response_source: LivenessResponseSource::Stdout,
    })
}

pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    parse_run_events("MiMo Code", response_output, stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-mimo-test",
            base_url: "https://relay.example.com/v1",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 60,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_uses_the_inline_config_without_exposing_the_key() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            ["run", "--pure", "--format", "json", "Say OK"]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-mimo-test")));
        let config = plan
            .environment
            .set_values()
            .find(|(name, _)| *name == "MIMOCODE_CONFIG_CONTENT")
            .map(|(_, value)| value)
            .unwrap();
        assert!(config.contains("sk-mimo-test"));
        let parsed: serde_json::Value = serde_json::from_str(config).unwrap();
        assert_eq!(parsed["model"], "balancehub/relay-model");
        assert_eq!(
            parsed["provider"][PROVIDER_ID]["options"]["baseURL"],
            "https://relay.example.com/v1"
        );
        assert!(plan.files.is_empty());
    }

    #[test]
    fn parse_output_reads_text_and_step_usage() {
        let parsed = parse_output(
            concat!(
                r#"{"type":"text","part":{"type":"text","text":"BH_OK"}}"#,
                "\n",
                r#"{"type":"step_finish","part":{"type":"step-finish","tokens":{"total":9,"input":7,"output":2,"reasoning":0,"cache":{"write":0,"read":3}},"cost":0.02}}"#,
                "\n"
            ),
            "",
        );
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(3));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(9));
        assert_eq!(parsed.usage.total_cost_usd, Some(0.02));
    }

    #[test]
    fn parse_output_surfaces_an_error_event() {
        let parsed = parse_output(
            r#"{"type":"error","error":{"data":{"message":"provider unavailable"}}}"#,
            "",
        );
        assert!(parsed.response.is_empty());
        assert_eq!(parsed.error.as_deref(), Some("provider unavailable"));
    }
}
