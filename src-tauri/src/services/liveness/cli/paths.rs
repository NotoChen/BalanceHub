use super::{compare_version_keys, numeric_version_key};
use crate::{limits, platform::process::run_command_with_output_timeout};
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub(in crate::services::liveness) fn runtime_path_for(cli_path: &Path) -> Option<OsString> {
    runtime_path_for_mode(cli_path, true)
}

pub(in crate::services::liveness) fn runtime_path_for_without_shell(
    cli_path: &Path,
) -> Option<OsString> {
    runtime_path_for_mode(cli_path, false)
}

fn runtime_path_for_mode(cli_path: &Path, include_shell: bool) -> Option<OsString> {
    let mut dirs = Vec::new();
    if let Some(parent) = cli_path.parent() {
        dirs.push(parent.to_path_buf());
    }
    if let Some(home) = home_dir() {
        dirs.extend(runtime_home_dirs(&home));
    }
    for dir in platform_global_dirs() {
        dirs.push(PathBuf::from(dir));
    }
    if include_shell {
        if let Some(path) = login_shell_path() {
            dirs.extend(env::split_paths(&path));
        }
    }
    if let Some(path) = env::var_os("PATH") {
        dirs.extend(env::split_paths(&path));
    }

    let mut seen = Vec::new();
    let dirs = dirs
        .into_iter()
        .filter(|dir| !dir.as_os_str().is_empty())
        .filter(|dir| {
            if seen.iter().any(|item: &PathBuf| item == dir) {
                false
            } else {
                seen.push(dir.clone());
                true
            }
        })
        .collect::<Vec<_>>();
    env::join_paths(dirs).ok()
}

pub(super) fn clean_preferred_path(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let quoted = (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'');
        if quoted {
            return value[1..value.len() - 1].trim().to_string();
        }
    }
    value.to_string()
}

pub(super) fn expand_home_path(value: &str) -> PathBuf {
    if value == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(value)
}

pub(super) fn has_path_separator(value: &str) -> bool {
    value.contains('/') || value.contains('\\')
}

pub(super) fn path_candidates(binary: &str, include_shell: bool) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = env::var("PATH") {
        for dir in env::split_paths(&path) {
            candidates.extend(binary_names(binary).into_iter().map(|name| dir.join(name)));
        }
    }
    if include_shell {
        if let Some(path) = login_shell_path() {
            for dir in env::split_paths(&path) {
                candidates.extend(binary_names(binary).into_iter().map(|name| dir.join(name)));
            }
        }
    }
    candidates
}

pub(super) fn shell_command_candidates(binary: &str) -> Vec<PathBuf> {
    if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(format!("where {binary}"));
        return run_command_with_output_timeout(
            &mut command,
            Duration::from_secs(5),
            limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
        )
        .ok()
        .filter(|output| !output.timed_out && output.status.is_some_and(|status| status.success()))
        .map(|output| {
            output
                .stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default();
    }

    let mut candidates = Vec::new();
    for command in [
        format!("command -v {}", shell_escape_word(binary)),
        format!("which -a {}", shell_escape_word(binary)),
    ] {
        if let Some(output) = login_shell_output(&command) {
            candidates.extend(
                output
                    .lines()
                    .map(str::trim)
                    .filter(|line| line.starts_with('/'))
                    .map(PathBuf::from),
            );
        }
    }
    candidates
}

fn login_shell_path() -> Option<OsString> {
    if cfg!(target_os = "windows") {
        return env::var_os("PATH");
    }
    login_shell_output("printf '__BALANCEHUB_PATH__%s' \"$PATH\"")
        .and_then(|path| {
            path.find("__BALANCEHUB_PATH__").map(|index| {
                path[index + "__BALANCEHUB_PATH__".len()..]
                    .trim()
                    .to_string()
            })
        })
        .map(OsString::from)
        .filter(|path| !path.is_empty())
}

fn login_shell_output(command: &str) -> Option<String> {
    let shell = env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".to_string()
        } else {
            "/bin/sh".to_string()
        }
    });
    for mode in ["-lc", "-ic"] {
        let mut shell_command = Command::new(&shell);
        shell_command.arg(mode).arg(command);
        let Ok(output) = run_command_with_output_timeout(
            &mut shell_command,
            Duration::from_secs(5),
            limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
        ) else {
            continue;
        };
        if !output.timed_out && output.status.is_some_and(|status| status.success()) {
            let text = output.stdout;
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn shell_escape_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn normalize_path(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

pub(super) fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub(super) fn binary_names(binary: &str) -> Vec<String> {
    let mut names = vec![binary.to_string()];
    if cfg!(target_os = "windows") {
        names.push(format!("{binary}.cmd"));
        names.push(format!("{binary}.exe"));
    }
    names
}

fn platform_global_dirs() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"]
    } else if cfg!(target_os = "windows") {
        &[]
    } else {
        &["/usr/local/bin", "/usr/bin", "/bin"]
    }
}

pub(super) fn home_bin_candidates(home: &Path, binary: &str) -> Vec<PathBuf> {
    runtime_home_dirs(home)
        .into_iter()
        .flat_map(|dir| {
            binary_names(binary)
                .into_iter()
                .map(move |name| dir.join(name))
        })
        .collect()
}

fn runtime_home_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![
        home.join(".local/bin"),
        home.join(".npm-global/bin"),
        home.join("n/bin"),
        home.join(".volta/bin"),
        home.join(".asdf/shims"),
        home.join(".local/share/mise/shims"),
        home.join(".bun/bin"),
        home.join("Library/pnpm"),
        home.join(".local/share/pnpm"),
    ];
    dirs.extend(node_manager_bin_dirs(home));
    dirs.extend(fnm_multishell_dirs(home));
    dirs
}

pub(super) fn node_manager_bin_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = versioned_bin_dirs(&home.join(".nvm/versions/node"), "bin");
    dirs.extend(versioned_bin_dirs(
        &home.join(".fnm/node-versions"),
        "installation/bin",
    ));
    dirs.extend(versioned_bin_dirs(
        &home.join(".local/share/fnm/node-versions"),
        "installation/bin",
    ));
    dirs
}

fn versioned_bin_dirs(base: &Path, suffix: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(base) else {
        return Vec::new();
    };
    let mut versions = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            (numeric_version_key(&name), entry.path().join(suffix))
        })
        .collect::<Vec<_>>();
    versions.sort_by(|left, right| compare_version_keys(&right.0, &left.0));
    versions.into_iter().map(|(_, path)| path).collect()
}

fn fnm_multishell_dirs(home: &Path) -> Vec<PathBuf> {
    let base = home.join(".local/state/fnm_multishells");
    let Ok(entries) = fs::read_dir(base) else {
        return Vec::new();
    };

    entries
        .flatten()
        .map(|entry| entry.path().join("bin"))
        .collect()
}

pub(super) fn windows_npm_candidates(binary: &str) -> Vec<PathBuf> {
    if !cfg!(target_os = "windows") {
        return Vec::new();
    }

    ["APPDATA", "LOCALAPPDATA"]
        .iter()
        .filter_map(env::var_os)
        .flat_map(|base| {
            let npm_dir = PathBuf::from(base).join("npm");
            binary_names(binary)
                .into_iter()
                .map(move |name| npm_dir.join(name))
        })
        .collect()
}
