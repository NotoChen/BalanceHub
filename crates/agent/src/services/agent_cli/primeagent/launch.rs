use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use super::config;
use crate::models::TemporaryCliSessionMode;
use serde_json::json;

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
                return Err("Prime Agent 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let agent_dir = request.launch_directory.join("agent");
            environment.remove("PRIME_AGENT_CODING_AGENT_DIR");
            environment.set_path("PRIME_AGENT_CODING_AGENT_DIR", &agent_dir);
            // models.json 只引用变量名，密钥不落到启动目录。
            environment.set("BALANCEHUB_PRIME_API_KEY", api_key.trim());
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "agent/models.json",
                settings: true,
                content: temporary_models(model, base_url.trim())?,
            });
            args.extend([
                "--provider".into(),
                PROVIDER_ID.into(),
                "--model".into(),
                model.into(),
            ]);
            // 独立 Agent 目录默认把会话放进启动目录；显式指回原生位置，
            // 退出后仍能从 Prime Agent 原生的会话选择器继续。
            if let Some(directory) = config::native_session_dir() {
                args.extend(["--session-dir".into(), directory]);
            }
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("Prime Agent 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("Prime Agent 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend(["--resume".into(), resume_id.into()]);
    }
    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_files,
        isolated_home: None,
    })
}

fn temporary_models(model: &str, base_url: &str) -> Result<String, String> {
    serde_json::to_string_pretty(&json!({
        "providers": {
            PROVIDER_ID: {
                "name": "BalanceHub",
                "baseUrl": base_url,
                "api": "openai-completions",
                "apiKey": "${BALANCEHUB_PRIME_API_KEY}",
                "models": [{ "id": model, "name": model }],
            },
        },
    }))
    .map_err(|err| format!("生成 Prime Agent 临时模型配置失败: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
            launch_directory: Path::new("/tmp/<launch>"),
        }
    }

    #[test]
    fn provider_launch_writes_models_without_the_key() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-prime-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert!(plan
            .args
            .starts_with(&["--provider".into(), "balancehub".into()]));
        assert!(!plan.args.iter().any(|value| value.contains("sk-prime-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "BALANCEHUB_PRIME_API_KEY" && value == "sk-prime-test"));
        let models = &plan.auxiliary_files[0];
        assert_eq!(models.path, "agent/models.json");
        assert!(!models.content.contains("sk-prime-test"));
        assert!(models.content.contains("${BALANCEHUB_PRIME_API_KEY}"));
    }

    #[test]
    fn native_launch_passes_the_model_without_writing_config() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "native-model",
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args, ["--model", "native-model"]);
        assert!(plan.auxiliary_files.is_empty());
    }

    #[test]
    fn history_launch_resumes_the_native_session() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-prime-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
            "session-1",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert!(plan
            .args
            .windows(2)
            .any(|pair| pair == ["--resume", "session-1"]));
    }
}
