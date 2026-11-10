use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput,
    },
    pi_family,
};

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let model = request.model.trim();
    let agent_dir = request.isolated_home.join(".omp/agent");
    let mut environment = EnvironmentPatch::default();
    for name in [
        "PI_CONFIG_DIR",
        "PI_CODING_AGENT_DIR",
        "PI_CODING_AGENT_SESSION_DIR",
        "OMP_CODING_AGENT_SESSION_DIR",
        "PI_PROFILE",
        "OMP_PROFILE",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set("NO_COLOR", "1");
    Ok(LivenessPlan {
        args: vec![
            "-p".to_string(),
            "--mode".to_string(),
            "json".to_string(),
            "--model".to_string(),
            format!("{}/{}", super::config::PROVIDER_ID, model),
            "--no-tools".to_string(),
            "--no-session".to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: vec![
            AgentFilePlan {
                path: agent_dir.join("config.yml"),
                content: super::config::temporary_config(model)?,
            },
            AgentFilePlan {
                path: super::config::models_path(&agent_dir),
                content: super::config::temporary_models(
                    model,
                    &super::super::config_support::openai_base_url(request.base_url.trim()),
                    request.api_key.trim(),
                )?,
            },
        ],
        response_source: LivenessResponseSource::Stdout,
    })
}

pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    pi_family::parse_message_end_events("omp", response_output, stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-omp-test",
            base_url: "https://relay.example.com",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 30,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_writes_isolated_model_files_and_keeps_the_key_out_of_argv() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            [
                "-p",
                "--mode",
                "json",
                "--model",
                "balancehub/relay-model",
                "--no-tools",
                "--no-session",
                "Say OK"
            ]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-omp-test")));
        assert!(plan
            .environment
            .removed_names()
            .any(|name| name == "PI_CODING_AGENT_DIR"));
        // omp 自己的会话目录覆盖同样会绕过隔离的 HOME。
        assert!(plan
            .environment
            .removed_names()
            .any(|name| name == "OMP_CODING_AGENT_SESSION_DIR"));
        let config = plan
            .files
            .iter()
            .find(|file| file.path.ends_with("config.yml"))
            .unwrap();
        assert_eq!(
            config.path,
            Path::new("/tmp/<liveness>/.omp/agent/config.yml")
        );
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&config.content).unwrap();
        assert_eq!(
            value["modelRoles"]["default"].as_str(),
            Some("balancehub/relay-model")
        );
        let models = plan
            .files
            .iter()
            .find(|file| file.path.ends_with("models.yml"))
            .unwrap();
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&models.content).unwrap();
        assert_eq!(
            value["providers"]["balancehub"]["baseUrl"].as_str(),
            Some("https://relay.example.com/v1")
        );
        assert_eq!(
            value["providers"]["balancehub"]["apiKey"].as_str(),
            Some("sk-omp-test")
        );
    }

    #[test]
    fn parse_output_reads_the_pi_event_stream_and_usage() {
        let stdout = concat!(
            r#"{"type":"turn_start"}"#,
            "\n",
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"BH_OK"}],"#,
            r#""usage":{"input":7,"output":2,"cacheRead":3,"cacheWrite":1,"totalTokens":13,"#,
            r#""cost":{"total":0.01}},"stopReason":"stop"}}"#,
            "\n",
            r#"{"type":"agent_end","messages":[]}"#
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
