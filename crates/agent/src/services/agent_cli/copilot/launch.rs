use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;
use serde_json::json;

const PROVIDER_ID: &str = "balancehub";

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let model = request.model.trim();
    let mut args = Vec::new();
    let mut environment = EnvironmentPatch::default();
    let mut auxiliary_files = Vec::new();
    match request.configuration {
        TemporaryLaunchConfiguration::Native => {}
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            if model.is_empty() {
                return Err("Copilot CLI 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let path = request.launch_directory.join("copilot/providers.json");
            // Only the BYOK provider file moves: Copilot keeps sessions and
            // settings under `COPILOT_HOME`, so history stays native.
            environment.remove("COPILOT_PROVIDERS_CONFIG");
            environment.set_path("COPILOT_PROVIDERS_CONFIG", &path);
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "copilot/providers.json",
                settings: false,
                content: provider_file(model, base_url.trim(), api_key.trim())?,
            });
            args.extend([
                "--model".to_string(),
                format!("{PROVIDER_ID}/{model}"),
            ]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("Copilot CLI 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(
        request.configuration,
        TemporaryLaunchConfiguration::Native
    ) && !model.is_empty()
    {
        args.extend(["--model".to_string(), model.to_string()]);
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("Copilot CLI 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend(["--resume".to_string(), resume_id.to_string()]);
    } else if !request.session_name.trim().is_empty() {
        args.extend([
            "-n".to_string(),
            request.session_name.trim().to_string(),
        ]);
    }
    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_files,
        isolated_home: None,
    })
}

fn provider_file(model: &str, base_url: &str, api_key: &str) -> Result<String, String> {
    serde_json::to_string_pretty(&json!({
        "providers": [{
            "name": PROVIDER_ID,
            "type": "openai",
            "baseUrl": super::super::config_support::openai_base_url(base_url),
            "apiKey": api_key,
        }],
        "models": [{
            "id": model,
            "provider": PROVIDER_ID,
            "name": model,
        }],
    }))
    .map_err(|err| format!("生成 Copilot CLI 临时 Provider 失败: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TemporaryCliSessionMode;

    fn request<'a>(
        configuration: TemporaryLaunchConfiguration<'a>,
        model: &'a str,
        name: &'a str,
        resume_id: &'a str,
        session_mode: TemporaryCliSessionMode,
    ) -> TemporaryLaunchRequest<'a> {
        TemporaryLaunchRequest {
            configuration,
            model,
            session_name: name,
            resume_id,
            session_mode,
            launch_directory: std::path::Path::new("<launch>"),
        }
    }

    #[test]
    fn provider_launch_isolates_only_the_provider_file() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-copilot-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
            "",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();

        assert_eq!(plan.args, ["--model", "balancehub/relay-model"]);
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-copilot-test")));
        assert!(plan.environment.set_values().any(|(name, value)| {
            name == "COPILOT_PROVIDERS_CONFIG" && value.ends_with("/copilot/providers.json")
        }));
        let file = plan.auxiliary_files.first().unwrap();
        let value: serde_json::Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(
            value["providers"][0]["baseUrl"],
            "https://relay.example.com/v1"
        );
        assert_eq!(value["models"][0]["id"], "relay-model");
    }

    #[test]
    fn native_launch_only_adds_the_selected_model_and_name() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "claude-sonnet-4-6",
            "审查任务",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();

        assert_eq!(
            plan.args,
            ["--model", "claude-sonnet-4-6", "-n", "审查任务"]
        );
        assert!(plan.auxiliary_files.is_empty());
        assert_eq!(plan.environment.set_values().count(), 0);
    }

    #[test]
    fn history_launch_resumes_the_exact_session() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "",
            "c2bcf122-8e19-46cc-83e1-c45ce1e01136",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();

        assert_eq!(
            plan.args,
            [
                "--resume",
                "c2bcf122-8e19-46cc-83e1-c45ce1e01136"
            ]
        );
    }
}
