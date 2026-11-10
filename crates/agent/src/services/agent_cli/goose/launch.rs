use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;
use serde_json::json;

const PROVIDER_ID: &str = "balancehub";

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let model = request.model.trim();
    let native = matches!(
        request.configuration,
        TemporaryLaunchConfiguration::Native
    );
    let mut args = vec!["session".to_string()];
    let mut environment = EnvironmentPatch::default();
    let mut auxiliary_files = Vec::new();
    match request.configuration {
        TemporaryLaunchConfiguration::Native => {}
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            if model.is_empty() {
                return Err("Goose 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            // Only the config directory moves: Goose keeps sessions under its
            // data directory, so history stays native and a resumed ID is
            // still found.
            let config_root = request.launch_directory.join("goose-config");
            for name in ["GOOSE_PROVIDER", "GOOSE_MODEL"] {
                environment.remove(name);
            }
            environment.set_path("XDG_CONFIG_HOME", &config_root);
            // The isolated config directory looks like a fresh install, which
            // would otherwise show the first-run telemetry prompt.
            environment.set("GOOSE_TELEMETRY_OFF", "1");
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "goose-config/goose/custom_providers/balancehub.json",
                settings: false,
                content: provider_config(model, base_url.trim(), api_key.trim())?,
            });
            args.extend([
                "--provider".to_string(),
                PROVIDER_ID.to_string(),
                "--model".to_string(),
                model.to_string(),
            ]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("Goose 不支持从 OAuth 账号启动".into());
        }
    }
    if native && !model.is_empty() {
        args.extend(["--model".to_string(), model.to_string()]);
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("Goose 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend([
            "--resume".to_string(),
            "--session-id".to_string(),
            resume_id.to_string(),
        ]);
    } else if !request.session_name.trim().is_empty() {
        args.extend([
            "-n".to_string(),
            request.session_name.trim().to_string(),
        ]);
    }
    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_files,
        isolated_home: None,
    })
}

fn provider_config(model: &str, base_url: &str, api_key: &str) -> Result<String, String> {
    let base_url = super::super::config_support::openai_base_url(base_url);
    serde_json::to_string_pretty(&json!({
        "name": PROVIDER_ID,
        "engine": "openai",
        "display_name": "BalanceHub",
        "description": "BalanceHub 中转站",
        "api_key_env": "",
        "base_url": base_url,
        "headers": {
            "Authorization": format!("Bearer {api_key}"),
        },
        "requires_auth": false,
        "skip_canonical_filtering": true,
        "dynamic_models": false,
        "models": [{ "name": model, "reasoning": false }],
    }))
    .map_err(|err| format!("生成 Goose 临时配置失败: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TemporaryCliSessionMode;

    fn request<'a>(
        configuration: TemporaryLaunchConfiguration<'a>,
        model: &'a str,
        name: &'a str,
        resume_id: &'a str,
        session_mode: TemporaryCliSessionMode,
    ) -> TemporaryLaunchRequest<'a> {
        TemporaryLaunchRequest {
            configuration,
            model,
            session_name: name,
            resume_id,
            session_mode,
            launch_directory: std::path::Path::new("<launch>"),
        }
    }

    #[test]
    fn provider_launch_isolates_the_config_and_selects_the_model() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-goose-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
            "",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();

        assert_eq!(
            plan.args,
            ["session", "--provider", PROVIDER_ID, "--model", "relay-model"]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-goose-test")));
        assert!(plan.environment.set_values().any(|(name, value)| {
            name == "XDG_CONFIG_HOME" && crate::test_support::ends_with_path(value, "/goose-config")
        }));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "GOOSE_TELEMETRY_OFF" && value == "1"));
        let file = plan.auxiliary_files.first().unwrap();
        assert_eq!(
            file.path,
            "goose-config/goose/custom_providers/balancehub.json"
        );
        let value: serde_json::Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(value["base_url"], "https://relay.example.com/v1");
        assert_eq!(value["headers"]["Authorization"], "Bearer sk-goose-test");
        assert_eq!(value["models"][0]["name"], "relay-model");
    }

    #[test]
    fn native_launch_only_adds_the_selected_model_and_name() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "relay-model",
            "审查任务",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();

        assert_eq!(
            plan.args,
            ["session", "--model", "relay-model", "-n", "审查任务"]
        );
        assert!(plan.auxiliary_files.is_empty());
        assert_eq!(plan.environment.set_values().count(), 0);
    }

    #[test]
    fn history_launch_resumes_the_exact_session_without_renaming() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "ignored-name",
            "20261109_3",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();

        assert_eq!(
            plan.args,
            ["session", "--resume", "--session-id", "20261109_3"]
        );
    }

    #[test]
    fn provider_launch_requires_a_model() {
        let error = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-goose-test",
                base_url: "https://relay.example.com/v1",
            },
            "",
            "",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap_err();
        assert!(error.contains("首选模型"));
    }
}
