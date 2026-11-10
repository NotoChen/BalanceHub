use super::super::{
    config_support::yaml_source,
    contracts::{
        EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration,
        TemporaryLaunchPlan, TemporaryLaunchRequest,
    },
};
use crate::models::TemporaryCliSessionMode;

const INFERENCE_PROVIDER: &str = "openai_response_compatible";
const USER_AGENT: &str = "mistermorph";

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = vec!["chat".to_string()];
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
                return Err("Mister Morph 需要先填写中转站的首选模型".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;
            let config_path = request.launch_directory.join("config.yaml");
            environment.remove("MISTER_MORPH_CONFIG");
            environment.set_path("MISTER_MORPH_CONFIG", &config_path);
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "config.yaml",
                settings: false,
                content: temporary_config(model, base_url.trim(), api_key.trim())?,
            });
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("Mister Morph 不支持从 OAuth 账号启动".into());
        }
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        return Err("Mister Morph 暂不支持临时启动续聊".into());
    }
    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_files,
        isolated_home: None,
    })
}

fn temporary_config(model: &str, base_url: &str, api_key: &str) -> Result<String, String> {
    let endpoint = super::super::config_support::openai_base_url(base_url);
    yaml_source::edit("", "Mister Morph 临时配置", |root| {
        let llm = yaml_source::mapping_member(root, "llm", "Mister Morph 临时配置")?;
        llm.set("inference_provider", INFERENCE_PROVIDER);
        llm.set("endpoint", endpoint.as_str());
        llm.set("api_key", api_key);
        llm.set("model", model);
        let headers = yaml_source::mapping_member(&llm, "headers", "Mister Morph 临时配置")?;
        headers.set("User-Agent", USER_AGENT);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>(
        configuration: TemporaryLaunchConfiguration<'a>,
        model: &'a str,
    ) -> TemporaryLaunchRequest<'a> {
        TemporaryLaunchRequest {
            configuration,
            model,
            session_name: "",
            resume_id: "",
            session_mode: TemporaryCliSessionMode::New,
            launch_directory: Path::new("/tmp/<launch>"),
        }
    }

    #[test]
    fn provider_launch_uses_an_isolated_config_file() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-morph-test",
                base_url: "https://relay.example.com/v1",
            },
            "relay-model",
        ))
        .unwrap();
        assert_eq!(plan.args, ["chat"]);
        assert!(!plan.args.iter().any(|value| value.contains("sk-morph-test")));
        assert!(plan
            .environment
            .set_values()
            .any(|(name, value)| name == "MISTER_MORPH_CONFIG"
                && value == "/tmp/<launch>/config.yaml"));
        let file = &plan.auxiliary_files[0];
        assert_eq!(file.path, "config.yaml");
        assert!(!file.settings);
        let parsed = yaml_source::value(&file.content).unwrap();
        assert_eq!(
            parsed["llm"]["inference_provider"],
            "openai_response_compatible"
        );
        assert_eq!(parsed["llm"]["endpoint"], "https://relay.example.com/v1");
        assert_eq!(parsed["llm"]["api_key"], "sk-morph-test");
        assert_eq!(parsed["llm"]["model"], "relay-model");
    }

    #[test]
    fn native_launch_passes_the_model_without_writing_config() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "native-model",
        ))
        .unwrap();
        assert_eq!(plan.args, ["chat", "--model", "native-model"]);
        assert!(plan.auxiliary_files.is_empty());
    }

    #[test]
    fn history_launch_is_refused() {
        let mut request = request(
            TemporaryLaunchConfiguration::Native,
            "native-model",
        );
        request.session_mode = TemporaryCliSessionMode::History;
        assert_eq!(
            build_plan(request).unwrap_err(),
            "Mister Morph 暂不支持临时启动续聊"
        );
    }
}
