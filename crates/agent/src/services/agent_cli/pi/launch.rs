use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;
use super::config;

const PROVIDER_ID: &str = "balancehub";

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
                args.extend(["--model".into(), request.model.trim().into()]);
            }
        }
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            let model = request.model.trim();
            if model.is_empty() {
                return Err("Pi 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let agent_dir = request.launch_directory.join("agent");
            environment.remove("PI_CODING_AGENT_DIR");
            environment.set_path("PI_CODING_AGENT_DIR", &agent_dir);
            // models.json 只引用变量名，密钥不落到启动目录。
            environment.set("BALANCEHUB_PI_API_KEY", api_key.trim());
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "agent/models.json",
                settings: true,
                content: temporary_models(model, base_url.trim())?,
            });
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "agent/settings.json",
                settings: false,
                content: temporary_settings(model)?,
            });
            args.extend([
                "--provider".into(),
                PROVIDER_ID.into(),
                "--model".into(),
                model.into(),
            ]);
            // 独立 Agent 目录默认把会话放进启动目录；显式指回原生位置，
            // 退出后仍能从 Pi 原生的会话选择器继续。
            if let Some(directory) = config::native_session_dir() {
                args.extend(["--session-dir".into(), directory]);
            }
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("Pi 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::New)
        && !request.session_name.trim().is_empty()
    {
        crate::services::agent_cli::integration::validate_native_value(
            request.session_name,
            "会话名称",
        )?;
        args.extend(["--name".into(), request.session_name.trim().into()]);
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("Pi 续聊缺少原生会话 ID".into());
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

pub(super) fn temporary_models(model: &str, base_url: &str) -> Result<String, String> {
    let mut providers = serde_json::Map::new();
    providers.insert(
        PROVIDER_ID.to_string(),
        serde_json::json!({
            "name": "BalanceHub",
            "baseUrl": base_url,
            "api": "openai-completions",
            "apiKey": "${BALANCEHUB_PI_API_KEY}",
            "models": [{ "id": model, "name": model }],
        }),
    );
    serde_json::to_string_pretty(&serde_json::json!({
        "$schema": "https://pi.dev/schemas/models.schema.json",
        "providers": providers,
    }))
    .map_err(|err| format!("生成 Pi 临时模型配置失败: {err}"))
}

fn temporary_settings(model: &str) -> Result<String, String> {
    serde_json::to_string_pretty(&serde_json::json!({
        "$schema": "https://pi.dev/schemas/settings.schema.json",
        "defaultProvider": PROVIDER_ID,
        "defaultModel": model,
    }))
    .map_err(|err| format!("生成 Pi 临时设置失败: {err}"))
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
    fn provider_launch_isolates_the_agent_dir_and_keeps_the_key_out_of_argv() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-pi-test",
                base_url: "https://relay.example.com",
            },
            "relay-model",
            "Probe",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args[0..5], ["--provider", "balancehub", "--model", "relay-model", "--session-dir"]);
        assert_eq!(plan.args.last().unwrap(), "Probe");
        assert!(plan.args.iter().any(|arg| arg == "--name"));
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-pi-test")));
        assert!(plan.environment.set_values().any(|(name, value)| {
            name == "PI_CODING_AGENT_DIR" && value.ends_with("/agent")
        }));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "BALANCEHUB_PI_API_KEY" && value == "sk-pi-test"));
        let models = plan
            .auxiliary_files
            .iter()
            .find(|file| file.path == "agent/models.json")
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&models.content).unwrap();
        assert_eq!(
            value["providers"]["balancehub"]["apiKey"],
            "${BALANCEHUB_PI_API_KEY}"
        );
    }

    #[test]
    fn session_name_is_only_forwarded_to_a_new_session() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "Probe",
            "01a1c02b-e6ef-7657-8eb5-abd877eafac4",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert_eq!(
            plan.args,
            ["--session", "01a1c02b-e6ef-7657-8eb5-abd877eafac4"]
        );
    }

    #[test]
    fn native_launch_selects_the_model_without_touching_the_environment() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "relay-model",
            "",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args, ["--model", "relay-model"]);
        assert!(plan.environment.set_values().next().is_none());
        assert!(plan.environment.removed_names().next().is_none());
        assert!(plan.auxiliary_files.is_empty());
    }
}
