use super::super::{
    config_support::yaml_source,
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{add_optional, extract_u64, token_sum},
};
use serde_json::Value;

const PROVIDER: &str = "custom_provider:balancehub";
const ENTRY: &str = "balancehub";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let model = request.model.trim();
    // MiniMax Code splits the model reference at the first slash and looks the
    // model up under a key with each slash spelled `~` (its flat id).
    let flat = model.replace('/', "~");
    let config = yaml_source::edit("", "MiniMax Code 测活配置", |root| {
        root.set("defaultModel", format!("{PROVIDER}/{flat}"));
        let custom = yaml_source::mapping_member(root, "custom_provider", "MiniMax Code 测活配置")?;
        let entry = yaml_source::mapping_member(&custom, ENTRY, "MiniMax Code 测活配置")?;
        entry.set("name", "BalanceHub");
        entry.set("kind", "custom");
        entry.set("enabled", true);
        entry.set("api", "openai-completions");
        let options = yaml_source::mapping_member(&entry, "options", "MiniMax Code 测活配置")?;
        options.set("apiKey", request.api_key.trim());
        options.set(
            "baseURL",
            super::super::config_support::openai_base_url(request.base_url.trim()),
        );
        options.set("authMode", "api-key");
        let models = yaml_source::mapping_member(&entry, "models", "MiniMax Code 测活配置")?;
        let model_entry = yaml_source::mapping_member(&models, &flat, "MiniMax Code 测活配置")?;
        model_entry.set("reasoning", false);
        model_entry.set("name", model);
        Ok(())
    })?;
    let config_path = request
        .isolated_home
        .join(".minimax")
        .join("config.yaml");
    let mut environment = EnvironmentPatch::default();
    for name in [
        "MINIMAX_DATA_DIR",
        "MINIMAX_API_KEY",
        "MINIMAX_CN_API_KEY",
        "MINIMAX_API_PROVIDER_ID",
        "MINIMAX_PROVIDER_ID",
        "MINIMAX_OFFICIAL",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ] {
        environment.remove(name);
    }
    Ok(LivenessPlan {
        args: vec![
            "exec".to_string(),
            "--config".to_string(),
            config_path.to_string_lossy().into_owned(),
            "--permission".to_string(),
            "full".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
            "--timeout".to_string(),
            format!("{}s", request.timeout_seconds),
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

/// `exec --output-format json` prints one `exec.result` object. A failed turn
/// still exits 0 on some errors, so `status` carries the outcome.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let mut parsed = ParsedLivenessOutput::default();
    let mut usage = ParsedTokenUsage::default();
    for line in source.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("exec.result") {
            continue;
        }
        if let Some(tokens) = value.get("usage") {
            add_optional(&mut usage.input_tokens, extract_u64(tokens, "inputTokens"));
            add_optional(
                &mut usage.output_tokens,
                extract_u64(tokens, "outputTokens"),
            );
            add_optional(
                &mut usage.cached_input_tokens,
                extract_u64(tokens, "cacheReadTokens"),
            );
            add_optional(
                &mut usage.total_tokens,
                extract_u64(tokens, "totalTokens"),
            );
        }
        match value.get("status").and_then(Value::as_str) {
            Some("succeeded") => {
                if let Some(output) = value.get("output").and_then(Value::as_str) {
                    parsed.response = output.trim().chars().take(240).collect();
                }
                parsed.error = None;
            }
            _ => {
                parsed.response.clear();
                parsed.error = value
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| Some("MiniMax Code 测活执行失败".to_string()));
            }
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
    parsed.usage = usage;
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-minimax-test",
            base_url: "https://relay.example.com/v1/",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 60,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_writes_a_custom_openai_provider_config() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            [
                "exec",
                "--config",
                "/tmp/<liveness>/.minimax/config.yaml",
                "--permission",
                "full",
                "--output-format",
                "json",
                "--timeout",
                "60s",
                "Say OK",
            ]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-minimax-test")));
        assert_eq!(plan.files.len(), 1);
        let file = &plan.files[0];
        assert_eq!(file.path, request().isolated_home.join(".minimax/config.yaml"));
        let parsed = yaml_source::value(&file.content).unwrap();
        assert_eq!(
            parsed["defaultModel"],
            "custom_provider:balancehub/relay-model"
        );
        assert_eq!(
            parsed["custom_provider"]["balancehub"]["options"]["baseURL"],
            "https://relay.example.com/v1"
        );
        assert_eq!(
            parsed["custom_provider"]["balancehub"]["options"]["apiKey"],
            "sk-minimax-test"
        );
        assert_eq!(
            parsed["custom_provider"]["balancehub"]["api"],
            "openai-completions"
        );
        assert_eq!(
            parsed["custom_provider"]["balancehub"]["models"]["relay-model"]["name"],
            "relay-model"
        );
        assert!(plan.environment.removed_names().any(|name| name == "MINIMAX_API_KEY"));
        assert!(plan.environment.removed_names().any(|name| name == "MINIMAX_DATA_DIR"));
    }

    #[test]
    fn plan_flattens_slashes_in_the_model_key() {
        let mut request = request();
        request.model = "vendor/model";
        let plan = build_plan(request).unwrap();
        let parsed = yaml_source::value(&plan.files[0].content).unwrap();
        assert_eq!(
            parsed["defaultModel"],
            "custom_provider:balancehub/vendor~model"
        );
        assert_eq!(
            parsed["custom_provider"]["balancehub"]["models"]["vendor~model"]["name"],
            "vendor/model"
        );
    }

    #[test]
    fn parse_output_reads_the_result_and_usage() {
        let parsed = parse_output(
            r#"{"schemaVersion":1,"type":"exec.result","status":"succeeded","output":"BH_OK","usage":{"inputTokens":7,"outputTokens":2,"totalTokens":9},"usageSource":"completed_responses"}"#,
            "",
        );
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(9));
    }

    #[test]
    fn parse_output_surfaces_a_failed_status() {
        let parsed = parse_output(
            r#"{"schemaVersion":1,"type":"exec.result","status":"failed","error":{"category":"runtime","message":"Connection error."}}"#,
            "",
        );
        assert!(parsed.response.is_empty());
        assert_eq!(parsed.error.as_deref(), Some("Connection error."));
    }
}
