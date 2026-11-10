use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use super::config;
use crate::models::TemporaryCliSessionMode;

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = vec!["tui".to_string()];
    let mut environment = EnvironmentPatch::default();
    let mut auxiliary_files = Vec::new();
    match request.configuration {
        TemporaryLaunchConfiguration::Native => {
            let model = request.model.trim();
            if !model.is_empty() {
                crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
                // A patch row replaces the whole config object, so repeat the
                // provider dsh already uses and only swap the model.
                let (provider, _) = config::active_default_model().ok_or(
                    "dsh 原生配置没有可覆盖的默认模型；请先在 dsh 内选择模型或改用中转站启动",
                )?;
                let patch_path = request.launch_directory.join("balancehub-model.yml");
                auxiliary_files.push(TemporaryAuxiliaryFile {
                    path: "balancehub-model.yml",
                    settings: false,
                    content: config::model_patch(&provider, model)?,
                });
                args.extend([
                    "--patch".into(),
                    patch_path.to_string_lossy().into_owned(),
                ]);
            }
        }
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            let model = request.model.trim();
            if model.is_empty() {
                return Err("dsh 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let patch_path = request.launch_directory.join("balancehub.yml");
            // 只叠加本次启动的 patch：DSH_HOME 保持原生位置，新会话仍写入
            // 原生会话库，退出后可以继续。
            environment.set(config::API_KEY_ENV, api_key.trim());
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "balancehub.yml",
                settings: false,
                content: config::temporary_patch(model, base_url.trim())?,
            });
            args.extend([
                "--patch".into(),
                patch_path.to_string_lossy().into_owned(),
            ]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("dsh 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("dsh 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend(["--resume".into(), resume_id.into()]);
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
    fn provider_launch_overlays_a_patch_and_keeps_the_native_home() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-dsh-test",
                base_url: "https://relay.example.com",
            },
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(
            plan.args,
            ["tui", "--patch", "/tmp/<launch>/balancehub.yml"]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-dsh-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == config::API_KEY_ENV && value == "sk-dsh-test"));
        assert!(plan
            .environment
            .set_values()
            .all(|(name, _)| name != "DSH_HOME"));
        let file = plan
            .auxiliary_files
            .iter()
            .find(|file| file.path == "balancehub.yml")
            .unwrap();
        let value: serde_json::Value = serde_yaml_ng::from_str(&file.content).unwrap();
        assert_eq!(value[0]["config"]["providers"]["balancehub"]["baseURL"], "https://relay.example.com/v1");
        assert_eq!(value[1]["config"]["provider"], "balancehub");
    }

    #[test]
    fn provider_launch_requires_a_model() {
        let error = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-dsh-test",
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
    fn history_launch_resumes_the_native_session() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "session-1",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert_eq!(plan.args, ["tui", "--resume", "session-1"]);
    }

    #[test]
    fn native_launch_without_a_model_leaves_config_untouched() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args, ["tui"]);
        assert!(plan.auxiliary_files.is_empty());
        assert!(plan.environment.set_values().next().is_none());
    }
}
