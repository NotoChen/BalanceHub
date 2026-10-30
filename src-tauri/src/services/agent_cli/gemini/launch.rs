use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use crate::{
    models::TemporaryCliSessionMode, services::oauth_accounts::native,
};

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = Vec::new();
    let mut environment = EnvironmentPatch::default();
    let mut auxiliary_files = Vec::new();
    let mut isolated_home = None;
    if !request.model.trim().is_empty() {
        args.extend(["--model".to_string(), request.model.trim().to_string()]);
    }
    match request.configuration {
        TemporaryLaunchConfiguration::Native => {}
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            let settings_path = request.launch_directory.join("gemini-system-settings.json");
            for name in [
                "GEMINI_API_KEY",
                "GOOGLE_API_KEY",
                "GOOGLE_GEMINI_BASE_URL",
                "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
                "GOOGLE_GENAI_USE_VERTEXAI",
                "GOOGLE_CLOUD_PROJECT",
                "GOOGLE_CLOUD_LOCATION",
                "GOOGLE_APPLICATION_CREDENTIALS",
                "GEMINI_CLI_HOME",
            ] {
                environment.remove(name);
            }
            environment.set("GEMINI_API_KEY", api_key.trim());
            environment.set("GOOGLE_GEMINI_BASE_URL", base_url.trim());
            environment.set_path("GEMINI_CLI_SYSTEM_SETTINGS_PATH", &settings_path);
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "gemini-system-settings.json",
                settings: true,
                content: system_settings("gemini-api-key")?,
            });
        }
        TemporaryLaunchConfiguration::OAuth { auth, identity, .. } => {
            // Gemini CLI keeps personal authorization in its own home, so the
            // isolated GEMINI_CLI_HOME holds both the credential file and the
            // authorization type that selects it.
            let home = request.launch_directory.join("gemini-home");
            let document = native::document(identity.vendor, auth, identity, false);
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "gemini-home/.gemini/oauth_creds.json",
                settings: false,
                content: serde_json::to_string_pretty(&document)
                    .map_err(|err| format!("生成 Gemini 临时授权失败: {err}"))?,
            });
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "gemini-home/.gemini/settings.json",
                settings: true,
                content: system_settings("oauth-personal")?,
            });
            for name in [
                "GEMINI_API_KEY",
                "GOOGLE_API_KEY",
                "GOOGLE_GEMINI_BASE_URL",
                "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
                "GOOGLE_GENAI_USE_VERTEXAI",
                "GOOGLE_CLOUD_PROJECT",
                "GOOGLE_CLOUD_LOCATION",
                "GOOGLE_APPLICATION_CREDENTIALS",
                "GEMINI_CLI_HOME",
            ] {
                environment.remove(name);
            }
            environment.set_path("GEMINI_CLI_HOME", &home);
            isolated_home = Some("gemini-home/.gemini");
        }
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

fn system_settings(auth_type: &str) -> Result<String, String> {
    serde_json::to_string_pretty(&serde_json::json!({
        "security": { "auth": { "selectedType": auth_type } }
    }))
    .map_err(|err| format!("生成 Gemini CLI 临时配置失败: {err}"))
}
