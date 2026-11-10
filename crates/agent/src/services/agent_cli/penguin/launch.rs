use super::super::contracts::{
    EnvironmentPatch, TemporaryLaunchConfiguration, TemporaryLaunchPlan, TemporaryLaunchRequest,
};
use crate::models::TemporaryCliSessionMode;

pub(super) fn build_plan(
    request: TemporaryLaunchRequest<'_>,
) -> Result<TemporaryLaunchPlan, String> {
    if !matches!(request.configuration, TemporaryLaunchConfiguration::Native) {
        return Err(
            "PenguinHarness 的中转站模型必须写入原生项目配置，暂不支持临时切换；请先应用中转站配置"
                .into(),
        );
    }
    let mut args = vec!["chat".to_string()];
    if matches!(request.session_mode, TemporaryCliSessionMode::History) {
        let resume_id = request.resume_id.trim();
        if resume_id.is_empty() {
            return Err("PenguinHarness 续聊缺少原生会话 ID".into());
        }
        crate::services::agent_cli::integration::validate_native_value(resume_id, "会话 ID")?;
        args.extend(["--resume".into(), resume_id.into()]);
    }
    Ok(TemporaryLaunchPlan {
        args,
        environment: EnvironmentPatch::default(),
        auxiliary_files: Vec::new(),
        isolated_home: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>(
        configuration: TemporaryLaunchConfiguration<'a>,
        resume_id: &'a str,
        session_mode: TemporaryCliSessionMode,
    ) -> TemporaryLaunchRequest<'a> {
        TemporaryLaunchRequest {
            configuration,
            model: "",
            session_name: "",
            resume_id,
            session_mode,
            launch_directory: Path::new("/tmp/<launch>"),
        }
    }

    #[test]
    fn native_launch_opens_the_repl_and_resumes_by_native_id() {
        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap();
        assert_eq!(plan.args, ["chat"]);

        let plan = build_plan(request(
            TemporaryLaunchConfiguration::Native,
            "session-1",
            TemporaryCliSessionMode::History,
        ))
        .unwrap();
        assert_eq!(plan.args, ["chat", "--resume", "session-1"]);
    }

    #[test]
    fn provider_launch_requires_applying_the_native_project_config() {
        let error = build_plan(request(
            TemporaryLaunchConfiguration::Provider {
                provider_name: "Relay",
                api_key: "sk-penguin-test",
                base_url: "https://relay.example.com/v1",
            },
            "",
            TemporaryCliSessionMode::New,
        ))
        .unwrap_err();
        assert!(error.contains("原生项目配置"));
    }
}
