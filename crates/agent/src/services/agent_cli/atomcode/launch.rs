use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use super::config;
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
                return Err("AtomCode 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let config_path = request.launch_directory.join("config.toml");
            // 只覆盖本次进程的 provider 配置：ATOMCODE_HOME 保持原生位置，
            // 新会话仍写入原生会话库，退出后可以继续。
            for name in [
                "ATOMCODE_API_KEY",
                "ATOMCODE_SEED_CONFIG",
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
                "OPENAI_API_BASE",
                "ANTHROPIC_API_KEY",
                "OLLAMA_API_KEY",
            ] {
                environment.remove(name);
            }
            environment.set("BALANCEHUB_ATOMCODE_API_KEY", api_key.trim());
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "config.toml",
                settings: true,
                content: config::temporary_config(model, base_url.trim(), None)?,
            });
            args.extend([
                "--config".into(),
                config_path.to_string_lossy().into_owned(),
                "--provider".into(),
                PROVIDER_ID.into(),
                "--model".into(),
                model.into(),
            ]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("AtomCode 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("AtomCode 续聊缺少原生会话 ID".into());
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
    fn provider_launch_writes_an_isolated_config_and_keeps_native_sessions() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-atomcode-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(
            plan.args,
            [
                "--config",
                "/tmp/<launch>/config.toml",
                "--provider",
                "balancehub",
                "--model",
                "relay-model",
            ]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-atomcode-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "BALANCEHUB_ATOMCODE_API_KEY"
                && value == "sk-atomcode-test"));
        assert!(plan
            .environment
            .removed_names()
            .any(|name| name == "ATOMCODE_API_KEY"));
        assert!(plan
            .environment
            .set_values()
            .all(|(name, _)| name != "ATOMCODE_HOME"));
        let file = &plan.auxiliary_files[0];
        assert_eq!(file.path, "config.toml");
        assert!(file.settings);
        let parsed: toml::Value = file.content.parse().unwrap();
        assert_eq!(
            parsed["default_model"].as_str(),
            Some("balancehub/relay-model")
        );
        assert_eq!(
            parsed["provider_accounts"]["balancehub"]["api_key"].as_str(),
            Some("${BALANCEHUB_ATOMCODE_API_KEY}")
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
        assert_eq!(plan.args, ["--resume", "session-1"]);
    }
}
