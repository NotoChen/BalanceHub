use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use super::config;
use crate::models::TemporaryCliSessionMode;

const PROVIDER_ID: &str = "balancehub";
const API_KEY_ENV: &str = "BALANCEHUB_DEVECO_API_KEY";

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
                return Err("DevEco Code 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let config_path = request.launch_directory.join("deveco.json");
            for name in ["DEVECO_CONFIG", "DEVECO_CONFIG_CONTENT"] {
                environment.remove(name);
            }
            environment.set_path("DEVECO_CONFIG", &config_path);
            environment.set(API_KEY_ENV, api_key.trim());
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "deveco.json",
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
            return Err("DevEco Code 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("DevEco Code 续聊缺少原生会话 ID".into());
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
    fn provider_launch_writes_an_explicit_config_and_keeps_the_key_in_env() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-deveco-test",
                base_url: "https://relay.example.com",
            },
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args, ["--model", "balancehub/relay-model"]);
        assert!(!plan.args.iter().any(|value| value.contains("sk-deveco-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == API_KEY_ENV && value == "sk-deveco-test"));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "DEVECO_CONFIG"
                && crate::test_support::same_path(value, "/tmp/<launch>/deveco.json")));
        let file = &plan.auxiliary_files[0];
        assert_eq!(file.path, "deveco.json");
        let parsed: Value = serde_json::from_str(&file.content).unwrap();
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
            TemporaryLaunchConfiguration::Native,
            "",
            "session-1",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert_eq!(plan.args, ["--session", "session-1"]);
    }
}
