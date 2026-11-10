use super::super::{
    contracts::{
        AgentFilePlan, EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput,
    },
    opencode_family,
};
use super::config;
use crate::services::cli_paths::{configured_path, user_home};
use serde_json::Value;
use std::{fs, path::PathBuf};

const PROVIDER_ID: &str = "balancehub";
const API_KEY_ENV: &str = "BALANCEHUB_DEVECO_API_KEY";

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    crate::services::agent_cli::integration::validate_native_value(request.api_key, "API Key")?;
    crate::services::agent_cli::integration::validate_native_value(
        request.base_url,
        "中转站地址",
    )?;
    crate::services::agent_cli::integration::validate_native_value(request.model, "模型 ID")?;
    let model = request.model.trim();
    let config_dir = request.isolated_home.join(".config/deveco");
    let config_path = config_dir.join("config.json");
    // DevEco Code 展开 OpenCode 风格的 `{env:NAME}`，密钥不落盘。
    let config = config::temporary_config(model, request.base_url.trim(), &format!("{{env:{API_KEY_ENV}}}"))?;
    let mut files = vec![AgentFilePlan {
        path: config_path,
        content: config,
    }];
    // 产品协议必须由用户本人在本机确认；这里只复用已存在的同意记录，
    // 不代替用户接受协议。缺失时让 DevEco 自己返回协议错误。
    let consent_path = request
        .isolated_home
        .join(".local/state/deveco/product-agreement.json");
    if let Some(consent) = native_consent() {
        files.push(AgentFilePlan {
            path: consent_path,
            content: consent,
        });
    }
    let mut environment = EnvironmentPatch::default();
    for name in [
        "DEVECO_CONFIG",
        "DEVECO_CONFIG_CONTENT",
        "DEVECO_DB",
        "DEVECO_API_KEY",
        "DEVECO_API_URL",
        "DEVECO_AUTH_CONTENT",
        "XDG_STATE_HOME",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "OPENAI_API_BASE",
    ] {
        environment.remove(name);
    }
    environment.set_path("DEVECO_CONFIG_DIR", &config_dir);
    // 关闭默认插件与技能，避免每次测活都安装数十兆的插件依赖。
    environment.set("DEVECO_DISABLE_DEFAULT_PLUGINS", "1");
    environment.set("DEVECO_DISABLE_DEFAULT_SKILLS", "1");
    environment.set("DEVECO_DISABLE_AUTOUPDATE", "1");
    environment.set("DEVECO_CLI_DISABLE_UPDATE", "1");
    environment.set("DEVECO_CLI_SKIP_VERSION_CHECK", "1");
    environment.set(API_KEY_ENV, request.api_key.trim());
    Ok(LivenessPlan {
        args: vec![
            "run".to_string(),
            "--pure".to_string(),
            "--model".to_string(),
            format!("{PROVIDER_ID}/{model}"),
            "--format".to_string(),
            "json".to_string(),
            request.prompt.to_string(),
        ],
        environment,
        files,
        response_source: LivenessResponseSource::Stdout,
    })
}

/// `run --format json` uses the same event stream as OpenCode: `text` carries
/// the answer and `step_finish` the token usage.
pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    opencode_family::parse_run_events("DevEco Code", response_output, stdout)
}

fn native_consent() -> Option<String> {
    let state_home = configured_path("XDG_STATE_HOME")
        .filter(|path| path.is_absolute())
        .or_else(|| user_home().map(|home| home.join(".local/state")))?;
    let path: PathBuf = state_home.join("deveco/product-agreement.json");
    let text = fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&text).ok()?;
    let version = value.get("version").and_then(Value::as_str)?.trim();
    (!version.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> LivenessRequest<'a> {
        LivenessRequest {
            api_key: "sk-deveco-test",
            base_url: "https://relay.example.com",
            model: "relay-model",
            prompt: "Say OK",
            timeout_seconds: 60,
            isolated_home: Path::new("/tmp/<liveness>"),
            output_path: Path::new("/tmp/<liveness>/response.txt"),
        }
    }

    #[test]
    fn plan_writes_an_isolated_config_and_keeps_the_key_in_env() {
        let plan = build_plan(request()).unwrap();
        assert_eq!(
            plan.args,
            [
                "run",
                "--pure",
                "--model",
                "balancehub/relay-model",
                "--format",
                "json",
                "Say OK",
            ]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-deveco-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == API_KEY_ENV && value == "sk-deveco-test"));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "DEVECO_CONFIG_DIR"
                && crate::test_support::same_path(value, "/tmp/<liveness>/.config/deveco")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "DEVECO_DISABLE_DEFAULT_PLUGINS" && value == "1"));
        assert!(plan
            .environment
            .removed_names()
            .any(|name| name == "DEVECO_CONFIG_CONTENT"));
        let config = plan
            .files
            .iter()
            .find(|file| file.path.ends_with("config.json"))
            .unwrap();
        let parsed: Value = serde_json::from_str(&config.content).unwrap();
        assert_eq!(parsed["model"], "balancehub/relay-model");
        assert_eq!(
            parsed["provider"]["balancehub"]["options"]["apiKey"],
            "{env:BALANCEHUB_DEVECO_API_KEY}"
        );
        assert_eq!(
            parsed["provider"]["balancehub"]["options"]["baseURL"],
            "https://relay.example.com/v1"
        );
    }

    #[test]
    fn text_and_step_finish_events_supply_the_answer_and_usage() {
        let parsed = parse_output(
            r#"{"type":"step_start"}
{"type":"text","part":{"type":"text","text":"OK"}}
{"type":"step_finish","part":{"type":"step-finish","tokens":{"total":9,"input":7,"output":2,"reasoning":1,"cache":{"write":0,"read":3}},"cost":0}}"#,
            "",
        );
        assert_eq!(parsed.response, "OK");
        assert_eq!(parsed.error, None);
        assert_eq!(parsed.usage.input_tokens, Some(7));
        assert_eq!(parsed.usage.cached_input_tokens, Some(3));
        assert_eq!(parsed.usage.output_tokens, Some(2));
        assert_eq!(parsed.usage.total_tokens, Some(9));
    }

    #[test]
    fn consent_error_event_surfaces_the_message() {
        let parsed = parse_output(
            r#"{"type":"error","error":{"data":{"message":"Review the product agreements in DevEco Code or run `deveco auth consent` on the server before continuing."}}}"#,
            "",
        );
        assert_eq!(
            parsed.error.as_deref(),
            Some(
                "Review the product agreements in DevEco Code or run `deveco auth consent` on the server before continuing."
            )
        );
    }
}
