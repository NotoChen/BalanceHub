use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{extract_u64, token_sum},
};
use super::config;
use serde_json::Value;

const PROVIDER_ID: &str = "balancehub";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let model = request.model.trim();
    let documents = config::provider_documents(
        model,
        "BalanceHub",
        request.base_url.trim(),
        request.api_key.trim(),
        None,
        None,
        None,
    )?;
    let home = request.isolated_home.join("commandcode-home");
    let root = home.join(".commandcode");
    let mut environment = EnvironmentPatch::default();
    for name in [
        "HOME",
        "USERPROFILE",
        "COMMAND_CODE_API_KEY",
        "CMD_LOCAL_ONLY",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    environment.set_path("HOME", &home);
    environment.set_path("USERPROFILE", &home);
    Ok(LivenessPlan {
        args: vec![
            "-p".to_string(),
            request.prompt.to_string(),
            "-m".to_string(),
            format!("{PROVIDER_ID}/{model}"),
            "--output-format".to_string(),
            "json".to_string(),
            "--no-session".to_string(),
            "--no-auto-update".to_string(),
            "--skip-onboarding".to_string(),
            "--local-only".to_string(),
        ],
        environment,
        files: vec![
            AgentFilePlan {
                path: root.join("providers.json"),
                content: documents.providers,
            },
            AgentFilePlan {
                path: root.join("credentials.json"),
                content: documents.credentials,
            },
            AgentFilePlan {
                path: root.join("config.json"),
                content: documents.user_config,
            },
        ],
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `--output-format json` emits NDJSON AgentEvent frames followed by one
/// `result` line carrying `finalText` and cumulative token usage.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let mut parsed = ParsedLivenessOutput::default();
    for line in source.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("result") {
            continue;
        }
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|message| !message.is_empty());
        if value.get("subtype").and_then(Value::as_str) == Some("error") || error.is_some() {
            parsed.response.clear();
            parsed.error = Some(error.unwrap_or("Command Code 测活执行失败").to_string());
            parsed.usage = value.get("usage").map(usage_from).unwrap_or_default();
            return parsed;
        }
        if let Some(text) = value
            .get("finalText")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            parsed.response = text.chars().take(240).collect();
        }
        parsed.error = None;
        parsed.usage = value.get("usage").map(usage_from).unwrap_or_default();
    }
    parsed
}

fn usage_from(value: &Value) -> ParsedTokenUsage {
    let input = extract_u64(value, "inputTokens");
    let cache_read = extract_u64(value, "cachedReadTokens");
    let cache_write = extract_u64(value, "cachedWriteTokens");
    let output = extract_u64(value, "outputTokens");
    ParsedTokenUsage {
        input_tokens: input,
        cached_input_tokens: cache_read,
        output_tokens: output,
        reasoning_output_tokens: None,
        total_tokens: extract_u64(value, "totalTokens")
            .or_else(|| token_sum(&[input, cache_read, cache_write, output])),
        total_cost_usd: None,
    }
}
