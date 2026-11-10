use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{extract_u64, token_sum},
};
use super::config;
use serde_json::Value;

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let home = request.isolated_home.join("dsh-home");
    let patch_path = home.join("balancehub-liveness.yml");
    let patch = config::temporary_patch(request.model.trim(), request.base_url.trim())?;
    let mut environment = EnvironmentPatch::default();
    environment.remove("DSH_HOME");
    environment.set_path("DSH_HOME", &home);
    environment.set(config::API_KEY_ENV, request.api_key.trim());
    Ok(LivenessPlan {
        args: vec![
            "--profile".to_string(),
            "headless".to_string(),
            "--patch".to_string(),
            patch_path.to_string_lossy().into_owned(),
            "--json".to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: vec![AgentFilePlan {
            path: patch_path,
            content: patch,
        }],
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `dsh --json` emits one event per line. The final answer is the `final`
/// event; token counts are summed from each committed step.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let mut final_text = None;
    let mut text = String::new();
    let mut usage = ParsedTokenUsage::default();
    let mut error = None;
    for line in source.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("final") => {
                final_text = value
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            Some("text") => {
                if let Some(chunk) = value.get("text").and_then(Value::as_str) {
                    text.push_str(chunk);
                }
            }
            Some("error") => {
                error = value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| Some("dsh 测活执行失败".to_string()));
            }
            Some("status") => {
                let phase = value.get("phase").and_then(Value::as_str);
                if phase == Some("step_end") {
                    if let Some(step) = value.get("usage") {
                        add_usage(&mut usage, step);
                    }
                } else if phase == Some("turn_end") {
                    let kind = value
                        .get("reason")
                        .and_then(|reason| reason.get("kind"))
                        .and_then(Value::as_str);
                    if let Some(kind) = kind.filter(|kind| *kind != "completed") {
                        error = Some(format!("dsh 对话未完成: {kind}"));
                    }
                }
            }
            _ => {}
        }
    }
    if usage.total_tokens.is_none() {
        usage.total_tokens = token_sum(&[
            usage.input_tokens,
            usage.cached_input_tokens,
            usage.output_tokens,
            usage.reasoning_output_tokens,
        ]);
    }
    ParsedLivenessOutput {
        response: final_text
            .or_else(|| (!text.is_empty()).then_some(text))
            .map(|value| value.trim().chars().take(240).collect())
            .unwrap_or_default(),
        error,
        usage,
    }
}

fn add_usage(usage: &mut ParsedTokenUsage, value: &Value) {
    add(&mut usage.input_tokens, extract_u64(value, "inputTokens"));
    add(
        &mut usage.cached_input_tokens,
        extract_u64(value, "cacheReadTokens"),
    );
    add(&mut usage.output_tokens, extract_u64(value, "outputTokens"));
    add(
        &mut usage.reasoning_output_tokens,
        extract_u64(value, "reasoningTokens"),
    );
    add(&mut usage.total_tokens, extract_u64(value, "totalTokens"));
}

fn add(target: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        *target = Some(target.unwrap_or(0).saturating_add(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn plan_uses_an_isolated_home_and_references_the_key_by_env() {
        let plan = build_plan(request("sk-dsh-test", "https://relay.example.com/v1")).unwrap();
        assert_eq!(
            crate::test_support::slash_args(&plan.args),
            [
                "--profile",
                "headless",
                "--patch",
                "/tmp/<liveness>/dsh-home/balancehub-liveness.yml",
                "--json",
                "Say OK",
            ]
        );
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "BALANCEHUB_DSH_API_KEY" && value == "sk-dsh-test"));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "DSH_HOME"
                && crate::test_support::same_path(value, "/tmp/<liveness>/dsh-home")));
        assert!(!plan.files[0].content.contains("sk-dsh-test"));
        let patch: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&plan.files[0].content).unwrap();
        let rows = patch.as_sequence().unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn json_stream_supplies_the_final_answer_and_sums_step_usage() {
        let parsed = parse_output(
            r#"{"type":"session","sessionId":"session-1"}
{"type":"status","phase":"step_end","turn":1,"step":1,"usage":{"inputTokens":7,"outputTokens":2,"cacheReadTokens":3,"totalTokens":12}}
{"type":"status","phase":"step_end","turn":1,"step":2,"usage":{"inputTokens":5,"outputTokens":1,"reasoningTokens":2,"totalTokens":6}}
{"type":"status","phase":"turn_end","turn":1,"reason":{"kind":"completed"}}
{"type":"final","text":"OK"}"#,
            "",
        );
        assert_eq!(parsed.response, "OK");
        assert_eq!(parsed.error, None);
        assert_eq!(parsed.usage.input_tokens, Some(12));
        assert_eq!(parsed.usage.cached_input_tokens, Some(3));
        assert_eq!(parsed.usage.output_tokens, Some(3));
        assert_eq!(parsed.usage.reasoning_output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(18));
    }

    #[test]
    fn non_completed_turn_or_error_event_surfaces_the_reason() {
        let aborted = parse_output(
            r#"{"type":"status","phase":"turn_end","turn":1,"reason":{"kind":"aborted"}}"#,
            "",
        );
        assert_eq!(aborted.response, "");
        assert_eq!(aborted.error.as_deref(), Some("dsh 对话未完成: aborted"));

        let errored = parse_output(
            r#"{"type":"error","message":"MISSING_CREDENTIAL"}"#,
            "",
        );
        assert_eq!(errored.error.as_deref(), Some("MISSING_CREDENTIAL"));
    }
}
