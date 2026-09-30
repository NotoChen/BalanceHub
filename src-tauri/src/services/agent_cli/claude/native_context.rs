use crate::services::agent_cli::contracts::EnvironmentPatch;
use std::path::Path;

pub(super) fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() {
        return Err("Claude 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set("CLAUDE_CONFIG_DIR", root.to_string_lossy());
    Ok(environment)
}
