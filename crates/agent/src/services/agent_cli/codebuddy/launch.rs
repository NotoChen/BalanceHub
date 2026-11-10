use super::super::contracts::{
    EnvironmentPatch, TemporaryLaunchConfiguration, TemporaryLaunchPlan, TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = Vec::new();
    let mut environment = EnvironmentPatch::default();
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
                return Err("CodeBuddy 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            // CodeBuddy 的第三方模型变量直接选择 OpenAI 兼容端点，不经过
            // models.json，也不改动原生 CODEBUDDY_CONFIG_DIR 里的配置。
            for name in [
                "CODEBUDDY_API_KEY",
                "CODEBUDDY_BASE_URL",
                "CODEBUDDY_MODEL",
                "CODEBUDDY_BIG_SLOW_MODEL",
                "CODEBUDDY_SMALL_FAST_MODEL",
                "CODEBUDDY_CODE_SUBAGENT_MODEL",
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_BASE_URL",
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
            ] {
                environment.remove(name);
            }
            environment.set(
                "CODEBUDDY_BASE_URL",
                super::super::config_support::openai_base_url(base_url.trim()),
            );
            environment.set("CODEBUDDY_API_KEY", api_key.trim());
            for name in [
                "CODEBUDDY_MODEL",
                "CODEBUDDY_BIG_SLOW_MODEL",
                "CODEBUDDY_SMALL_FAST_MODEL",
                "CODEBUDDY_CODE_SUBAGENT_MODEL",
            ] {
                environment.set(name, model);
            }
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("CodeBuddy 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("CodeBuddy 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend(["--resume".into(), resume_id.into()]);
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
    fn provider_launch_uses_third_party_model_environment_without_writing_config() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-codebuddy-test",
                base_url: "https://relay.example.com",
            },
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert!(plan.args.is_empty());
        assert!(plan.auxiliary_files.is_empty());
        let values: Vec<_> = plan.environment.set_values().collect();
        assert!(values
            .iter()
            .any(|(name, value)| *name == "CODEBUDDY_BASE_URL"
                && *value == "https://relay.example.com/v1"));
        assert!(values
            .iter()
            .any(|(name, value)| *name == "CODEBUDDY_API_KEY" && *value == "sk-codebuddy-test"));
        for name in [
            "CODEBUDDY_MODEL",
            "CODEBUDDY_BIG_SLOW_MODEL",
            "CODEBUDDY_SMALL_FAST_MODEL",
            "CODEBUDDY_CODE_SUBAGENT_MODEL",
        ] {
            assert!(values
                .iter()
                .any(|(candidate, value)| *candidate == name && *value == "relay-model"));
        }
        assert!(plan
            .environment
            .removed_names()
            .any(|name| name == "ANTHROPIC_API_KEY"));
    }

    #[test]
    fn provider_launch_requires_a_model() {
        let error = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-codebuddy-test",
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
    fn native_launch_passes_the_model_without_environment_changes() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "native-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args, ["--model", "native-model"]);
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
