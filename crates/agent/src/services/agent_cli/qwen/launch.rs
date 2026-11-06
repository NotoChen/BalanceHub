use super::super::contracts::{
    EnvironmentPatch, TemporaryLaunchConfiguration, TemporaryLaunchPlan, TemporaryLaunchRequest,
};

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = Vec::new();
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
            for name in ["OPENAI_API_KEY", "OPENAI_BASE_URL", "OPENAI_MODEL", "QWEN_MODEL"] {
                environment.remove(name);
            }
            // The key travels in the child environment rather than argv, so it
            // does not appear in process listings. The endpoint and auth type
            // are ordinary flags and take precedence over settings.json.
            environment.set("OPENAI_API_KEY", api_key.trim());
            environment.set("OPENAI_BASE_URL", base_url.trim());
            args.extend(["--auth-type".into(), "openai".into()]);
            args.extend(["--openai-base-url".into(), base_url.trim().into()]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("Qwen Code 不支持从 OAuth 账号启动".into());
        }
    }
    if !request.model.trim().is_empty() {
        args.extend(["--model".into(), request.model.trim().into()]);
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
    use crate::models::TemporaryCliSessionMode;

    fn provider_request<'a>(api_key: &'a str, base_url: &'a str, model: &'a str) -> TemporaryLaunchRequest<'a> {
        TemporaryLaunchRequest {
            configuration: TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key,
                base_url,
            },
            model,
            session_name: "",
            resume_id: "",
            session_mode: TemporaryCliSessionMode::New,
            launch_directory: std::path::Path::new("<launch>"),
        }
    }

    #[test]
    fn launch_forces_openai_auth_and_keeps_the_key_out_of_argv() {
        let plan = build_plan(provider_request(
            "sk-test",
            "https://relay.example.com/v1",
            "relay-model",
        ))
        .unwrap();

        assert_eq!(
            plan.args,
            [
                "--auth-type",
                "openai",
                "--openai-base-url",
                "https://relay.example.com/v1",
                "--model",
                "relay-model",
            ]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "OPENAI_API_KEY" && value == "sk-test"));
    }

    #[test]
    fn native_launch_only_adds_the_selected_model() {
        let plan = build_plan(TemporaryLaunchRequest {
            configuration: TemporaryLaunchConfiguration::Native,
            model: "relay-model",
            session_name: "",
            resume_id: "",
            session_mode: TemporaryCliSessionMode::New,
            launch_directory: std::path::Path::new("<launch>"),
        })
        .unwrap();

        assert_eq!(plan.args, ["--model", "relay-model"]);
        assert_eq!(plan.environment.set_values().count(), 0);
    }
}
