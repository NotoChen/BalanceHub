use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{extract_f64, extract_u64, token_sum},
};
use serde_json::Value;

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let root = request.isolated_home.join("openclaw");
    let config_path = root.join("openclaw.json");
    let state_dir = root.join("state");
    let config = super::config::temporary_provider_config(
        request.model.trim(),
        &super::super::config_support::openai_base_url(request.base_url.trim()),
        request.api_key.trim(),
    )?;
    let mut environment = EnvironmentPatch::default();
    for name in [
        "OPENCLAW_HOME",
        "OPENCLAW_PROFILE",
        "OPENCLAW_STATE_DIR",
        "OPENCLAW_CONFIG_PATH",
        "OPENCLAW_CONFIG_DIR",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set_path("OPENCLAW_STATE_DIR", &state_dir);
    environment.set_path("OPENCLAW_CONFIG_PATH", &config_path);
    Ok(LivenessPlan {
        args: vec![
            "agent".to_string(),
            "exec".to_string(),
            "--config".to_string(),
            config_path.to_string_lossy().into_owned(),
            "--json".to_string(),
            "--timeout".to_string(),
            request.timeout_seconds.to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: vec![AgentFilePlan {
            path: config_path,
            content: config,
        }],
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `agent exec --json` prints one stable envelope: `ok` / `status`, the final
/// reply, token usage and an `error` object on failure.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let Ok(value) = serde_json::from_str::<Value>(source.trim()) else {
        return ParsedLivenessOutput::default();
    };
    let usage = value
        .get("usage")
        .map(usage_from)
        .unwrap_or_default();
    let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false)
        && value
            .get("status")
            .and_then(Value::as_str)
            .is_none_or(|status| status == "ok");
    let final_text = value
        .get("final")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("payloads")
                .and_then(Value::as_array)
                .and_then(|payloads| payloads.first())
                .and_then(|payload| payload.get("text"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        });
    if !ok {
        let error = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|message| !message.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                value
                    .get("status")
                    .and_then(Value::as_str)
                    .filter(|status| *status != "ok")
                    .map(|status| format!("OpenClaw 测活未完成: {status}"))
            })
            .unwrap_or_else(|| "OpenClaw 测活执行失败".to_string());
        return ParsedLivenessOutput {
            response: String::new(),
            error: Some(error),
            usage,
        };
    }
    ParsedLivenessOutput {
        response: final_text
            .map(|value| value.chars().take(240).collect())
            .unwrap_or_default(),
        error: None,
        usage,
    }
}

fn usage_from(value: &Value) -> ParsedTokenUsage {
    let input = extract_u64(value, "input");
    let cached = extract_u64(value, "cacheRead");
    let cache_write = extract_u64(value, "cacheWrite");
    let output = extract_u64(value, "output");
    let total = extract_u64(value, "total")
        .or_else(|| token_sum(&[input, cached, cache_write, output]));
    ParsedTokenUsage {
        input_tokens: input,
        cached_input_tokens: cached,
        output_tokens: output,
        reasoning_output_tokens: None,
        total_tokens: total,
        total_cost_usd: value
            .pointer("/cost/total")
            .and_then(Value::as_f64)
            .or_else(|| extract_f64(value, "cost"))
            .filter(|cost| *cost > 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-openclaw-test",
            base_url: "https://relay.example.com/v1",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 60,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_pins_an_isolated_config_and_keeps_the_key_out_of_argv() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            [
                "agent",
                "exec",
                "--config",
                "/tmp/<liveness>/openclaw/openclaw.json",
                "--json",
                "--timeout",
                "60",
                "Say OK"
            ]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-openclaw-test")));
        assert!(plan.environment.set_values().any(|(name, value)| {
            name == "OPENCLAW_STATE_DIR" && value.ends_with("/openclaw/state")
        }));
        let file = plan.files.first().unwrap();
        assert_eq!(file.path, Path::new("/tmp/<liveness>/openclaw/openclaw.json"));
        let value: serde_json::Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(
            value["models"]["providers"]["balancehub"]["baseUrl"],
            "https://relay.example.com/v1"
        );
        assert_eq!(
            value["agents"]["defaults"]["model"]
                .as_str()
                .or_else(|| value["agents"]["defaults"]["model"]["primary"].as_str()),
            Some("balancehub/relay-model")
        );
    }

    #[test]
    fn parse_output_reads_the_envelope_and_usage() {
        let stdout = concat!(
            r#"{"ok":true,"status":"ok","final":"BH_OK","payloads":[{"text":"BH_OK"}],"#,
            r#""usage":{"input":7,"output":2,"cacheRead":3,"cacheWrite":1,"total":9,"#,
            r#""cost":{"total":0.01}},"model":"relay-model","provider":"balancehub"}"#
        );
        let parsed = parse_output(stdout, "");
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(3));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(9));
        assert_eq!(parsed.usage.total_cost_usd, Some(0.01));
    }

    #[test]
    fn parse_output_surfaces_the_error_envelope() {
        let stdout = concat!(
            r#"{"ok":false,"status":"error","final":"","payloads":[],"model":null,"#,
            r#""provider":null,"error":{"message":"Couldn't connect to the AI service","#,
            r#""kind":"exception"}}"#
        );
        let parsed = parse_output(stdout, "");
        assert!(parsed.response.is_empty());
        assert_eq!(
            parsed.error.as_deref(),
            Some("Couldn't connect to the AI service")
        );
    }
}
