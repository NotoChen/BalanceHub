use super::super::contracts::{
    EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource, ParsedLivenessOutput,
    ParsedTokenUsage,
};
use serde_json::Value;

/// The engine's fixed recovery text when the model did not produce the
/// `{"type":"final_answer",...}` envelope; it proves the transport worked but
/// not that the requested answer came back.
const FORMAT_FALLBACK: &str =
    "the final answer format was iiiinvalid. cooommonn, you can do itttt.";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "MISTER_MORPH_CONFIG",
        "MISTER_MORPH_API_KEY",
        "MISTER_MORPH_LLM_API_KEY",
        "MISTER_MORPH_LLM_ENDPOINT",
        "MISTER_MORPH_LLM_MODEL",
        "MISTER_MORPH_LLM_PROVIDER",
        "MISTER_MORPH_LLM_INFERENCE_PROVIDER",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set("MISTER_MORPH_LLM_API_KEY", request.api_key.trim());
    Ok(LivenessPlan {
        args: vec![
            "run".to_string(),
            "--provider".to_string(),
            "openai".to_string(),
            "--endpoint".to_string(),
            super::super::config_support::openai_base_url(request.base_url.trim()),
            "--model".to_string(),
            request.model.trim().to_string(),
            "--task".to_string(),
            request.prompt.to_string(),
            "--no-workspace".to_string(),
            "--log-level".to_string(),
            "error".to_string(),
        ],
        environment,
        files: Vec::new(),
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `run` prints the final answer object; the engine only fills `reasoning`
/// when it accepted a structured answer from the model.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let Ok(value) = serde_json::from_str::<Value>(source.trim()) else {
        return ParsedLivenessOutput::default();
    };
    let output = value.get("output").map(|output| match output {
        Value::String(text) => text.trim().to_string(),
        other => other.to_string(),
    });
    let mut parsed = ParsedLivenessOutput {
        response: output
            .as_deref()
            .unwrap_or_default()
            .chars()
            .take(240)
            .collect(),
        ..ParsedLivenessOutput::default()
    };
    if output.is_none() || parsed.response.is_empty() {
        parsed.response.clear();
        parsed.error = Some("Mister Morph 未返回最终回答".to_string());
    } else if parsed.response == FORMAT_FALLBACK {
        parsed.response.clear();
        parsed.error = Some("Mister Morph 未返回有效的最终回答格式".to_string());
    }
    // The printout has no token accounting; usage stays empty.
    parsed.usage = ParsedTokenUsage::default();
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-morph-test",
            base_url: "https://relay.example.com/v1",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 60,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_keeps_the_key_in_the_environment() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            [
                "run",
                "--provider",
                "openai",
                "--endpoint",
                "https://relay.example.com/v1",
                "--model",
                "relay-model",
                "--task",
                "Say OK",
                "--no-workspace",
                "--log-level",
                "error",
            ]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-morph-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "MISTER_MORPH_LLM_API_KEY"
                && value == "sk-morph-test"));
        assert!(plan
            .environment
            .removed_names()
            .any(|name| name == "MISTER_MORPH_LLM_ENDPOINT"));
        assert!(plan.files.is_empty());
    }

    #[test]
    fn parse_output_reads_the_final_answer() {
        let parsed = parse_output(
            "{\n  \"reasoning\": \"done\",\n  \"output\": \"BH_OK\"\n}",
            "",
        );
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
    }

    #[test]
    fn parse_output_rejects_the_format_fallback() {
        let parsed = parse_output(&format!("{{\"output\":\"{FORMAT_FALLBACK}\"}}"), "");
        assert!(parsed.response.is_empty());
        assert_eq!(
            parsed.error.as_deref(),
            Some("Mister Morph 未返回有效的最终回答格式")
        );
    }

    #[test]
    fn parse_output_rejects_missing_output() {
        let parsed = parse_output("{\"reasoning\":\"done\"}", "");
        assert!(parsed.response.is_empty());
        assert_eq!(parsed.error.as_deref(), Some("Mister Morph 未返回最终回答"));
    }
}
