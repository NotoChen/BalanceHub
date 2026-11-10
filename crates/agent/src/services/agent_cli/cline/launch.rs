use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;
use serde_json::json;

const PROVIDER_ID: &str = "openai-compatible";

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let model = request.model.trim();
    let native = matches!(
        request.configuration,
        TemporaryLaunchConfiguration::Native
    );
    let mut args = vec!["--tui".to_string()];
    let mut environment = EnvironmentPatch::default();
    let mut auxiliary_files = Vec::new();
    match request.configuration {
        TemporaryLaunchConfiguration::Native => {}
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            if model.is_empty() {
                return Err("Cline 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let config_dir = request.launch_directory.join("cline");
            let data_dir = config_dir.join("data");
            // Provider settings move to the launch directory, but sessions
            // stay in the native root so a resumed ID is still found and new
            // sessions remain available to the reader afterwards.
            let session_root = super::config::session_data_dir();
            for name in [
                "CLINE_DIR",
                "CLINE_DATA_DIR",
                "CLINE_SESSION_DATA_DIR",
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
            ] {
                environment.remove(name);
            }
            environment.set_path("CLINE_DIR", &config_dir);
            environment.set_path("CLINE_DATA_DIR", &data_dir);
            if let Some(root) = session_root {
                environment.set_path("CLINE_SESSION_DATA_DIR", &root);
            }
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "cline/data/settings/providers.json",
                settings: false,
                content: provider_settings(model, base_url.trim(), api_key.trim())?,
            });
            args.extend([
                "-P".into(),
                PROVIDER_ID.into(),
                "-m".into(),
                model.into(),
            ]);
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("Cline 不支持从 OAuth 账号启动".into());
        }
    }
    if native && !model.is_empty() {
        args.extend(["-m".into(), model.into()]);
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("Cline 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend(["--id".into(), resume_id.into()]);
    }
    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_files,
        isolated_home: None,
    })
}

fn provider_settings(model: &str, base_url: &str, api_key: &str) -> Result<String, String> {
    serde_json::to_string_pretty(&json!({
        "version": 1,
        "lastUsedProvider": PROVIDER_ID,
        "modes": {},
        "providers": {
            PROVIDER_ID: {
                "settings": {
                    "provider": PROVIDER_ID,
                    "apiKey": api_key,
                    "model": model,
                    "baseUrl": base_url,
                },
                "updatedAt": "2000-01-01T00:00:00.000Z",
                "tokenSource": "manual",
            }
        },
    }))
    .map_err(|err| format!("生成 Cline 临时配置失败: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TemporaryCliSessionMode;

    fn request<'a>(
        configuration: TemporaryLaunchConfiguration<'a>,
        model: &'a str,
        resume_id: &'a str,
        session_mode: TemporaryCliSessionMode,
    ) -> TemporaryLaunchRequest<'a> {
        TemporaryLaunchRequest {
            configuration,
            model,
            session_name: "",
            resume_id,
            session_mode,
            launch_directory: std::path::Path::new("<launch>"),
        }
    }

    #[test]
    fn provider_launch_isolates_the_cli_and_selects_the_model() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-cline-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();

        assert_eq!(
            plan.args,
            ["--tui", "-P", PROVIDER_ID, "-m", "relay-model"]
        );
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-cline-test")));
        let env = plan.environment.set_values().collect::<Vec<_>>();
        assert!(env
            .iter()
            .any(|(name, value)| *name == "CLINE_DIR" && crate::test_support::ends_with_path(value, "/cline")));
        assert!(env.iter().any(|(name, value)| {
            *name == "CLINE_DATA_DIR" && crate::test_support::ends_with_path(value, "/cline/data")
        }));
        let file = plan.auxiliary_files.first().unwrap();
        assert_eq!(file.path, "cline/data/settings/providers.json");
        let value: serde_json::Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(
            value["providers"][PROVIDER_ID]["settings"]["apiKey"],
            "sk-cline-test"
        );
        assert_eq!(value["providers"][PROVIDER_ID]["settings"]["model"], "relay-model");
    }

    #[test]
    fn native_launch_only_adds_the_selected_model() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();

        assert_eq!(plan.args, ["--tui", "-m", "relay-model"]);
        assert!(plan.auxiliary_files.is_empty());
        assert_eq!(plan.environment.set_values().count(), 0);
    }

    #[test]
    fn history_launch_resumes_the_exact_session() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "1794205032164_i1msi",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();

        assert_eq!(plan.args, ["--tui", "--id", "1794205032164_i1msi"]);
    }
}
