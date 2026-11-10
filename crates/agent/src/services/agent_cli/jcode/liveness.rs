use super::super::{
    contracts::{
        EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource, ParsedLivenessOutput,
        ParsedTokenUsage,
    },
    liveness_support::{extract_u64, token_sum},
};
use serde_json::Value;

const API_KEY_ENV: &str = "BALANCEHUB_JCODE_API_KEY";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let model = request.model.trim();
    let mut environment = compatible_environment(
        model,
        &super::super::config_support::openai_base_url(request.base_url.trim()),
        request.api_key.trim(),
    );
    // 测活是临时进程，不把会话写进本机 jcode 状态目录。
    environment.set_path("JCODE_HOME", &request.isolated_home.join("jcode"));
    // 默认 socket 在 `$XDG_RUNTIME_DIR` 下，不随 JCODE_HOME 隔离；本机已有
    // 常驻服务时 `run` 会连过去而忽略本次环境变量。与临时启动一致，使用
    // 隔离目录里的私有 socket，让 jcode 按本次配置启动自己的服务进程。
    let socket = request.isolated_home.join("jcode.sock");
    Ok(LivenessPlan {
        args: vec![
            "--socket".to_string(),
            socket.to_string_lossy().into_owned(),
            "run".to_string(),
            "--json".to_string(),
            "-p".to_string(),
            "openai-compatible".to_string(),
            "-m".to_string(),
            model.to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files: Vec::new(),
        response_source: LivenessResponseSource::Stdout,
    })
}

/// jcode's built-in OpenAI-compatible provider reads its endpoint, model and
/// key from environment variables; the key itself is named by
/// `JCODE_OPENAI_COMPAT_API_KEY_NAME` so it never lands in a config file.
pub(super) fn compatible_environment(
    model: &str,
    base_url: &str,
    api_key: &str,
) -> EnvironmentPatch {
    let mut environment = EnvironmentPatch::default();
    for name in [
        "JCODE_PROVIDER",
        "JCODE_MODEL",
        "JCODE_ACTIVE_PROVIDER",
        "JCODE_INITIAL_PROVIDER_EXPLICIT",
        "JCODE_API_BASE",
        "JCODE_API_KEY",
        "JCODE_PROVIDER_PROFILE_ACTIVE",
        "JCODE_NAMED_PROVIDER_PROFILE",
        "JCODE_OPENAI_COMPAT_ENV_FILE",
        "JCODE_OPENAI_COMPAT_API_BASE",
        "JCODE_OPENAI_COMPAT_API_KEY_NAME",
        "JCODE_OPENAI_COMPAT_DEFAULT_MODEL",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "OPENAI_API_BASE",
    ] {
        environment.remove(name);
    }
    environment.set("JCODE_OPENAI_COMPAT_LOCAL_ENABLED", "1");
    environment.set("JCODE_OPENAI_COMPAT_API_BASE", base_url);
    environment.set("JCODE_OPENAI_COMPAT_API_KEY_NAME", API_KEY_ENV);
    environment.set("JCODE_OPENAI_COMPAT_DEFAULT_MODEL", model);
    environment.set(API_KEY_ENV, api_key);
    environment.set("JCODE_PROVIDER", "openai-compatible");
    environment
}

/// `run --json` prints one object with the final `text` and provider usage.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let source = source.trim();
    let value = serde_json::from_str::<Value>(source).ok().or_else(|| {
        let start = source.find('{')?;
        serde_json::from_str::<Value>(&source[start..]).ok()
    });
    let Some(value) = value else {
        return ParsedLivenessOutput::default();
    };
    let usage = value.get("usage").map(usage_from).unwrap_or_default();
    let error = value.get("error").and_then(error_message);
    if let Some(error) = error {
        return ParsedLivenessOutput {
            response: String::new(),
            error: Some(error),
            usage,
        };
    }
    ParsedLivenessOutput {
        response: value
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| text.chars().take(240).collect())
            .unwrap_or_default(),
        error: None,
        usage,
    }
}

fn usage_from(value: &Value) -> ParsedTokenUsage {
    let input = extract_u64(value, "input_tokens");
    let cache_read = extract_u64(value, "cache_read_input_tokens");
    let cache_write = extract_u64(value, "cache_creation_input_tokens");
    let cached = token_sum(&[cache_read, cache_write]);
    let output = extract_u64(value, "output_tokens");
    ParsedTokenUsage {
        input_tokens: input,
        cached_input_tokens: cached,
        output_tokens: output,
        reasoning_output_tokens: None,
        total_tokens: token_sum(&[input, cached, output]),
        total_cost_usd: value
            .get("cost_usd")
            .and_then(Value::as_f64)
            .filter(|cost| *cost > 0.0),
    }
}

fn error_message(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(str::to_owned)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-jcode-test",
            base_url: "https://relay.example.com",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 30,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_uses_the_environment_provider_and_keeps_the_key_out_of_argv() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            crate::test_support::slash_args(&plan.args),
            [
                "--socket",
                "/tmp/<liveness>/jcode.sock",
                "run",
                "--json",
                "-p",
                "openai-compatible",
                "-m",
                "relay-model",
                "Say OK"
            ]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-jcode-test")));
        assert!(plan.files.is_empty());
        let values: Vec<(&str, &str)> = plan.environment.set_values().collect();
        assert!(values.contains(&("JCODE_OPENAI_COMPAT_API_BASE", "https://relay.example.com/v1")));
        assert!(values.contains(&("JCODE_OPENAI_COMPAT_API_KEY_NAME", API_KEY_ENV)));
        assert!(values.contains(&(API_KEY_ENV, "sk-jcode-test")));
        assert!(values.contains(&("JCODE_PROVIDER", "openai-compatible")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| {
                name == "JCODE_HOME" && crate::test_support::ends_with_path(value, "/jcode")
            }));
    }

    #[test]
    fn parse_output_reads_the_text_and_usage() {
        let stdout = concat!(
            r#"{"session_id":"session_1","provider":"balancehub","model":"relay-model","#,
            r#""text":"BH_OK","usage":{"input_tokens":7,"output_tokens":2,"#,
            r#""cache_read_input_tokens":3,"cache_creation_input_tokens":1}}"#
        );
        let parsed = parse_output(stdout, "");
        assert_eq!(parsed.response, "BH_OK");
        assert!(parsed.error.is_none());
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(4));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(13));
    }

    #[test]
    fn parse_output_tolerates_a_leading_log_line() {
        let stdout = concat!(
            "warning: update available\n",
            r#"{"text":"BH_OK","usage":{"input_tokens":7,"output_tokens":2}}"#
        );
        let parsed = parse_output(stdout, "");
        assert_eq!(parsed.response, "BH_OK");
    }

    #[test]
    fn parse_output_surfaces_a_structured_error() {
        let stdout = r#"{"error":{"message":"Failed to send request"}}"#;
        let parsed = parse_output(stdout, "");
        assert!(parsed.response.is_empty());
        assert_eq!(parsed.error.as_deref(), Some("Failed to send request"));
    }
}
