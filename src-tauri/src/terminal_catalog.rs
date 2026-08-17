//! Single compile-time catalog for built-in terminal identities.
//!
//! Platform-specific probing and launching live under `services::temporary_cli::terminal`.
//! This catalog only owns the stable enum identity, serialized key and fallback label so the
//! Rust model and terminal registry cannot silently drift apart.

macro_rules! for_each_terminal {
    ($consumer:ident) => {
        $consumer! {
            Terminal => { key: "terminal", label: "系统终端" },
            ITerm2 => { key: "iTerm2", label: "iTerm2" },
            Warp => { key: "warp", label: "Warp" },
            WezTerm => { key: "wezTerm", label: "WezTerm" },
            Ghostty => { key: "ghostty", label: "Ghostty" },
            Kitty => { key: "kitty", label: "Kitty" },
            Alacritty => { key: "alacritty", label: "Alacritty" },
            Kaku => { key: "kaku", label: "Kaku" },
            WindowsTerminal => { key: "windowsTerminal", label: "Windows Terminal" },
            CommandPrompt => { key: "commandPrompt", label: "命令提示符" },
            PowerShell => { key: "powerShell", label: "PowerShell" },
        }
    };
}

pub(crate) use for_each_terminal;
