use super::super::contracts::{
    EnvironmentPatch, TemporaryLaunchConfiguration, TemporaryLaunchPlan, TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = Vec::new();
    let mut environment = EnvironmentPatch::default();
    if let TemporaryLaunchConfiguration::Provider {
        provider_name,
        api_key,
        base_url,
    } = request.configuration
    {
        let display_name = provider_name.trim();
        let display_name = if display_name.is_empty() {
            "Custom"
        } else {
            display_name
        };
        if !request.model.trim().is_empty() {
            args.extend(["-m".to_string(), request.model.trim().to_string()]);
        }
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
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        args.push("resume".to_string());
        if !request.resume_id.trim().is_empty() {
            args.push(request.resume_id.trim().to_string());
        }
    }

    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_file_content: None,
    })
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
