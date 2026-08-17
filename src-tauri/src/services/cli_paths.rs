use std::{env, path::PathBuf};

pub(crate) fn configured_path(key: &str) -> Option<PathBuf> {
    env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn user_home() -> Option<PathBuf> {
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
