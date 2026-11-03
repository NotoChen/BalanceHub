use crate::services::agent_cli::contracts::EnvironmentPatch;
use std::path::Path;

pub(super) fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("Grok 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("GROK_HOME", root);
    Ok(environment)
}
