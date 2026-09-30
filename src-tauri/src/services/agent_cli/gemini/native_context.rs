use crate::services::agent_cli::contracts::EnvironmentPatch;
use std::path::Path;

pub(super) fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() || root.file_name().is_none_or(|name| name != ".gemini") {
        return Err("当前 Gemini 数据根不能由原生 GEMINI_CLI_HOME 精确复现，可查看但不能启动");
    }
    let home = root.parent().ok_or("无法定位 Gemini 原生主目录")?;
    let mut environment = EnvironmentPatch::default();
    environment.set("GEMINI_CLI_HOME", home.to_string_lossy());
    Ok(environment)
}
