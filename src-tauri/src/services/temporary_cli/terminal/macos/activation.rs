use super::{applescript::build_macos_ghostty_activation_applescript, process::run_command};
use crate::services::cli_runtime;
use std::process::Command;

pub(super) fn activate_ghostty(target: &cli_runtime::CliTerminalLocator) -> Result<(), String> {
    match target {
        cli_runtime::CliTerminalLocator::Ghostty { terminal_id } => {
            let script = build_macos_ghostty_activation_applescript(terminal_id);
            run_command(
                Command::new("osascript").arg("-e").arg(script),
                "无法打开对应的 Ghostty CLI 窗口，窗口可能已关闭",
            )
        }
    }
}
