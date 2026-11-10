use super::super::contracts::{
    EnvironmentPatch, TemporaryAuxiliaryFile, TemporaryLaunchConfiguration, TemporaryLaunchPlan,
    TemporaryLaunchRequest,
};
use super::config;
use crate::{
    models::TemporaryCliSessionMode,
    services::cli_paths::user_home,
};

const PROVIDER_ID: &str = "balancehub";

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    let mut args = Vec::new();
    let mut environment = EnvironmentPatch::default();
    let mut auxiliary_files = Vec::new();

    let isolated_home = match request.configuration {
        TemporaryLaunchConfiguration::Native => {
            if !request.model.trim().is_empty() {
                crate::services::agent_cli::integration::validate_native_value(
                    request.model,
                    "模型 ID",
                )?;
                args.extend(["-m".into(), request.model.trim().into()]);
            }
            // One-shot native launches still need an isolated `~/.commandcode`,
            // but the retained-session path keeps the native root untouched.
            Some("run-home/.commandcode")
        }
        TemporaryLaunchConfiguration::Provider {
            provider_name,
            api_key,
            base_url,
        } => {
            let model = request.model.trim();
            if model.is_empty() {
                return Err("Command Code 需要先填写中转站的首选模型".into());
            }
            if matches!(request.session_mode, TemporaryCliSessionMode::History) {
                return Err("Command Code 的中转站启动暂不支持继续原生会话，请使用原生启动续聊".into());
            }
            crate::services::agent_cli::integration::validate_native_value(api_key, "API Key")?;
            crate::services::agent_cli::integration::validate_native_value(
                base_url,
                "中转站地址",
            )?;
            crate::services::agent_cli::integration::validate_native_value(model, "模型 ID")?;

            let root = config::config_root().ok_or_else(|| "无法定位用户目录".to_string())?;
            let native_config = config::optional_config_text(&root, "config.json");
            let documents = config::provider_documents(
                model,
                provider_name,
                base_url.trim(),
                api_key.trim(),
                None,
                None,
                native_config.as_deref(),
            )?;
            let home = request.launch_directory.join("commandcode-home");
            for name in ["HOME", "USERPROFILE", "COMMAND_CODE_API_KEY"] {
                environment.remove(name);
            }
            environment.set_path("HOME", &home);
            environment.set_path("USERPROFILE", &home);
            if let Some(git_config) = user_home().map(|home| home.join(".gitconfig")) {
                if git_config.is_file() {
                    environment.set_path("GIT_CONFIG_GLOBAL", &git_config);
                }
            }
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "commandcode-home/.commandcode/providers.json",
                settings: false,
                content: documents.providers,
            });
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "commandcode-home/.commandcode/credentials.json",
                settings: false,
                content: documents.credentials,
            });
            auxiliary_files.push(TemporaryAuxiliaryFile {
                path: "commandcode-home/.commandcode/config.json",
                settings: false,
                content: documents.user_config,
            });
            // MCP declarations are independent of the provider choice; copy
            // them so the temporary home behaves like the native one.
            if let Some(mcp) = config::optional_config_text(&root, "mcp.json") {
                auxiliary_files.push(TemporaryAuxiliaryFile {
                    path: "commandcode-home/.commandcode/mcp.json",
                    settings: false,
                    content: mcp,
                });
            }
            args.extend(["-m".into(), format!("{PROVIDER_ID}/{model}")]);
            Some("commandcode-home/.commandcode")
        }
        TemporaryLaunchConfiguration::OAuth { .. } => {
            return Err("Command Code 不支持从 OAuth 账号启动".into());
        }
    };

    if !matches!(request.session_mode, TemporaryCliSessionMode::History)
        && !request.session_name.trim().is_empty()
    {
        crate::services::agent_cli::integration::validate_native_value(
            request.session_name,
            "会话名称",
        )?;
        args.extend(["-n".into(), request.session_name.trim().into()]);
    }
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("Command Code 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend(["--resume".into(), resume_id.into()]);
    }

    Ok(TemporaryLaunchPlan {
        args,
        environment,
        auxiliary_files,
        isolated_home,
    })
}
