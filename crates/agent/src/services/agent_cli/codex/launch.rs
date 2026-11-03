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
        args.extend(["-m".to_string(), request.model.trim().to_string()]);
    }
    match request.configuration {
        TemporaryLaunchConfiguration::Native => {}
        TemporaryLaunchConfiguration::Provider {
            provider_name,
            api_key,
            base_url,
        } => {
            let display_name = provider_name.trim();
            let display_name = if display_name.is_empty() {
                "Custom"
            } else {
                display_name
            };
            args.extend([
                "-c".to_string(),
                "model_provider=\"custom\"".to_string(),
                "-c".to_string(),
                format!(
                    "model_providers.custom.name=\"{}\"",
                    escape_toml_string(display_name)
                ),
                "-c".to_string(),
                format!(
                    "model_providers.custom.base_url=\"{}\"",
                    escape_toml_string(base_url)
                ),
                "-c".to_string(),
                "model_providers.custom.wire_api=\"responses\"".to_string(),
                "-c".to_string(),
                "model_providers.custom.env_key=\"OPENAI_API_KEY\"".to_string(),
                "-c".to_string(),
                "model_providers.custom.requires_openai_auth=true".to_string(),
            ]);
            environment.remove("CODEX_API_KEY");
            environment.remove("CODEX_ACCESS_TOKEN");
            environment.set("OPENAI_API_KEY", api_key.trim());
        }
        TemporaryLaunchConfiguration::OAuth { auth, identity, .. } => {
            // Codex routes ChatGPT authorization through auth.json, never an
            // API key. The isolated home keeps the user's own login untouched.
            let home = request.launch_directory.join("codex-home");
            let document = native::document(identity.vendor, auth, identity, false);
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "codex-home/auth.json",
                settings: false,
                content: serde_json::to_string_pretty(&document)
                    .map_err(|err| format!("生成 Codex 临时授权失败: {err}"))?,
            });
            for name in [
                "OPENAI_API_KEY",
                "CODEX_API_KEY",
                "CODEX_ACCESS_TOKEN",
                "CODEX_HOME",
                "BALANCEHUB_CODEX_HOME",
            ] {
                environment.remove(name);
            }
            environment.set_path("CODEX_HOME", &home);
            isolated_home = Some("codex-home");
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        args.push("resume".to_string());
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

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
