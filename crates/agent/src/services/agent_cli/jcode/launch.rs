use super::super::contracts::{
    EnvironmentPatch, TemporaryLaunchConfiguration, TemporaryLaunchPlan, TemporaryLaunchRequest,
};
use super::liveness;
use crate::models::TemporaryCliSessionMode;

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = Vec::new();
    let mut environment = EnvironmentPatch::default();
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
                return Err("jcode 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            environment = liveness::compatible_environment(
                model,
                &super::super::config_support::openai_base_url(base_url.trim()),
                api_key.trim(),
            );
            args.extend([
                "--provider".into(),
                "openai-compatible".into(),
                "--model".into(),
                model.to_string(),
            ]);
            // 共享 daemon 已经运行时命令行 Provider 不会生效；本次启动使用
            // 独立 socket，让 jcode 用当前环境变量启动自己的服务进程。
            args.extend([
                "--socket".into(),
                request
                    .launch_directory
                    .join("jcode.sock")
                    .to_string_lossy()
                    .into_owned(),
            ]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("jcode 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("jcode 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend(["--resume".into(), resume_id.to_string()]);
    }
    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_files: Vec::new(),
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
    fn provider_launch_uses_the_environment_provider_and_a_private_socket() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-jcode-test",
                base_url: "https://relay.example.com",
            },
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(
            plan.args,
            [
                "--provider",
                "openai-compatible",
                "--model",
                "relay-model",
                "--socket",
                "/tmp/<launch>/jcode.sock"
            ]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-jcode-test")));
        assert!(plan.auxiliary_files.is_empty());
        assert!(plan.isolated_home.is_none());
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "BALANCEHUB_JCODE_API_KEY"
                && value == "sk-jcode-test"));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "JCODE_OPENAI_COMPAT_API_BASE"
                && value == "https://relay.example.com/v1"));
    }

    #[test]
    fn provider_launch_requires_a_model() {
        let error = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-jcode-test",
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
    fn native_launch_selects_the_model_without_touching_the_environment() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "relay-model",
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
            "session_owl_1794223705975_6358e9711aa3e638",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert_eq!(
            plan.args,
            [
                "--resume",
                "session_owl_1794223705975_6358e9711aa3e638"
            ]
        );
    }
}
