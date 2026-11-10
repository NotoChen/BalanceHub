use super::super::contracts::{
    AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
    ParsedLivenessOutput,
};
use serde_json::Value;

const PROVIDER_ID: &str = "balancehub";
const DEFAULT_CONTEXT: u64 = 128_000;

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let root = request.isolated_home.join("kimi");
    let model = request.model.trim();
    let key = format!("{PROVIDER_ID}/{model}");
    let config = format!(
        "default_model = {key:?}\n\n\
         [providers.{PROVIDER_ID}]\n\
         type = \"kimi\"\n\
         base_url = {base_url:?}\n\
         api_key = {api_key:?}\n\n\
         [models.{key:?}]\n\
         provider = {PROVIDER_ID:?}\n\
         model = {model:?}\n\
         max_context_size = {DEFAULT_CONTEXT}\n",
        key = key,
        base_url = super::super::config_support::openai_base_url(request.base_url.trim()),
        api_key = request.api_key.trim(),
        model = model,
    );
    let mut environment = EnvironmentPatch::default();
    for name in [
        "KIMI_CODE_HOME",
        "KIMI_SHARE_DIR",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set_path("KIMI_CODE_HOME", &root);
    Ok(LivenessPlan {
        args: vec![
            "-p".to_string(),
            request.prompt.to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
        ],
        environment,
        files: vec![AgentFilePlan {
            path: super::config::config_path(&root),
            content: config,
        }],
        response_source: LivenessResponseSource::Stdout,
    })
}

/// Prompt mode emits one event per line; the assistant line carries the
/// answer, while retry/failure meta events carry the transport error. A later
/// assistant reply clears an earlier retry error.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let mut parsed = ParsedLivenessOutput::default();
    for line in source.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if event.get("role").and_then(Value::as_str) == Some("assistant") {
            if let Some(content) = event
                .get("content")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|content| !content.is_empty())
            {
                parsed.response = content.chars().take(240).collect();
                parsed.error = None;
            }
            continue;
        }
        if event.get("role").and_then(Value::as_str) != Some("meta") {
            continue;
        }
        if let Some(message) = event
            .get("error_message")
            .or_else(|| event.get("message"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|message| !message.is_empty())
        {
            parsed.error = Some(message.to_string());
        }
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-kimi-test",
            base_url: "https://relay.example.com/v1",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 60,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_writes_an_isolated_config_and_keeps_the_key_out_of_argv() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            ["-p", "Say OK", "--output-format", "stream-json"]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-kimi-test")));
        assert!(plan.environment.set_values().any(|(name, value)| {
            name == "KIMI_CODE_HOME" && value.ends_with("/kimi")
        }));
        let file = plan.files.first().unwrap();
        assert_eq!(file.path, Path::new("/tmp/<liveness>/kimi/config.toml"));
        let value: toml::Value = toml::from_str(&file.content).unwrap();
        assert_eq!(value["default_model"].as_str(), Some("balancehub/relay-model"));
        assert_eq!(
            value["providers"][PROVIDER_ID]["base_url"].as_str(),
            Some("https://relay.example.com/v1")
        );
        assert_eq!(
            value["models"]["balancehub/relay-model"]["model"].as_str(),
            Some("relay-model")
        );
    }

    #[test]
    fn parse_output_reads_the_assistant_answer() {
        let stdout = concat!(
            r#"{"role":"meta","type":"system.version","version":"2.5.2"}"#,
            "\n",
            r#"{"role":"assistant","content":"BH_OK"}"#,
            "\n"
        );
        let parsed = parse_output(stdout, "");
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
    }

    #[test]
    fn parse_output_surfaces_a_transport_error_and_a_success_clears_it() {
        let retry = r#"{"role":"meta","type":"turn.step.retrying","error_name":"APIConnectionError","error_message":"Connection error."}"#;
        let failed = parse_output(retry, "");
        assert!(failed.response.is_empty());
        assert_eq!(failed.error.as_deref(), Some("Connection error."));

        let recovered = format!("{retry}\n{}", r#"{"role":"assistant","content":"BH_OK"}"#);
        let parsed = parse_output(&recovered, "");
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
    }
}
