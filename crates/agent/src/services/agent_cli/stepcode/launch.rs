use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use super::config;
use crate::models::TemporaryCliSessionMode;
use serde_json::json;
use toml_edit::{value as toml_value, Document as TomlDocument};

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
                return Err("StepCode 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let agent_dir = request.launch_directory.join("agent");
            environment.remove("STEP_CODING_AGENT_DIR");
            environment.set_path("STEP_CODING_AGENT_DIR", &agent_dir);
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "config.toml",
                settings: true,
                content: temporary_config(model)?,
            });
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "models.json",
                settings: false,
                content: temporary_models(model, base_url.trim(), api_key.trim())?,
            });
            args.extend([
                "--provider".into(),
                PROVIDER_ID.into(),
                "--model".into(),
                model.into(),
            ]);
            // 独立配置目录默认会把会话写进启动目录；显式指回原生位置，
            // 退出后仍能从 StepCode 原生的会话选择器继续。
            if let Some(directory) = config::native_session_dir() {
                args.extend(["--session-dir".into(), directory]);
            }
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("StepCode 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("StepCode 续聊缺少原生会话 ID".into());
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

fn temporary_config(model: &str) -> Result<String, String> {
    let mut config = TomlDocument::new();
    config
        .as_table_mut()
        .insert("defaultProvider", toml_value(PROVIDER_ID));
    config.as_table_mut().insert("defaultModel", toml_value(model));
    Ok(config.to_string())
}

fn temporary_models(model: &str, base_url: &str, api_key: &str) -> Result<String, String> {
    serde_json::to_string_pretty(&json!({
        "providers": {
            PROVIDER_ID: {
                "name": "BalanceHub",
                "baseUrl": base_url,
                "api": "openai-completions",
                "apiKey": api_key,
                "models": [{ "id": model, "name": model }],
            },
        },
    }))
    .map_err(|err| format!("生成 StepCode 临时模型配置失败: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>(
        configuration: TemporaryLaunchConfiguration<'a>,
        model: &'a str,
        resume_id: &'a str,
        session_mode: TemporaryCliSessionMode,
    ) -> TemporaryLaunchRequest<'a> {
        TemporaryLaunchRequest {
            configuration,
            model,
            session_name: "",
            resume_id,
            session_mode,
            launch_directory: Path::new("/tmp/<launch>"),
        }
    }

    #[test]
    fn provider_launch_writes_isolated_config_and_points_at_native_sessions() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-step-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert!(plan
            .args
            .starts_with(&["--provider".into(), "balancehub".into()]));
        assert!(plan.args.windows(2).any(|pair| pair == ["--model", "relay-model"]));
        assert!(!plan.args.iter().any(|value| value.contains("sk-step-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "STEP_CODING_AGENT_DIR"
                && value == "/tmp/<launch>/agent"));
        let config = plan
            .auxiliary_files
            .iter()
            .find(|file| file.path == "config.toml")
            .unwrap();
        assert!(config.settings);
        let parsed = config.content.parse::<toml::Value>().unwrap();
        assert_eq!(parsed["defaultProvider"].as_str(), Some("balancehub"));
        let models = plan
            .auxiliary_files
            .iter()
            .find(|file| file.path == "models.json")
            .unwrap();
        assert!(!models.settings);
        let parsed: serde_json::Value = serde_json::from_str(&models.content).unwrap();
        assert_eq!(
            parsed["providers"][PROVIDER_ID]["apiKey"],
            "sk-step-test"
        );
    }

    #[test]
    fn native_launch_passes_the_model_without_writing_config() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "native-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args, ["--model", "native-model"]);
        assert!(plan.auxiliary_files.is_empty());
        assert!(plan.environment.set_values().next().is_none());
    }

    #[test]
    fn history_launch_resumes_the_native_session() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-step-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
            "session-1",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert!(plan.args.windows(2).any(|pair| pair == ["--session", "session-1"]));
    }
}
