use crate::models::{CliCandidate, CodexCliProbeResult};

use super::process::cli_version;
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

struct CliSpec {
    env_keys: &'static [&'static str],
    binary: &'static str,
    global_dirs: &'static [&'static str],
    home_candidates: fn(&Path) -> Vec<PathBuf>,
    require_version_substring: Option<&'static str>,
    not_found_message: &'static str,
}

const CODEX_SPEC: CliSpec = CliSpec {
    env_keys: &["CODEX_CLI_PATH"],
    binary: "codex",
    global_dirs: &["/opt/homebrew/bin", "/usr/local/bin"],
    home_candidates: codex_home_candidates,
    require_version_substring: None,
    not_found_message: "未找到可用的 codex CLI，请在设置中指定 codex 可执行文件路径",
};

const CLAUDE_SPEC: CliSpec = CliSpec {
    env_keys: &["CLAUDE_CODE_CLI_PATH", "CLAUDE_CLI_PATH"],
    binary: "claude",
    global_dirs: &["/opt/homebrew/bin", "/usr/local/bin"],
    home_candidates: claude_home_candidates,
    require_version_substring: Some("claude"),
    not_found_message: "未找到可用的 Claude Code CLI，请在设置中指定 claude 可执行文件路径",
};

pub(super) fn find_codex_cli(preferred_path: &str) -> Result<CodexCliProbeResult, String> {
    find_cli(preferred_path, &CODEX_SPEC)
}

pub(super) fn find_claude_cli(preferred_path: &str) -> Result<CodexCliProbeResult, String> {
    find_cli(preferred_path, &CLAUDE_SPEC)
}

fn codex_home_candidates(home: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(versions) = fs::read_dir(home.join(".nvm/versions/node")) {
        for version in versions.flatten() {
            candidates.push(version.path().join("bin/codex"));
        }
    }
    if let Ok(versions) = fs::read_dir(home.join(".fnm/node-versions")) {
        for version in versions.flatten() {
            candidates.push(version.path().join("installation/bin/codex"));
        }
    }
    if let Ok(versions) = fs::read_dir(home.join(".local/share/fnm/node-versions")) {
        for version in versions.flatten() {
            candidates.push(version.path().join("installation/bin/codex"));
        }
    }
    candidates.push(home.join(".codex/bin/codex"));
    candidates.extend(home_bin_candidates(home, "codex"));
    candidates.extend(windows_npm_candidates("codex"));
    candidates
}

fn claude_home_candidates(home: &Path) -> Vec<PathBuf> {
    let mut candidates = home_bin_candidates(home, "claude");
    candidates.push(home.join(".claude/local/claude"));
    candidates.extend(windows_npm_candidates("claude"));
    candidates
}

/// 按优先级构建 CLI 候选路径：preferred → 环境变量 → 各 CLI 专属路径 → 常见安装目录 → PATH → shell。
fn cli_candidates(preferred_path: &str, spec: &CliSpec) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let preferred_path = clean_preferred_path(preferred_path);
    if !preferred_path.is_empty() {
        let preferred = expand_home_path(&preferred_path);
        candidates.push(preferred.clone());
        if !has_path_separator(&preferred_path) {
            candidates.extend(path_candidates(&preferred_path));
        }
    }
    for key in spec.env_keys {
        if let Ok(path) = env::var(key) {
            let path = clean_preferred_path(&path);
            if !path.is_empty() {
                candidates.push(expand_home_path(&path));
            }
        }
    }
    if let Some(home) = home_dir() {
        candidates.extend((spec.home_candidates)(&home));
    }
    for dir in spec.global_dirs {
        candidates.extend(
            binary_names(spec.binary)
                .into_iter()
                .map(|name| PathBuf::from(dir).join(name)),
        );
    }
    if let Ok(path) = env::var("PATH") {
        for dir in env::split_paths(&path) {
            candidates.extend(
                binary_names(spec.binary)
                    .into_iter()
                    .map(|name| dir.join(name)),
            );
        }
    }
    candidates.extend(shell_command_candidates(spec.binary));
    candidates
}

/// 按优先级查找首个可用 CLI。
fn find_cli(preferred_path: &str, spec: &CliSpec) -> Result<CodexCliProbeResult, String> {
    let preferred_path = clean_preferred_path(preferred_path);
    if !preferred_path.is_empty() && has_path_separator(&preferred_path) {
        let preferred = normalize_path(expand_home_path(&preferred_path));
        if is_unsupported_cli_path(&preferred, spec) {
            return Err(unsupported_cli_path_message(spec).to_string());
        }
    }

    let mut seen = Vec::new();
    for candidate in cli_candidates(&preferred_path, spec) {
        if seen.iter().any(|item: &PathBuf| item == &candidate) {
            continue;
        }
        seen.push(candidate.clone());
        if is_unsupported_cli_path(&normalize_path(candidate.clone()), spec) {
            continue;
        }
        if !candidate.is_file() {
            continue;
        }
        if let Ok(version) = cli_version(&candidate, spec.require_version_substring) {
            return Ok(CodexCliProbeResult {
                path: candidate.to_string_lossy().to_string(),
                version,
            });
        }
    }

    Err(spec.not_found_message.to_string())
}

/// 枚举所有存在的候选可执行文件，标注版本/有效性/来源（不止首个）。
fn enumerate_cli(preferred_path: &str, spec: &CliSpec) -> Vec<CliCandidate> {
    let mut seen = Vec::new();
    let mut result = Vec::new();
    let path_default = resolve_path_default(spec.binary);
    for candidate in cli_candidates(preferred_path, spec) {
        if seen.iter().any(|item: &PathBuf| item == &candidate) {
            continue;
        }
        seen.push(candidate.clone());
        let normalized_candidate = normalize_path(candidate.clone());
        if is_unsupported_cli_path(&normalized_candidate, spec) {
            continue;
        }
        if !candidate.is_file() {
            continue;
        }
        let version = cli_version(&candidate, spec.require_version_substring).ok();
        let path = candidate.to_string_lossy().to_string();
        let source = infer_cli_source(&path);
        let is_path_default = path_default.as_ref() == Some(&normalized_candidate);
        result.push(CliCandidate {
            valid: version.is_some(),
            version,
            source,
            path,
            is_path_default,
        });
    }
    result.sort_by_key(|candidate| std::cmp::Reverse(candidate.is_path_default));
    result
}

pub(super) fn enumerate_codex_cli(preferred_path: &str) -> Vec<CliCandidate> {
    enumerate_cli(preferred_path, &CODEX_SPEC)
}

pub(super) fn enumerate_claude_cli(preferred_path: &str) -> Vec<CliCandidate> {
    enumerate_cli(preferred_path, &CLAUDE_SPEC)
}

/// 从可执行文件路径粗略推断来源（node 版本管理器 / 包管理器 / 系统目录），用于 UI 标注。
fn infer_cli_source(path: &str) -> String {
    let lower = path.to_lowercase();
    let has = |needle: &str| lower.contains(needle);
    if has("/.nvm/") {
        "nvm".to_string()
    } else if has("/.fnm/") || has("/fnm/") {
        "fnm".to_string()
    } else if has("/.volta/") {
        "Volta".to_string()
    } else if has("/.asdf/") {
        "asdf".to_string()
    } else if has("/mise/") {
        "mise".to_string()
    } else if has("/.bun/") || has("/bun/") {
        "bun".to_string()
    } else if has("/opt/homebrew/") || has("/homebrew/") {
        "Homebrew".to_string()
    } else if has("pnpm") {
        "pnpm".to_string()
    } else if has("\\npm\\") || has("/npm/") {
        "npm".to_string()
    } else if has("/.local/") {
        "~/.local".to_string()
    } else if has("/usr/local/bin") {
        "/usr/local/bin".to_string()
    } else if has("/usr/bin") || lower.starts_with("/bin/") {
        "系统".to_string()
    } else {
        Path::new(path)
            .parent()
            .and_then(|parent| parent.to_str())
            .unwrap_or("其他")
            .to_string()
    }
}

fn is_unsupported_cli_path(path: &Path, spec: &CliSpec) -> bool {
    spec.binary == "codex" && is_macos_app_bundle_path(path)
}

fn unsupported_cli_path_message(spec: &CliSpec) -> &'static str {
    if spec.binary == "codex" {
        "不支持使用 Codex Desktop App 内置二进制作为测活 CLI，请安装并选择独立的 codex CLI"
    } else {
        "不支持该 CLI 路径"
    }
}

fn is_macos_app_bundle_path(path: &Path) -> bool {
    let value = path.to_string_lossy().replace('\\', "/");
    value.contains(".app/Contents/")
}

pub(super) fn runtime_path_for(cli_path: &Path) -> Option<OsString> {
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
    if let Some(path) = login_shell_path() {
        dirs.extend(env::split_paths(&path));
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

fn clean_preferred_path(value: &str) -> String {
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

fn expand_home_path(value: &str) -> PathBuf {
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

fn has_path_separator(value: &str) -> bool {
    value.contains('/') || value.contains('\\')
}

fn path_candidates(binary: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = env::var("PATH") {
        for dir in env::split_paths(&path) {
            candidates.extend(binary_names(binary).into_iter().map(|name| dir.join(name)));
        }
    }
    if let Some(path) = login_shell_path() {
        for dir in env::split_paths(&path) {
            candidates.extend(binary_names(binary).into_iter().map(|name| dir.join(name)));
        }
    }
    candidates
}

fn shell_command_candidates(binary: &str) -> Vec<PathBuf> {
    if cfg!(target_os = "windows") {
        return Command::new("cmd")
            .arg("/C")
            .arg(format!("where {binary}"))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
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

fn resolve_path_default(binary: &str) -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        return Command::new("cmd")
            .arg("/C")
            .arg(format!("where {binary}"))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .map(PathBuf::from)
            })
            .map(normalize_path);
    }

    login_shell_output(&format!("command -v {}", shell_escape_word(binary)))
        .and_then(|output| {
            output
                .lines()
                .map(str::trim)
                .find(|line| line.starts_with('/'))
                .map(PathBuf::from)
        })
        .map(normalize_path)
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
        let output = Command::new(&shell)
            .arg(mode)
            .arg(command)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).to_string();
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

fn normalize_path(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn binary_names(binary: &str) -> Vec<String> {
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

fn home_bin_candidates(home: &Path, binary: &str) -> Vec<PathBuf> {
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
    dirs.extend(fnm_multishell_dirs(home));
    dirs
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

fn windows_npm_candidates(binary: &str) -> Vec<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn preferred_path_trims_wrapping_quotes() {
        assert_eq!(
            clean_preferred_path("  '/usr/local/bin/codex'  "),
            "/usr/local/bin/codex"
        );
        assert_eq!(
            clean_preferred_path("  \"C:\\Users\\me\\AppData\\Roaming\\npm\\codex.cmd\"  "),
            "C:\\Users\\me\\AppData\\Roaming\\npm\\codex.cmd"
        );
        assert_eq!(clean_preferred_path("codex"), "codex");
    }

    #[test]
    fn separator_detection_handles_unix_and_windows_paths() {
        assert!(has_path_separator("/usr/local/bin/codex"));
        assert!(has_path_separator(
            r"C:\Users\me\AppData\Roaming\npm\codex.cmd"
        ));
        assert!(!has_path_separator("codex"));
    }

    #[test]
    fn home_bin_candidates_include_node_manager_shims() {
        let home = Path::new("/Users/example");
        let candidates = home_bin_candidates(home, "codex");
        assert!(candidates.contains(&PathBuf::from("/Users/example/.volta/bin/codex")));
        assert!(candidates.contains(&PathBuf::from("/Users/example/.asdf/shims/codex")));
        assert!(candidates.contains(&PathBuf::from(
            "/Users/example/.local/share/mise/shims/codex"
        )));
        assert!(candidates.contains(&PathBuf::from("/Users/example/.npm-global/bin/codex")));
        assert!(candidates.contains(&PathBuf::from("/Users/example/n/bin/codex")));
        assert!(candidates.contains(&PathBuf::from("/Users/example/.bun/bin/codex")));
    }

    #[test]
    fn codex_home_candidates_include_codex_home_bin() {
        let home = Path::new("/Users/example");
        let candidates = codex_home_candidates(home);
        assert!(candidates.contains(&PathBuf::from("/Users/example/.codex/bin/codex")));
    }

    #[test]
    fn codex_home_candidates_do_not_include_desktop_app_binary() {
        let home = Path::new("/Users/example");
        let candidates = codex_home_candidates(home);
        assert!(!candidates.contains(&PathBuf::from(
            "/Applications/Codex.app/Contents/Resources/codex"
        )));
    }

    #[test]
    fn codex_desktop_app_binary_is_not_a_supported_cli_path() {
        assert!(is_unsupported_cli_path(
            Path::new("/Applications/Codex.app/Contents/Resources/codex"),
            &CODEX_SPEC
        ));
        assert!(!is_unsupported_cli_path(
            Path::new("/opt/homebrew/bin/codex"),
            &CODEX_SPEC
        ));
    }

    #[cfg(unix)]
    #[test]
    fn cli_probe_keeps_symlink_entrypoint_runtime_path() {
        fn no_home_candidates(_: &Path) -> Vec<PathBuf> {
            Vec::new()
        }

        let root = env::temp_dir().join(format!("balancehub-cli-symlink-{}", std::process::id()));
        let bin = root.join("bin");
        let lib = root.join("lib");
        let entrypoint = bin.join("balancehub-test-cli");
        let runtime = bin.join("balancehub-test-runtime");
        let target = lib.join("cli");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&lib).unwrap();
        fs::write(&runtime, "#!/bin/sh\nprintf 'test-cli 1.0\\n'\n").unwrap();
        fs::write(&target, "#!/usr/bin/env balancehub-test-runtime\n").unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        symlink(&target, &entrypoint).unwrap();

        let result = find_cli(
            entrypoint.to_str().unwrap(),
            &CliSpec {
                env_keys: &[],
                binary: "balancehub-test-cli",
                global_dirs: &[],
                home_candidates: no_home_candidates,
                require_version_substring: None,
                not_found_message: "not found",
            },
        )
        .unwrap();

        assert_eq!(result.path, entrypoint.to_string_lossy());
        assert_eq!(result.version, "test-cli 1.0");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn claude_home_candidates_include_native_installer_path() {
        let home = Path::new("/Users/example");
        let candidates = claude_home_candidates(home);
        assert!(candidates.contains(&PathBuf::from("/Users/example/.claude/local/claude")));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_binary_names_include_cmd_and_exe() {
        assert_eq!(binary_names("codex"), ["codex", "codex.cmd", "codex.exe"]);
    }
}
