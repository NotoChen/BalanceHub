use super::super::contracts::{
    EnvironmentPatch, TemporaryLaunchConfiguration, TemporaryLaunchPlan, TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;

const DEFAULT_CONTEXT: &str = "128000";

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
                return Err("Kimi Code 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            // KIMI_MODEL_* 在内存中定义临时 provider，不写 config.toml；
            // KIMI_CODE_HOME 保持原生位置，会话仍写入原生会话库。
            for name in [
                "KIMI_MODEL_NAME",
                "KIMI_MODEL_API_KEY",
                "KIMI_MODEL_PROVIDER_TYPE",
                "KIMI_MODEL_BASE_URL",
                "KIMI_MODEL_MAX_CONTEXT_SIZE",
                "KIMI_MODEL_CAPABILITIES",
                "KIMI_MODEL_DISPLAY_NAME",
                "KIMI_MODEL_MAX_OUTPUT_SIZE",
                "KIMI_MODEL_REASONING_KEY",
                "KIMI_MODEL_THINKING_EFFORT",
                "KIMI_MODEL_ADAPTIVE_THINKING",
                "KIMI_MODEL_MAX_COMPLETION_TOKENS",
                "KIMI_MODEL_TEMPERATURE",
                "KIMI_MODEL_TOP_P",
                "KIMI_MODEL_THINKING_KEEP",
                "KIMI_API_KEY",
                "KIMI_BASE_URL",
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
            ] {
                environment.remove(name);
            }
            environment.set("KIMI_MODEL_NAME", model);
            environment.set("KIMI_MODEL_API_KEY", api_key.trim());
            environment.set("KIMI_MODEL_PROVIDER_TYPE", "openai");
            environment.set(
                "KIMI_MODEL_BASE_URL",
                super::super::config_support::openai_base_url(base_url.trim()),
            );
            environment.set("KIMI_MODEL_MAX_CONTEXT_SIZE", DEFAULT_CONTEXT);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("Kimi Code 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("Kimi Code 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend(["--session".into(), resume_id.into()]);
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
    fn provider_launch_uses_an_in_memory_model_without_touching_config() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-kimi-test",
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
            .any(|(name, value)| *name == "KIMI_MODEL_NAME" && *value == "relay-model"));
        assert!(values
            .iter()
            .any(|(name, value)| *name == "KIMI_MODEL_API_KEY" && *value == "sk-kimi-test"));
        assert!(values
            .iter()
            .any(|(name, value)| *name == "KIMI_MODEL_PROVIDER_TYPE" && *value == "openai"));
        assert!(values.iter().any(|(name, value)| *name == "KIMI_MODEL_BASE_URL"
            && *value == "https://relay.example.com/v1"));
        assert!(values.iter().all(|(name, _)| *name != "KIMI_CODE_HOME"));
        assert!(plan
            .environment
            .removed_names()
            .any(|name| name == "KIMI_API_KEY"));
    }

    #[test]
    fn provider_launch_requires_a_model() {
        let error = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-kimi-test",
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
    fn native_launch_passes_the_model_alias_without_environment_changes() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "balancehub/relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args, ["--model", "balancehub/relay-model"]);
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
