use super::super::contracts::{
    EnvironmentPatch, TemporaryLaunchConfiguration, TemporaryLaunchPlan, TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = Vec::new();
    let mut environment = EnvironmentPatch::default();
    let mut auxiliary_file_content = None;
    if let TemporaryLaunchConfiguration::Provider {
        api_key, base_url, ..
    } = request.configuration
    {
        let settings_path = request
            .auxiliary_file_path
            .ok_or_else(|| "Gemini CLI 临时启动缺少系统配置文件路径".to_string())?;
        if !request.model.trim().is_empty() {
            args.extend(["--model".to_string(), request.model.trim().to_string()]);
        }
        for name in [
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "GOOGLE_GEMINI_BASE_URL",
            "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
            "GOOGLE_GENAI_USE_VERTEXAI",
            "GOOGLE_CLOUD_PROJECT",
            "GOOGLE_CLOUD_LOCATION",
            "GOOGLE_APPLICATION_CREDENTIALS",
        ] {
            environment.remove(name);
        }
        environment.set("GEMINI_API_KEY", api_key.trim());
        environment.set("GOOGLE_GEMINI_BASE_URL", base_url.trim());
        environment.set(
            "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
            settings_path.to_string_lossy().to_string(),
        );
        auxiliary_file_content = Some(
            serde_json::to_string_pretty(&serde_json::json!({
                "security": {
                    "auth": {
                        "selectedType": "gemini-api-key"
                    }
                }
            }))
            .map_err(|err| format!("生成 Gemini CLI 临时配置失败: {err}"))?,
        );
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        args.push("--resume".to_string());
        if !request.resume_id.trim().is_empty() {
            args.push(request.resume_id.trim().to_string());
        }
    }

    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_file_content,
    })
}
