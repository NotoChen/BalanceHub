use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;
use serde_json::json;

const MODEL_PREFIX: &str = "custom:balancehub/";

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let model = request.model.trim();
    let mut args = Vec::new();
    let mut auxiliary_files = Vec::new();
    match request.configuration {
        TemporaryLaunchConfiguration::Native => {
            if !model.is_empty() {
                crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
                let path = request.launch_directory.join("droid/settings.json");
                auxiliary_files.push(TemporaryAuxiliaryFile {
                    path: "droid/settings.json",
                    settings: false,
                    content: selection_overlay(model)?,
                });
                args.extend([
                    "--settings".to_string(),
                    path.to_string_lossy().into_owned(),
                ]);
            }
        }
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            if model.is_empty() {
                return Err("Droid 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let path = request.launch_directory.join("droid/settings.json");
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "droid/settings.json",
                settings: false,
                content: provider_settings(model, base_url.trim(), api_key.trim())?,
            });
            args.extend([
                "--settings".to_string(),
                path.to_string_lossy().into_owned(),
            ]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("Droid 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("Droid 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend(["--resume".to_string(), resume_id.to_string()]);
    }
    Ok(TemporaryLaunchPlan {
        args,
        environment: EnvironmentPatch::default(),
        auxiliary_files,
        isolated_home: None,
    })
}

/// A native launch only overrides the session model. Droid merges `--settings`
/// over the user's own file, so native providers, MCP servers and auth remain
/// in place.
fn selection_overlay(model: &str) -> Result<String, String> {
    serde_json::to_string_pretty(&json!({
        "sessionDefaultSettings": { "model": model },
    }))
    .map_err(|err| format!("生成 Droid 临时设置失败: {err}"))
}

fn provider_settings(model: &str, base_url: &str, api_key: &str) -> Result<String, String> {
    let id = format!("{MODEL_PREFIX}{model}");
    serde_json::to_string_pretty(&json!({
        "customModels": [{
            "id": id,
            "displayName": model,
            "model": model,
            "provider": "generic-chat-completion-api",
            "baseUrl": super::super::config_support::openai_base_url(base_url),
            "apiKey": api_key,
            "noImageSupport": true,
        }],
        "sessionDefaultSettings": { "model": id },
    }))
    .map_err(|err| format!("生成 Droid 临时设置失败: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TemporaryCliSessionMode;

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
            launch_directory: std::path::Path::new("<launch>"),
        }
    }

    #[test]
    fn provider_launch_writes_a_generic_chat_completion_model() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-droid-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();

        assert_eq!(
            plan.args,
            ["--settings", "<launch>/droid/settings.json"]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-droid-test")));
        let file = plan.auxiliary_files.first().unwrap();
        let value: serde_json::Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(
            value["customModels"][0]["id"],
            "custom:balancehub/relay-model"
        );
        assert_eq!(
            value["customModels"][0]["provider"],
            "generic-chat-completion-api"
        );
        assert_eq!(value["customModels"][0]["baseUrl"], "https://relay.example.com/v1");
    }

    #[test]
    fn native_launch_only_overrides_the_model() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "claude-sonnet-4-6",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();

        assert_eq!(plan.args, ["--settings", "<launch>/droid/settings.json"]);
        let file = plan.auxiliary_files.first().unwrap();
        let value: serde_json::Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(
            value["sessionDefaultSettings"]["model"],
            "claude-sonnet-4-6"
        );
        assert!(value.get("customModels").is_none());
    }

    #[test]
    fn history_launch_resumes_the_native_session() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "5d3b2358-b9f9-4257-ab87-1d5f2f1d77ea",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();

        assert_eq!(
            plan.args,
            [
                "--resume",
                "5d3b2358-b9f9-4257-ab87-1d5f2f1d77ea"
            ]
        );
        assert!(plan.auxiliary_files.is_empty());
    }
}
