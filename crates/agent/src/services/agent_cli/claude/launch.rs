use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = Vec::new();
    let mut environment = EnvironmentPatch::default();
    let mut auxiliary_files = Vec::new();
    let isolated_home = None;
    match request.configuration {
        TemporaryLaunchConfiguration::Native => {}
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            environment.remove("ANTHROPIC_API_KEY");
            environment.remove("ANTHROPIC_AUTH_TOKEN");
            environment.remove("ANTHROPIC_BASE_URL");
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "claude-settings.json",
                settings: true,
                content: serde_json::to_string_pretty(&serde_json::json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": api_key.trim(),
                        "ANTHROPIC_BASE_URL": base_url.trim(),
                    }
                }))
                .map_err(|err| format!("生成 Claude Code 临时配置失败: {err}"))?,
            });
        }
        TemporaryLaunchConfiguration::OAuth { auth, .. } => {
            // Claude Code accepts a subscription access token directly from
            // CLAUDE_CODE_OAUTH_TOKEN, which outranks stored sign-ins, so the
            // account's own keychain entry and .credentials.json stay as they
            // are. The settings file stays empty and exists only so per-launch
            // run settings have somewhere to merge.
            for name in [
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "ANTHROPIC_BASE_URL",
                "CLAUDE_CODE_OAUTH_TOKEN",
            ] {
                environment.remove(name);
            }
            environment.set("CLAUDE_CODE_OAUTH_TOKEN", auth.access_token.trim());
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "claude-settings.json",
                settings: true,
                content: "{}".to_string(),
            });
        }
    }
    if let Some(file) = auxiliary_files.iter().find(|file| file.settings) {
        args.splice(
            0..0,
            [
                "--settings".to_string(),
                request
                    .launch_directory
                    .join(file.path)
                    .to_string_lossy()
                    .into_owned(),
            ],
        );
    }
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

    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_files,
        isolated_home,
    })
}
