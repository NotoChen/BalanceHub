use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;

const PROVIDER_ID: &str = "balancehub";

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = Vec::new();
    let mut environment = EnvironmentPatch::default();
    let mut auxiliary_files = Vec::new();
    match request.configuration {
        TemporaryLaunchConfiguration::Native => {
            if !request.model.trim().is_empty() {
                crate::services::agent_cli::integration::validate_native_value(
                    request.model,
                    "模型 ID",
                )?;
                args.extend(["--model".into(), request.model.trim().into()]);
            }
        }
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            let model = request.model.trim();
            if model.is_empty() {
                return Err("OpenCode 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let settings_path = request.launch_directory.join("opencode.json");
            for name in ["OPENCODE_CONFIG", "OPENCODE_CONFIG_CONTENT"] {
                environment.remove(name);
            }
            environment.set_path("OPENCODE_CONFIG", &settings_path);
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "opencode.json",
                settings: false,
                content: temporary_config(model, base_url.trim(), api_key.trim())?,
            });
            args.extend([
                "--model".into(),
                format!("{PROVIDER_ID}/{}", model),
            ]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("OpenCode 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("OpenCode 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend(["--session".into(), resume_id.into()]);
    }
    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_files,
        isolated_home: None,
    })
}

pub(super) fn temporary_config(
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let mut models = serde_json::Map::new();
    models.insert(model.to_string(), serde_json::json!({ "name": model }));
    let mut provider = serde_json::Map::new();
    provider.insert(
        PROVIDER_ID.to_string(),
        serde_json::json!({
            "npm": "@ai-sdk/openai-compatible",
            "name": "BalanceHub",
            "options": {
                "baseURL": base_url,
                "apiKey": api_key,
            },
            "models": serde_json::Value::Object(models),
        }),
    );
    serde_json::to_string_pretty(&serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "provider": serde_json::Value::Object(provider),
        "model": format!("{PROVIDER_ID}/{model}"),
    }))
    .map_err(|err| format!("生成 OpenCode 临时配置失败: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TemporaryCliSessionMode;

    #[test]
    fn launch_uses_an_isolated_config_and_the_exact_model() {
        let plan = build_plan(TemporaryLaunchRequest {
            configuration: TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-test",
                base_url: "https://relay.example.com/v1",
            },
            model: "model-1",
            session_name: "",
            resume_id: "",
            session_mode: TemporaryCliSessionMode::New,
            launch_directory: std::path::Path::new("<launch>"),
        })
        .unwrap();

        assert_eq!(plan.args, ["--model", "balancehub/model-1"]);
        assert!(plan
            .environment
            .set_values()
            .any(|(name, _)| name == "OPENCODE_CONFIG"));
        let file = plan
            .auxiliary_files
            .iter()
            .find(|file| file.path == "opencode.json")
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(value["provider"][PROVIDER_ID]["options"]["apiKey"], "sk-test");
        assert_eq!(value["model"], "balancehub/model-1");
    }

    #[test]
    fn provider_launch_requires_a_model() {
        let error = build_plan(TemporaryLaunchRequest {
            configuration: TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-test",
                base_url: "https://relay.example.com/v1",
            },
            model: "",
            session_name: "",
            resume_id: "",
            session_mode: TemporaryCliSessionMode::New,
            launch_directory: std::path::Path::new("<launch>"),
        })
        .unwrap_err();

        assert!(error.contains("首选模型"));
    }

    #[test]
    fn native_launch_only_adds_the_selected_model() {
        let plan = build_plan(TemporaryLaunchRequest {
            configuration: TemporaryLaunchConfiguration::Native,
            model: "balancehub/model-1",
            session_name: "",
            resume_id: "",
            session_mode: TemporaryCliSessionMode::New,
            launch_directory: std::path::Path::new("<launch>"),
        })
        .unwrap();

        assert_eq!(plan.args, ["--model", "balancehub/model-1"]);
        assert!(plan.auxiliary_files.is_empty());
        assert_eq!(plan.environment.set_values().count(), 0);
    }

    #[test]
    fn history_launch_resumes_the_exact_session() {
        let plan = build_plan(TemporaryLaunchRequest {
            configuration: TemporaryLaunchConfiguration::Native,
            model: "",
            session_name: "",
            resume_id: "ses_abc123",
            session_mode: TemporaryCliSessionMode::History,
            launch_directory: std::path::Path::new("<launch>"),
        })
        .unwrap();

        assert_eq!(plan.args, ["--session", "ses_abc123"]);
    }
}
