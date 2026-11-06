use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
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
    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_files,
        isolated_home: None,
    })
}

fn temporary_models(model: &str, base_url: &str) -> Result<String, String> {
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
