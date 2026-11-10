use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use super::config;
use crate::models::TemporaryCliSessionMode;

const API_KEY_ENV: &str = "BALANCEHUB_OMP_API_KEY";

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = Vec::new();
    let mut environment = EnvironmentPatch::default();
    let mut auxiliary_files = Vec::new();
    match request.configuration {
        TemporaryLaunchConfiguration::Native => {
            let model = request.model.trim();
            if !model.is_empty() {
                crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
                args.extend(["--model".into(), model.to_string()]);
            }
        }
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            let model = request.model.trim();
            if model.is_empty() {
                return Err("omp 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let agent_dir = request.launch_directory.join("omp-agent");
            for name in [
                "PI_CONFIG_DIR",
                "PI_CODING_AGENT_DIR",
                "PI_CODING_AGENT_SESSION_DIR",
                "PI_PROFILE",
                "OMP_PROFILE",
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
            ] {
                environment.remove(name);
            }
            environment.set_path("PI_CODING_AGENT_DIR", &agent_dir);
            environment.set(API_KEY_ENV, api_key.trim());
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "omp-agent/config.yml",
                settings: false,
                content: config::temporary_config(model)?,
            });
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "omp-agent/models.yml",
                settings: true,
                // 密钥只引用变量名，不落到启动目录。
                content: config::temporary_models(
                    model,
                    base_url.trim(),
                    &format!("${{{API_KEY_ENV}}}"),
                )?,
            });
            args.extend([
                "--model".into(),
                format!("{}/{}", config::PROVIDER_ID, model),
            ]);
            // 独立 Agent 目录默认把会话放进启动目录；显式指回原生位置，
            // 退出后仍能从 omp 原生的会话选择器继续。
            if let Some(directory) = config::native_session_dir() {
                args.extend(["--session-dir".into(), directory]);
            }
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("omp 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("omp 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend(["--resume".into(), resume_id.to_string()]);
    }
    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_files,
        isolated_home: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml_ng::Value;
    use std::path::Path;

    fn request<'a>(
        configuration: TemporaryLaunchConfiguration<'a>,
        model: &'a str,
        session_name: &'a str,
        resume_id: &'a str,
        session_mode: TemporaryCliSessionMode,
    ) -> TemporaryLaunchRequest<'a> {
        TemporaryLaunchRequest {
            configuration,
            model,
            session_name,
            resume_id,
            session_mode,
            launch_directory: Path::new("/tmp/<launch>"),
        }
    }

    #[test]
    fn provider_launch_isolates_the_agent_dir_and_keeps_native_sessions() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-omp-test",
                base_url: "https://relay.example.com",
            },
            "relay-model",
            "Probe",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args[0], "--model");
        assert_eq!(plan.args[1], "balancehub/relay-model");
        assert_eq!(plan.args[2], "--session-dir");
        assert!(!plan.args.iter().any(|arg| arg == "--name"));
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-omp-test")));
        assert!(plan
            .environment
            .removed_names()
            .any(|name| name == "PI_CONFIG_DIR"));
        assert!(plan.environment.set_values().any(|(name, value)| {
            name == "PI_CODING_AGENT_DIR" && crate::test_support::ends_with_path(value, "/omp-agent")
        }));
        assert!(plan.environment.set_values().any(|(name, value)| {
            name == API_KEY_ENV && value == "sk-omp-test"
        }));
        let models = plan
            .auxiliary_files
            .iter()
            .find(|file| file.path == "omp-agent/models.yml")
            .unwrap();
        let value: Value = serde_yaml_ng::from_str(&models.content).unwrap();
        assert_eq!(
            value["providers"]["balancehub"]["baseUrl"].as_str(),
            Some("https://relay.example.com/v1")
        );
        let expected_key = format!("${{{API_KEY_ENV}}}");
        assert_eq!(
            value["providers"]["balancehub"]["apiKey"].as_str(),
            Some(expected_key.as_str())
        );
        let config = plan
            .auxiliary_files
            .iter()
            .find(|file| file.path == "omp-agent/config.yml")
            .unwrap();
        let value: Value = serde_yaml_ng::from_str(&config.content).unwrap();
        assert_eq!(
            value["modelRoles"]["default"].as_str(),
            Some("balancehub/relay-model")
        );
    }

    #[test]
    fn provider_launch_requires_a_model() {
        let error = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-omp-test",
                base_url: "https://relay.example.com",
            },
            "",
            "",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap_err();
        assert!(error.contains("首选模型"));
    }

    #[test]
    fn native_launch_selects_the_model_without_touching_the_environment() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "relay-model",
            "",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args, ["--model", "relay-model"]);
        assert!(plan.environment.set_values().next().is_none());
        assert!(plan.environment.removed_names().next().is_none());
        assert!(plan.auxiliary_files.is_empty());
    }

    #[test]
    fn history_launch_resumes_the_native_session_id() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "",
            "01a1c02b-e6ef-7657-8eb5-abd877eafac4",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert_eq!(
            plan.args,
            ["--resume", "01a1c02b-e6ef-7657-8eb5-abd877eafac4"]
        );
    }
}
