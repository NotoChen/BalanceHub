use super::super::contracts::{
    AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
    ParsedLivenessOutput,
};
use serde_json::{Value, json};

const PROVIDER_ID: &str = "balancehub";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let root = request.isolated_home.join("copilot");
    let model = request.model.trim();
    let settings = serde_json::to_string_pretty(&json!({
        "model": format!("{PROVIDER_ID}/{model}"),
    }))
    .map_err(|err| format!("生成 Copilot CLI 测活设置失败: {err}"))?;
    let providers = serde_json::to_string_pretty(&json!({
        "providers": [{
            "name": PROVIDER_ID,
            "type": "openai",
            "baseUrl": super::super::config_support::openai_base_url(
                request.base_url.trim(),
            ),
            "apiKey": request.api_key.trim(),
        }],
        "models": [{
            "id": model,
            "provider": PROVIDER_ID,
            "name": model,
        }],
    }))
    .map_err(|err| format!("生成 Copilot CLI 测活 Provider 失败: {err}"))?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "COPILOT_HOME",
        "COPILOT_PROVIDERS_CONFIG",
        "COPILOT_ALLOW_ALL",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set_path("COPILOT_HOME", &root);
    Ok(LivenessPlan {
        args: vec![
            "-p".to_string(),
            request.prompt.to_string(),
            "--output-format".to_string(),
            "json".to_string(),
            "--disable-client-telemetry".to_string(),
            "--allow-all-tools".to_string(),
            "--disable-builtin-mcps".to_string(),
        ],
        environment,
        files: vec![
            AgentFilePlan {
                path: super::config::settings_path(&root),
                content: settings,
            },
            AgentFilePlan {
                path: root.join("providers.json"),
                content: providers,
            },
        ],
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `--output-format json` emits one event per line. The final
/// `assistant.message` carries the answer; `session.error` carries failures.
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
        match event.get("type").and_then(Value::as_str) {
            Some("assistant.message") => {
                if let Some(content) = event
                    .pointer("/data/content")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|content| !content.is_empty())
                {
                    parsed.response = content.chars().take(240).collect();
                }
            }
            Some("session.error") => {
                parsed.error = Some(
                    event
                        .pointer("/data/message")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|message| !message.is_empty())
                        .unwrap_or("Copilot CLI 测活执行失败")
                        .to_string(),
                );
                parsed.response.clear();
            }
            _ => {}
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
            api_key: "sk-copilot-test",
            base_url: "https://relay.example.com/v1",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 60,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_isolates_the_copilot_home_and_keeps_the_key_out_of_argv() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            [
                "-p",
                "Say OK",
                "--output-format",
                "json",
                "--disable-client-telemetry",
                "--allow-all-tools",
                "--disable-builtin-mcps"
            ]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-copilot-test")));
        assert!(plan.environment.set_values().any(|(name, value)| {
            name == "COPILOT_HOME" && crate::test_support::ends_with_path(value, "/copilot")
        }));
        let providers = plan
            .files
            .iter()
            .find(|file| file.path.ends_with("providers.json"))
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&providers.content).unwrap();
        assert_eq!(
            value["providers"][0]["baseUrl"],
            "https://relay.example.com/v1"
        );
        assert_eq!(value["models"][0]["provider"], PROVIDER_ID);
    }

    #[test]
    fn parse_output_reads_the_final_assistant_message() {
        let stdout = concat!(
            r#"{"type":"assistant.message_delta","data":{"deltaContent":"BH_"}}"#,
            "\n",
            r#"{"type":"assistant.message","data":{"content":"BH_OK","model":"relay-model"}}"#,
            "\n",
            r#"{"type":"result","exitCode":0}"#,
            "\n"
        );
        let parsed = parse_output(stdout, "");
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
    }

    #[test]
    fn parse_output_surfaces_a_session_error() {
        let stdout = concat!(
            r#"{"type":"session.error","data":{"errorType":"query","message":"Could not connect to local model provider"}}"#,
            "\n",
            r#"{"type":"result","exitCode":1}"#,
            "\n"
        );
        let parsed = parse_output(stdout, "");
        assert!(parsed.response.is_empty());
        assert_eq!(
            parsed.error.as_deref(),
            Some("Could not connect to local model provider")
        );
    }
}
