use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = Vec::new();
    let environment = EnvironmentPatch::default();
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
            provider_name,
            api_key,
            base_url,
        } => {
            let model = request.model.trim();
            if model.is_empty() {
                return Err("Qoder 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            // `--settings` is the highest-precedence settings layer. The
            // overlay defines a private provider and selects its model without
            // touching the logged-in user's `~/.qoder/settings.json`, so native
            // sessions and Qoder authorization stay intact.
            let settings = super::config::merge_config(
                "",
                provider_name,
                model,
                base_url.trim(),
                api_key.trim(),
            )?;
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "qoder-settings.json",
                settings: true,
                content: settings,
            });
            args.extend([
                "--settings".into(),
                request
                    .launch_directory
                    .join("qoder-settings.json")
                    .to_string_lossy()
                    .into_owned(),
            ]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("Qoder 不支持从 OAuth 账号启动".into());
        }
    }

    if !matches!(request.session_mode, TemporaryCliSessionMode::History)
        && !request.session_name.trim().is_empty()
    {
        crate::services::agent_cli::integration::validate_native_value(
            request.session_name,
            "会话名称",
        )?;
        args.extend(["-n".into(), request.session_name.trim().into()]);
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("Qoder 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend(["-r".into(), resume_id.into()]);
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
        session_name: &'a str,
        resume_id: &'a str,
        session_mode: TemporaryCliSessionMode,
    ) -> TemporaryLaunchRequest<'a> {
        TemporaryLaunchRequest {
            configuration,
            model,
            session_name,
            resume_id,
            session_mode,
            launch_directory: Path::new("/tmp/<launch>"),
        }
    }

    #[test]
    fn native_launch_sets_model_name_and_resume() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "relay-model",
            "新会话",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(
            plan.args,
            ["-m", "relay-model", "-n", "新会话"]
        );
        assert!(plan.auxiliary_files.is_empty());

        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "会被忽略",
            "sess-1",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert_eq!(plan.args, ["-r", "sess-1"]);
    }

    #[test]
    fn provider_launch_uses_a_private_settings_overlay() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-qoder-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
            "",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(
            plan.args,
            ["--settings", "/tmp/<launch>/qoder-settings.json"]
        );
        assert!(!plan.args.iter().any(|value| value.contains("sk-qoder-test")));
        let settings = &plan.auxiliary_files[0];
        assert!(settings.settings);
        let value: serde_json::Value = serde_json::from_str(&settings.content).unwrap();
        assert_eq!(value["model"]["name"], "balancehub/relay-model");
        assert_eq!(
            value["providers"]["balancehub"]["baseUrl"],
            "https://relay.example.com/v1"
        );
        assert_eq!(
            value["providers"]["balancehub"]["apiKey"],
            "sk-qoder-test"
        );
    }
}
