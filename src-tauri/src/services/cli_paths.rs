use std::{env, path::PathBuf};

const BALANCEHUB_CODEX_HOME: &str = "BALANCEHUB_CODEX_HOME";
const BALANCEHUB_CLAUDE_CONFIG_DIR: &str = "BALANCEHUB_CLAUDE_CONFIG_DIR";

/// Resolve the Codex state directory used by BalanceHub.
///
/// The BalanceHub-specific override is intentionally checked first so a
/// development instance can use a copied, isolated directory while retaining
/// the user's normal `HOME`. The official `CODEX_HOME` variable remains the
/// next fallback for normal CLI installations.
pub(crate) fn codex_home() -> Option<PathBuf> {
    configured_path(BALANCEHUB_CODEX_HOME)
        .or_else(|| configured_path("CODEX_HOME"))
        .or_else(|| user_home().map(|home| home.join(".codex")))
}

/// Resolve the Claude Code configuration directory used by BalanceHub.
pub(crate) fn claude_config_dir() -> Option<PathBuf> {
    configured_path(BALANCEHUB_CLAUDE_CONFIG_DIR)
        .or_else(|| configured_path("CLAUDE_CONFIG_DIR"))
        .or_else(|| user_home().map(|home| home.join(".claude")))
}

fn configured_path(key: &str) -> Option<PathBuf> {
    env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn user_home() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env::var_os("USERPROFILE")
            .filter(|value| !value.is_empty())
            .or_else(|| {
                let mut home = env::var_os("HOMEDRIVE")?;
                home.push(env::var_os("HOMEPATH")?);
                Some(home)
            })
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }

    #[cfg(not(target_os = "windows"))]
    {
        env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }
}
