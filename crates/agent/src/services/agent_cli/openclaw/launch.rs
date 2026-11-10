use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use super::config;
use crate::models::TemporaryCliSessionMode;

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = vec!["chat".to_string()];
    let mut environment = EnvironmentPatch::default();
    let mut auxiliary_files = Vec::new();
    match request.configuration {
        TemporaryLaunchConfiguration::Native => {
            let model = request.model.trim();
            if !model.is_empty() {
                crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
                let config_path = request.launch_directory.join("openclaw-native.json");
                environment.remove("OPENCLAW_CONFIG_PATH");
                environment.set_path("OPENCLAW_CONFIG_PATH", &config_path);
                auxiliary_files.push(TemporaryAuxiliaryFile {
                    path: "openclaw-native.json",
                    settings: false,
                    content: config::temporary_native_config(model)?,
                });
            }
        }
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            let model = request.model.trim();
            if model.is_empty() {
                return Err("OpenClaw 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let config_path = request.launch_directory.join("openclaw.json");
            // 只替换本次进程的配置路径；OPENCLAW_STATE_DIR / profile 保持原生，
            // 新会话仍写入原生会话库，退出后可以直接继续。
            for name in ["OPENCLAW_CONFIG_PATH", "OPENAI_API_KEY", "OPENAI_BASE_URL"] {
                environment.remove(name);
            }
            environment.set_path("OPENCLAW_CONFIG_PATH", &config_path);
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "openclaw.json",
                settings: true,
                content: config::temporary_provider_config(
                    model,
                    &super::super::config_support::openai_base_url(base_url.trim()),
                    api_key.trim(),
                )?,
            });
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("OpenClaw 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("OpenClaw 续聊缺少原生会话键".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话键")?;
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
    fn provider_launch_writes_an_explicit_config_and_keeps_native_state() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-openclaw-test",
                base_url: "https://relay.example.com",
            },
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args, ["chat"]);
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "OPENCLAW_CONFIG_PATH"
                && crate::test_support::same_path(value, "/tmp/<launch>/openclaw.json")));
        assert!(!plan
            .environment
            .set_values()
            .any(|(name, _)| name == "OPENCLAW_STATE_DIR"));
        let file = plan
            .auxiliary_files
            .iter()
            .find(|file| file.path == "openclaw.json")
            .unwrap();
        assert!(file.settings);
        let config: Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(
            config.pointer("/models/providers/balancehub/baseUrl"),
            Some(&Value::String("https://relay.example.com/v1".into()))
        );
        assert_eq!(
            config.pointer("/models/providers/balancehub/apiKey"),
            Some(&Value::String("sk-openclaw-test".into()))
        );
        let selected = config
            .pointer("/agents/defaults/model")
            .and_then(|model| {
                model
                    .as_str()
                    .or_else(|| model.get("primary").and_then(Value::as_str))
            });
        assert_eq!(selected, Some("balancehub/relay-model"));
    }

    #[test]
    fn provider_launch_requires_a_model() {
        let error = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-openclaw-test",
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
    fn native_launch_without_a_model_leaves_the_native_config_alone() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args, ["chat"]);
        assert!(plan.environment.set_values().next().is_none());
        assert!(plan.environment.removed_names().next().is_none());
        assert!(plan.auxiliary_files.is_empty());
    }

    #[test]
    fn history_launch_resumes_the_native_session_key() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "agent:main:main",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert_eq!(plan.args, ["chat", "--session", "agent:main:main"]);
    }
}
