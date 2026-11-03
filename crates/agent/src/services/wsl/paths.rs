//! Linux names and Windows filesystem locators are different identities.
use std::path::{Path, PathBuf};

pub(super) fn valid_distro(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with(['-', '.'])
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '"'))
}

pub(crate) fn guest_path(path: &str) -> Result<&str, String> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path.split('/').any(|part| part == "..")
    {
        return Err("WSL 返回了无效的 Linux 路径".into());
    }
    Ok(path)
}

pub(crate) fn host_path(distro: &str, path: &str) -> Result<PathBuf, String> {
    if !valid_distro(distro) {
        return Err("WSL 发行版标识无效".into());
    }
    let path = guest_path(path)?;
    if path
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return Err("此 Linux 路径包含 Windows 文件接口无法访问的字符".into());
    }
    Ok(PathBuf::from(format!(
        r"\\wsl.localhost\{}\{}",
        distro,
        path.trim_start_matches('/').replace('/', "\\")
    )))
}

pub(crate) fn split_host_path(path: &Path) -> Option<(String, String)> {
    let normalized = crate::platform::paths::compatible_path(path)
        .to_string_lossy()
        .replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    let prefix = ["//wsl.localhost/", "//wsl$/"]
        .into_iter()
        .find(|prefix| lower.starts_with(prefix))?;
    let (name, rest) = normalized[prefix.len()..]
        .split_once('/')
        .unwrap_or((&normalized[prefix.len()..], ""));
    if !valid_distro(name) {
        return None;
    }
    let guest = format!("/{rest}");
    guest_path(&guest).ok()?;
    Some((name.to_owned(), guest))
}

/// A path recorded inside a guest is resolved against that guest, never C:\.
pub(crate) fn recorded_path(root: &Path, recorded: &str) -> Result<PathBuf, String> {
    if let Some((name, _)) = split_host_path(root) {
        host_path(&name, recorded)
    } else {
        Ok(PathBuf::from(recorded))
    }
}
