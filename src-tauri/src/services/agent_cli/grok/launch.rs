use super::super::contracts::{
    EnvironmentPatch, TemporaryLaunchPlan, TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = Vec::new();
    if !request.model.trim().is_empty() {
        args.extend(["--model".to_string(), request.model.trim().to_string()]);
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        args.extend([
            "--resume".to_string(),
            request.resume_id.trim().to_string(),
        ]);
    }

    let mut environment = EnvironmentPatch::default();
    for name in [
        "GROK_MODELS_BASE_URL",
        "GROK_MODELS_LIST_URL",
        "XAI_API_KEY",
        "GROK_DISABLE_AUTOUPDATER",
    ] {
        environment.remove(name);
    }
    environment.set("GROK_MODELS_BASE_URL", request.base_url.trim());
    environment.set("XAI_API_KEY", request.api_key.trim());
    environment.set("GROK_DISABLE_AUTOUPDATER", "1");

    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_file_content: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_uses_endpoint_key_model_and_exact_resume_id() {
        let plan = build_plan(TemporaryLaunchRequest {
            provider_name: "Relay",
            api_key: "xai-test",
            base_url: "https://relay.example.com/v1",
            model: "grok-code-fast-1",
            session_name: "ignored",
            resume_id: "019c-grok-session",
            session_mode: TemporaryCliSessionMode::History,
            auxiliary_file_path: None,
        })
        .unwrap();

        assert_eq!(
            plan.args,
            [
                "--model",
                "grok-code-fast-1",
                "--resume",
                "019c-grok-session"
            ]
        );
        assert!(plan.environment.set_values().any(|(name, value)| {
            name == "GROK_MODELS_BASE_URL" && value == "https://relay.example.com/v1"
        }));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "XAI_API_KEY" && value == "xai-test"));
    }
}
