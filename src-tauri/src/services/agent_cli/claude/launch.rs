use super::super::contracts::{
    EnvironmentPatch, TemporaryLaunchPlan, TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let settings_path = request
        .auxiliary_file_path
        .ok_or_else(|| "Claude Code 临时启动缺少配置文件路径".to_string())?;
    let mut args = vec![
        "--settings".to_string(),
        settings_path.to_string_lossy().to_string(),
    ];
    if !request.model.trim().is_empty() {
        args.extend(["--model".to_string(), request.model.trim().to_string()]);
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::New)
        && !request.session_name.trim().is_empty()
    {
        args.extend([
            "--name".to_string(),
            request.session_name.trim().to_string(),
        ]);
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        args.push("--resume".to_string());
        if !request.resume_id.trim().is_empty() {
            args.push(request.resume_id.trim().to_string());
        }
    }

    let mut environment = EnvironmentPatch::default();
    environment.remove("ANTHROPIC_API_KEY");
    environment.remove("ANTHROPIC_AUTH_TOKEN");
    environment.remove("ANTHROPIC_BASE_URL");
    let content = serde_json::to_string_pretty(&serde_json::json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": request.api_key.trim(),
            "ANTHROPIC_BASE_URL": request.base_url.trim(),
        }
    }))
    .map_err(|err| format!("生成 Claude Code 临时配置失败: {err}"))?;

    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_file_content: Some(content),
    })
}
