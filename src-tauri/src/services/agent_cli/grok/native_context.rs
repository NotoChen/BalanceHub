use crate::services::agent_cli::contracts::EnvironmentPatch;
use std::path::Path;

pub(super) fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() {
        return Err("Grok 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set("GROK_HOME", root.to_string_lossy());
    Ok(environment)
}
