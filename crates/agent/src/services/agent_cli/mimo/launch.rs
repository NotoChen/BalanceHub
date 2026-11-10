use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use super::config;
use crate::models::TemporaryCliSessionMode;

const PROVIDER_ID: &str = "balancehub";
const API_KEY_ENV: &str = "BALANCEHUB_MIMOCODE_API_KEY";

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
                return Err("MiMo Code 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let config_path = request.launch_directory.join("mimocode.json");
            // 只覆盖本次进程的 provider 配置：MIMOCODE_HOME / MIMOCODE_DB 保持
            // 原生位置，新会话仍写入原生会话库，退出后可以继续。
            for name in [
                "MIMOCODE_CONFIG",
                "MIMOCODE_CONFIG_CONTENT",
                "MIMOCODE_CONFIG_DIR",
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
            ] {
                environment.remove(name);
            }
            environment.set_path("MIMOCODE_CONFIG", &config_path);
            environment.set(API_KEY_ENV, api_key.trim());
            environment.set("MIMOCODE_DISABLE_AUTOUPDATE", "1");
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "mimocode.json",
                settings: true,
                content: config::temporary_config(
                    model,
                    base_url.trim(),
                    &format!("{{env:{API_KEY_ENV}}}"),
                )?,
            });
            args.extend(["--model".into(), format!("{PROVIDER_ID}/{model}")]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("MiMo Code 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("MiMo Code 续聊缺少原生会话 ID".into());
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
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
    fn provider_launch_uses_an_explicit_config_and_keeps_native_sessions() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-mimo-test",
                base_url: "https://relay.example.com",
            },
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args, ["--model", "balancehub/relay-model"]);
        assert!(!plan.args.iter().any(|value| value.contains("sk-mimo-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == API_KEY_ENV && value == "sk-mimo-test"));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "MIMOCODE_CONFIG"
                && crate::test_support::ends_with_path(value, "/mimocode.json")));
        assert!(plan
            .environment
            .set_values()
            .all(|(name, _)| name != "MIMOCODE_HOME" && name != "MIMOCODE_DB"));
        let file = plan
            .auxiliary_files
            .iter()
            .find(|file| file.path == "mimocode.json")
            .unwrap();
        assert!(file.settings);
        let value: Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(value["provider"][PROVIDER_ID]["options"]["apiKey"], format!("{{env:{API_KEY_ENV}}}"));
        assert_eq!(
            value["provider"][PROVIDER_ID]["options"]["baseURL"],
            "https://relay.example.com/v1"
        );
        assert_eq!(value["model"], "balancehub/relay-model");
    }

    #[test]
    fn provider_launch_requires_a_model() {
        let error = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-mimo-test",
                base_url: "https://relay.example.com",
            },
            "",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap_err();
        assert!(error.contains("首选模型"));
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
        assert!(plan.environment.removed_names().next().is_none());
    }

    #[test]
    fn history_launch_resumes_the_native_session() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "session-1",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert_eq!(plan.args, ["--session", "session-1"]);
    }
}
