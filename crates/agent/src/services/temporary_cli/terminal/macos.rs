mod activation;
mod applescript;
mod launch;
mod probe;
mod process;

use super::TerminalDefinition;
use crate::models::TemporaryCliTerminalKind;
use activation::activate_ghostty;
use launch::{
    launch_alacritty, launch_ghostty, launch_iterm2, launch_kaku, launch_kitty, launch_terminal,
    launch_warp, launch_wezterm,
};
use probe::{
    probe_alacritty, probe_ghostty, probe_iterm2, probe_kaku, probe_kitty, probe_terminal,
    probe_warp, probe_wezterm,
};

#[cfg(test)]
pub(crate) use applescript::{
    build_macos_ghostty_activation_applescript, build_macos_ghostty_applescript,
    build_macos_iterm2_applescript, build_macos_terminal_applescript,
};
#[cfg(test)]
pub(crate) use launch::warp_launcher_script_path;

const DEFINITIONS: &[TerminalDefinition] = &[
    TerminalDefinition::new(
        TemporaryCliTerminalKind::Warp,
        probe_warp,
        launch_warp,
        None,
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::ITerm2,
        probe_iterm2,
        launch_iterm2,
        None,
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::WezTerm,
        probe_wezterm,
        launch_wezterm,
        None,
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::Kaku,
        probe_kaku,
        launch_kaku,
        None,
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::Ghostty,
        probe_ghostty,
        launch_ghostty,
        Some(activate_ghostty),
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::Terminal,
        probe_terminal,
        launch_terminal,
        None,
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::Kitty,
        probe_kitty,
        launch_kitty,
        None,
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::Alacritty,
        probe_alacritty,
        launch_alacritty,
        None,
    ),
];

pub(super) const fn definitions() -> &'static [TerminalDefinition] {
    DEFINITIONS
}
