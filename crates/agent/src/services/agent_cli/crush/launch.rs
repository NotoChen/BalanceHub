use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;
use serde_json::json;

const PROVIDER_ID: &str = "balancehub";

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let model = request.model.trim();
    let mut args = Vec::new();
    let mut environment = EnvironmentPatch::default();
    let mut auxiliary_files = Vec::new();
    match request.configuration {
        TemporaryLaunchConfiguration::Native => {
            if !model.is_empty() {
                let (provider, model_id) = native_selection(model)?;
                let data_root = request.launch_directory.join("crush-data");
                environment.set_path("XDG_DATA_HOME", &data_root);
                for name in ["CRUSH_GLOBAL_DATA", "CRUSH_DATA_DIR"] {
                    environment.remove(name);
                }
                auxiliary_files.push(TemporaryAuxiliaryFile {
                    path: "crush-data/crush/crush.json",
                    settings: false,
                    content: selection_config(&provider, &model_id)?,
                });
            }
        }
        TemporaryLaunchConfiguration::Provider {
            api_key, base_url, ..
        } => {
            if model.is_empty() {
                return Err("Crush 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let config_root = request.launch_directory.join("crush-config");
            let data_root = request.launch_directory.join("crush-data");
            environment.set_path("XDG_CONFIG_HOME", &config_root);
            environment.set_path("XDG_DATA_HOME", &data_root);
            for name in [
                "CRUSH_GLOBAL_DATA",
                "CRUSH_CONFIG",
                "CRUSH_DATA_DIR",
                "CRUSH_SESSION_DATA_DIR",
            ] {
                environment.remove(name);
            }
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "crush-config/crush/crush.json",
                settings: false,
                content: provider_config(model, base_url.trim(), api_key.trim())?,
            });
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("Crush 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("Crush 续聊缺少原生会话 ID".into());
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

fn selection_config(provider: &str, model: &str) -> Result<String, String> {
    serde_json::to_string_pretty(&json!({
        "models": {
            "large": { "provider": provider, "model": model },
            "small": { "provider": provider, "model": model },
        },
    }))
    .map_err(|err| format!("生成 Crush 临时配置失败: {err}"))
}

fn provider_config(model: &str, base_url: &str, api_key: &str) -> Result<String, String> {
    serde_json::to_string_pretty(&json!({
        "$schema": "https://charm.land/crush.json",
        "providers": {
            PROVIDER_ID: {
                "type": "completions",
                "name": "BalanceHub",
                "base_url": base_url,
                "api_key": api_key,
                "models": [{ "id": model, "name": model }],
            }
        },
        "models": {
            "large": { "provider": PROVIDER_ID, "model": model },
            "small": { "provider": PROVIDER_ID, "model": model },
        },
    }))
    .map_err(|err| format!("生成 Crush 临时配置失败: {err}"))
}

/// `crush` has no command-line model flag outside `run`, so a native launch
/// resolves the selected model to its provider and writes the native pick file
/// in an isolated data directory.
fn native_selection(model: &str) -> Result<(String, String), String> {
    if let Some((provider, name)) = model.split_once('/') {
        if !provider.trim().is_empty() && !name.trim().is_empty() {
            return Ok((provider.trim().to_owned(), name.trim().to_owned()));
        }
    }
    let home = crate::services::cli_paths::user_home()
        .ok_or_else(|| "无法定位用户目录".to_string())?;
    let path = super::config::config_path_for_home(&home);
    let source = std::fs::read_to_string(&path)
        .map_err(|_| "读取 Crush 原生配置失败，无法确定所选模型的 Provider".to_string())?;
    let value = crate::services::agent_cli::config_support::parse_jsonc_document(&source)?;
    let providers = value
        .get("providers")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "Crush 原生配置没有 Provider，无法确定所选模型".to_string())?;
    let mut owners = providers
        .iter()
        .filter(|(_, entry)| {
            entry
                .get("models")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|models| {
                    models.iter().any(|entry| {
                        entry.get("id").and_then(serde_json::Value::as_str) == Some(model)
                    })
                })
        })
        .map(|(id, _)| id.clone());
    let provider = owners
        .next()
        .ok_or_else(|| "未在 Crush 原生配置中找到所选模型".to_string())?;
    if owners.next().is_some() {
        return Err("多个 Crush Provider 提供同名模型，请选择 Provider/模型 形式".to_string());
    }
    Ok((provider, model.to_owned()))
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
    fn provider_launch_writes_an_isolated_completions_config() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-crush-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();

        assert!(plan.args.is_empty());
        assert!(!plan.args.iter().any(|arg| arg.contains("sk-crush-test")));
        let env = plan.environment.set_values().collect::<Vec<_>>();
        assert!(env
            .iter()
            .any(|(name, value)| *name == "XDG_CONFIG_HOME" && value.ends_with("crush-config")));
        assert!(env
            .iter()
            .any(|(name, value)| *name == "XDG_DATA_HOME" && value.ends_with("crush-data")));
        let file = plan.auxiliary_files.first().unwrap();
        assert_eq!(file.path, "crush-config/crush/crush.json");
        let value: serde_json::Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(value["providers"][PROVIDER_ID]["type"], "completions");
        assert_eq!(value["providers"][PROVIDER_ID]["api_key"], "sk-crush-test");
        assert_eq!(value["models"]["large"]["model"], "relay-model");
    }

    #[test]
    fn native_launch_selects_an_explicit_provider_model() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "relay/model-a",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();

        assert!(plan.args.is_empty());
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "XDG_DATA_HOME" && value.ends_with("crush-data")));
        let file = plan.auxiliary_files.first().unwrap();
        assert_eq!(file.path, "crush-data/crush/crush.json");
        let value: serde_json::Value = serde_json::from_str(&file.content).unwrap();
        assert_eq!(value["models"]["large"]["provider"], "relay");
        assert_eq!(value["models"]["large"]["model"], "model-a");
    }

    #[test]
    fn native_launch_without_a_model_keeps_the_native_configuration() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();

        assert!(plan.args.is_empty());
        assert!(plan.auxiliary_files.is_empty());
        assert_eq!(plan.environment.set_values().count(), 0);
    }

    #[test]
    fn history_launch_resumes_the_exact_session() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            "0f3f6c0e-session",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();

        assert_eq!(plan.args, ["--session", "0f3f6c0e-session"]);
    }
}
