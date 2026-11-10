use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;

const PROVIDER: &str = "custom_provider:balancehub";

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
                args.extend(["-m".into(), request.model.trim().into()]);
            }
        }
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            let model = request.model.trim();
            if model.is_empty() {
                return Err("MiniMax Code 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            if matches!(request.session_mode, TemporaryCliSessionMode::History) {
                // MiniMax Code has no per-run config flag. The only way to point
                // the TUI at a relay is an isolated data directory, and the
                // session database lives in that same directory, so a native
                // session id cannot be resumed there.
                return Err(
                    "MiniMax Code 中转站启动使用独立运行数据目录，不能续聊原生会话；请改用原生配置启动"
                        .into(),
                );
            }
            let config = super::config::merge_config("", model, base_url.trim(), api_key.trim())?;
            let root = request.launch_directory.join("minimax-data");
            let flat = model.replace('/', "~");
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "minimax-data/config.yaml",
                settings: false,
                content: config,
            });
            for name in [
                "MINIMAX_DATA_DIR",
                "MAVIS_DATA_DIR",
                "MAVIS_PROFILE",
                "MCODE_PROVIDER_API_KEY",
                "MCODE_CONFIG_DIR",
                "MINIMAX_API_KEY",
                "MINIMAX_CN_API_KEY",
                "MINIMAX_API_PROVIDER_ID",
                "MINIMAX_PROVIDER_ID",
                "MINIMAX_OFFICIAL",
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "ANTHROPIC_BASE_URL",
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
            ] {
                environment.remove(name);
            }
            environment.set_path("MINIMAX_DATA_DIR", &root);
            args.extend(["-m".into(), format!("{PROVIDER}/{flat}")]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("MiniMax Code 不支持从 OAuth 账号启动".into());
        }
    }

    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("MiniMax Code 续聊缺少原生会话 ID".into());
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
    fn native_launch_resumes_a_native_session() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "relay-model",
            "sess-1",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert_eq!(plan.args, ["-m", "relay-model", "--session", "sess-1"]);
        assert_eq!(plan.environment.set_values().count(), 0);
    }

    #[test]
    fn provider_launch_writes_an_isolated_runtime_config() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-minimax-test",
                base_url: "https://relay.example.com",
            },
            "relay/model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(
            plan.args,
            ["-m", "custom_provider:balancehub/relay~model"]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-minimax-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "MINIMAX_DATA_DIR"
                && value == "/tmp/<launch>/minimax-data"));
        assert!(plan
            .environment
            .removed_names()
            .any(|name| name == "MAVIS_DATA_DIR"));
        let config = plan
            .auxiliary_files
            .iter()
            .find(|file| file.path == "minimax-data/config.yaml")
            .unwrap();
        let value: serde_json::Value =
            crate::services::agent_cli::config_support::yaml_source::value(&config.content)
                .unwrap();
        assert_eq!(
            value["defaultModel"].as_str(),
            Some("custom_provider:balancehub/relay~model")
        );
        assert_eq!(
            value["custom_provider"]["balancehub"]["options"]["baseURL"].as_str(),
            Some("https://relay.example.com")
        );
    }

    #[test]
    fn provider_launch_refuses_native_history() {
        let error = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-minimax-test",
                base_url: "https://relay.example.com",
            },
            "relay-model",
            "sess-1",
            TemporaryCliSessionMode::History,
        ))
        .unwrap_err();
        assert!(error.contains("独立运行数据目录"));
    }
}
