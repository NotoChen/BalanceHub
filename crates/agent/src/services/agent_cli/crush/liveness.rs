use super::super::contracts::{
    AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
    ParsedLivenessOutput,
};
use serde_json::json;

const PROVIDER_ID: &str = "balancehub";
const API_KEY_ENV: &str = "BALANCEHUB_CRUSH_API_KEY";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let config_dir = request.isolated_home.join(".config/crush");
    let model = request.model.trim();
    let config = serde_json::to_string_pretty(&json!({
        "$schema": "https://charm.land/crush.json",
        "providers": {
            PROVIDER_ID: {
                // `completions` is the current name for an OpenAI-compatible
                // chat API; the legacy `openai` type now means the Responses API.
                "type": "completions",
                "name": "BalanceHub",
                "base_url": request.base_url.trim(),
                "api_key": format!("${{{API_KEY_ENV}}}"),
                "models": [{ "id": model, "name": model }],
            }
        },
        "models": {
            "large": { "provider": PROVIDER_ID, "model": model },
            "small": { "provider": PROVIDER_ID, "model": model },
        },
    }))
    .map_err(|err| format!("生成 Crush 测活配置失败: {err}"))?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "CRUSH_GLOBAL_DATA",
        "CRUSH_CONFIG",
        "CRUSH_DATA_DIR",
        "CRUSH_SESSION_DATA_DIR",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set(API_KEY_ENV, request.api_key.trim());
    Ok(LivenessPlan {
        args: vec![
            "run".to_string(),
            "--quiet".to_string(),
            "--model".to_string(),
            format!("{PROVIDER_ID}/{model}"),
            request.prompt.to_string(),
        ],
        environment,
        files: vec![AgentFilePlan {
            path: config_dir.join("crush.json"),
            content: config,
        }],
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `crush run --quiet` prints only the answer on stdout; it has no machine
/// readable event stream, so the probe reports the terminal text only.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    ParsedLivenessOutput {
        response: source.trim().chars().take(240).collect(),
        ..ParsedLivenessOutput::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-crush-test",
            base_url: "https://relay.example.com/v1",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 30,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_uses_completions_type_and_references_the_key_by_env() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            [
                "run",
                "--quiet",
                "--model",
                "balancehub/relay-model",
                "Say OK"
            ]
        );
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == API_KEY_ENV && value == "sk-crush-test"));
        assert!(plan
            .environment
            .removed_names()
            .any(|name| name == "CRUSH_GLOBAL_DATA"));
        let file = plan.files.first().unwrap();
        assert_eq!(file.path, Path::new("/tmp/<liveness>/.config/crush/crush.json"));
        let value: serde_json::Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(value["providers"][PROVIDER_ID]["type"], "completions");
        assert_eq!(
            value["providers"][PROVIDER_ID]["api_key"],
            "${BALANCEHUB_CRUSH_API_KEY}"
        );
        assert_eq!(value["models"]["large"]["model"], "relay-model");
        assert_eq!(value["models"]["small"]["model"], "relay-model");
    }

    #[test]
    fn parse_output_returns_the_terminal_answer() {
        let parsed = parse_output("BH_OK\n", "");
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
    }
}
