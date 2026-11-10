use super::super::contracts::{
    EnvironmentPatch, TemporaryLaunchConfiguration, TemporaryLaunchPlan, TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;

/// Hermes accepts an OpenAI-compatible endpoint for this launch only. The key
/// travels as `OPENAI_API_KEY` paired with `OPENAI_BASE_URL`, which Hermes
/// recognises as the matching endpoint, so it never appears in argv.
pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = vec!["chat".to_string()];
    let mut environment = EnvironmentPatch::default();
    match request.configuration {
        TemporaryLaunchConfiguration::Native => {}
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            for name in [
                "HERMES_INFERENCE_PROVIDER",
                "HERMES_INFERENCE_MODEL",
                "HERMES_CUSTOM_API_KEY",
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
            ] {
                environment.remove(name);
            }
            environment.set("OPENAI_API_KEY", api_key.trim());
            environment.set("OPENAI_BASE_URL", base_url.trim());
            args.extend(["--provider".into(), "custom".into()]);
            args.extend(["--base-url".into(), base_url.trim().into()]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("Hermes Agent 暂不支持从 OAuth 账号启动".into());
        }
    }
    if !request.model.trim().is_empty() {
        crate::services::agent_cli::integration::validate_native_value(
            request.model.trim(),
            "模型 ID",
        )?;
        args.extend(["--model".into(), request.model.trim().into()]);
    }
    match request.session_mode {
        TemporaryCliSessionMode::History => {
            let resume_id = request.resume_id.trim();
            if resume_id.is_empty() {
                return Err("Hermes 续聊缺少原生会话 ID".into());
            }
            crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
            args.extend(["--resume".into(), resume_id.into()]);
        }
        TemporaryCliSessionMode::New => {}
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

    #[test]
    fn provider_launch_uses_explicit_endpoint_and_keeps_the_key_out_of_argv() {
        let plan = build_plan(TemporaryLaunchRequest {
            configuration: TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-hermes-test",
                base_url: "https://relay.example.com/v1",
            },
            model: "relay-model",
            session_name: "",
            resume_id: "",
            session_mode: TemporaryCliSessionMode::New,
            launch_directory: std::path::Path::new("<launch>"),
        })
        .unwrap();

        assert_eq!(
            plan.args,
            [
                "chat",
                "--provider",
                "custom",
                "--base-url",
                "https://relay.example.com/v1",
                "--model",
                "relay-model",
            ]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-hermes-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "OPENAI_API_KEY" && value == "sk-hermes-test"));
        assert!(plan.environment.set_values().any(|(name, value)| {
            name == "OPENAI_BASE_URL" && value == "https://relay.example.com/v1"
        }));
    }

    #[test]
    fn history_launch_resumes_the_exact_session() {
        let plan = build_plan(TemporaryLaunchRequest {
            configuration: TemporaryLaunchConfiguration::Native,
            model: "",
            session_name: "",
            resume_id: "hermes-session-1",
            session_mode: TemporaryCliSessionMode::History,
            launch_directory: std::path::Path::new("<launch>"),
        })
        .unwrap();

        assert_eq!(
            plan.args,
            ["chat", "--resume", "hermes-session-1"]
        );
        assert_eq!(plan.environment.set_values().count(), 0);
    }
}
