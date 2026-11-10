use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;

/// Files MothX reads from `$MOTHX_DIR` beside `settings.json`. Copying them
/// keeps MCP servers, tool environment, permission allow rules and the global
/// context file available without mutating the user's native config.
const NATIVE_FILES: &[&str] = &["mcp.json", "env.json", "allow.json", "global-context.md"];

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
                return Err("MothX 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;

            let native_root = super::config::native_config_root()?;
            let root = request.launch_directory.join("mothx");
            let settings = super::config::launch_settings(
                &native_root,
                model,
                &super::super::config_support::openai_base_url(base_url.trim()),
                api_key.trim(),
            )?;
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "mothx/settings.json",
                settings: true,
                content: settings,
            });
            for name in NATIVE_FILES {
                if let Some(content) = super::config::optional_config_text(&native_root, name) {
                    auxiliary_files.push(TemporaryAuxiliaryFile {
                        path: match *name {
                            "mcp.json" => "mothx/mcp.json",
                            "env.json" => "mothx/env.json",
                            "allow.json" => "mothx/allow.json",
                            "global-context.md" => "mothx/global-context.md",
                            _ => continue,
                        },
                        settings: false,
                        content,
                    });
                }
            }
            for name in [
                "MOTHX_DIR",
                "VIBECODING_PROVIDER",
                "VIBECODING_MODEL",
                "VIBECODING_MODE",
                "VIBECODING_THINKING",
            ] {
                environment.remove(name);
            }
            environment.set_path("MOTHX_DIR", &root);
            args.extend(["-p".into(), super::config::PROVIDER_ID.into()]);
            args.extend(["-m".into(), model.into()]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("MothX 不支持从 OAuth 账号启动".into());
        }
    }

    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("MothX 续聊缺少原生会话 ID".into());
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
    fn native_launch_only_adds_the_selected_model_and_resume_id() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "relay-model",
            "sess-1",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert_eq!(plan.args, ["-m", "relay-model", "--resume", "sess-1"]);
        assert_eq!(plan.environment.set_values().count(), 0);
        assert!(plan.auxiliary_files.is_empty());
    }

    #[test]
    fn provider_launch_isolates_the_config_and_keeps_the_key_out_of_argv() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-mothx-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args, ["-p", "balancehub", "-m", "relay-model"]);
        assert!(!plan.args.iter().any(|value| value.contains("sk-mothx-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "MOTHX_DIR" && value == "/tmp/<launch>/mothx"));
        for name in [
            "VIBECODING_PROVIDER",
            "VIBECODING_MODEL",
            "VIBECODING_MODE",
            "VIBECODING_THINKING",
        ] {
            assert!(plan.environment.removed_names().any(|removed| removed == name));
        }
        let settings = plan
            .auxiliary_files
            .iter()
            .find(|file| file.path == "mothx/settings.json")
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&settings.content).unwrap();
        assert_eq!(value["defaultProvider"], "balancehub");
        assert_eq!(value["defaultModel"], "relay-model");
        assert_eq!(
            value["providers"]["balancehub"]["baseUrl"],
            "https://relay.example.com/v1"
        );
        assert_eq!(
            value["providers"]["balancehub"]["apiKey"],
            "sk-mothx-test"
        );
        assert!(settings.content.contains("sessionDir"));
        assert!(settings.content.contains("skillsDir"));
    }

    #[test]
    fn provider_history_launch_keeps_resume() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-mothx-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
            "sess-1",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert_eq!(
            plan.args,
            ["-p", "balancehub", "-m", "relay-model", "--resume", "sess-1"]
        );
    }
}
