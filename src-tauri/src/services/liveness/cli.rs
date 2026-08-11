use crate::models::CodexCliProbeResult;

mod paths;
#[cfg(test)]
mod tests;

use super::process::cli_version;
use std::{
    cmp::Ordering,
    env,
    path::{Path, PathBuf},
};

use paths::{
    binary_names, clean_preferred_path, expand_home_path, has_path_separator, home_bin_candidates,
    home_dir, node_manager_bin_dirs, normalize_path, path_candidates, shell_command_candidates,
    windows_npm_candidates,
};
pub(super) use paths::{runtime_path_for, runtime_path_for_without_shell};

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
    not_found_message: "未自动检测到可用的 Codex CLI",
};

const CLAUDE_SPEC: CliSpec = CliSpec {
    env_keys: &["CLAUDE_CODE_CLI_PATH", "CLAUDE_CLI_PATH"],
    binary: "claude",
    global_dirs: &["/opt/homebrew/bin", "/usr/local/bin"],
    home_candidates: claude_home_candidates,
    require_version_substring: Some("claude"),
    not_found_message: "未自动检测到可用的 Claude Code CLI",
};

pub(super) fn find_codex_cli(preferred_path: &str) -> Result<CodexCliProbeResult, String> {
    find_cli(preferred_path, &CODEX_SPEC, true)
}

pub(super) fn find_claude_cli(preferred_path: &str) -> Result<CodexCliProbeResult, String> {
    find_cli(preferred_path, &CLAUDE_SPEC, true)
}

pub(super) fn find_codex_cli_without_shell(
    preferred_path: &str,
) -> Result<CodexCliProbeResult, String> {
    find_cli(preferred_path, &CODEX_SPEC, false)
}

pub(super) fn find_claude_cli_without_shell(
    preferred_path: &str,
) -> Result<CodexCliProbeResult, String> {
    find_cli(preferred_path, &CLAUDE_SPEC, false)
}

fn codex_home_candidates(home: &Path) -> Vec<PathBuf> {
    let mut candidates = node_manager_bin_dirs(home)
        .into_iter()
        .map(|dir| dir.join("codex"))
        .collect::<Vec<_>>();
    candidates.push(home.join(".codex/bin/codex"));
    candidates.extend(home_bin_candidates(home, "codex"));
    candidates.extend(windows_npm_candidates("codex"));
    candidates
}

fn claude_home_candidates(home: &Path) -> Vec<PathBuf> {
    let mut candidates = node_manager_bin_dirs(home)
        .into_iter()
        .map(|dir| dir.join("claude"))
        .collect::<Vec<_>>();
    candidates.extend(home_bin_candidates(home, "claude"));
    candidates.push(home.join(".claude/local/claude"));
    candidates.extend(windows_npm_candidates("claude"));
    candidates
}

fn explicit_env_candidates(spec: &CliSpec, include_shell: bool) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for key in spec.env_keys {
        if let Ok(path) = env::var(key) {
            let path = clean_preferred_path(&path);
            if !path.is_empty() {
                candidates.push(expand_home_path(&path));
                if !has_path_separator(&path) {
                    candidates.extend(path_candidates(&path, include_shell));
                }
            }
        }
    }
    candidates
}

/// 按优先级构建 CLI 候选路径：preferred → 环境变量 → 各 CLI 专属路径 → 常见安装目录 → PATH → shell。
fn cli_candidates(
    preferred_path: &str,
    explicit_env_candidates: &[PathBuf],
    spec: &CliSpec,
    include_shell: bool,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let preferred_path = clean_preferred_path(preferred_path);
    if !preferred_path.is_empty() {
        let preferred = expand_home_path(&preferred_path);
        candidates.push(preferred.clone());
        if !has_path_separator(&preferred_path) {
            candidates.extend(path_candidates(&preferred_path, include_shell));
        }
    }
    candidates.extend(explicit_env_candidates.iter().cloned());
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
    if include_shell {
        candidates.extend(shell_command_candidates(spec.binary));
    }
    candidates
}

/// 显式自定义路径有效时优先使用；NVM/FNM 版本路径与自动发现候选则选择最高版本。
fn find_cli(
    preferred_path: &str,
    spec: &CliSpec,
    include_shell: bool,
) -> Result<CodexCliProbeResult, String> {
    let preferred_path = clean_preferred_path(preferred_path);
    let preferred_can_move = preferred_path_is_version_managed(&preferred_path);
    let explicit_env_candidates = explicit_env_candidates(spec, include_shell);
    let mut seen = Vec::new();
    let mut best: Option<(CodexCliProbeResult, Vec<u64>)> = None;
    let mut failures = Vec::new();
    for candidate in cli_candidates(
        &preferred_path,
        &explicit_env_candidates,
        spec,
        include_shell,
    ) {
        if seen.iter().any(|item: &PathBuf| item == &candidate) {
            continue;
        }
        seen.push(candidate.clone());
        let explicit_env = explicit_env_candidates.contains(&candidate);
        if is_unsupported_cli_path(&normalize_path(candidate.clone()), spec) {
            if candidate_matches_preferred(&candidate, &preferred_path) || explicit_env {
                failures.push(format!(
                    "{}: {}",
                    candidate.display(),
                    unsupported_cli_path_message(spec)
                ));
            }
            continue;
        }
        if !candidate.is_file() {
            if candidate_matches_preferred(&candidate, &preferred_path) || explicit_env {
                failures.push(format!("{}: 文件不存在", candidate.display()));
            }
            continue;
        }
        match cli_version(&candidate, spec.require_version_substring, include_shell) {
            Ok(version) => {
                let result = CodexCliProbeResult {
                    path: candidate.to_string_lossy().to_string(),
                    version,
                };
                if candidate_has_fixed_priority(
                    &candidate,
                    &preferred_path,
                    preferred_can_move,
                    &explicit_env_candidates,
                ) {
                    return Ok(result);
                }
                let version_key = numeric_version_key(&result.version);
                let should_replace = best.as_ref().is_none_or(|(_, current_key)| {
                    compare_version_keys(&version_key, current_key) == Ordering::Greater
                });
                if should_replace {
                    best = Some((result, version_key));
                }
            }
            Err(message) => {
                if failures.len() < 4
                    || candidate_matches_preferred(&candidate, &preferred_path)
                    || explicit_env
                {
                    failures.push(format!("{}: {message}", candidate.display()));
                }
            }
        }
    }

    if let Some((result, _)) = best {
        return Ok(result);
    }

    failures.truncate(4);
    if failures.is_empty() {
        Err(spec.not_found_message.to_string())
    } else {
        Err(format!(
            "{}；{}",
            spec.not_found_message,
            failures.join("；")
        ))
    }
}

fn candidate_matches_preferred(candidate: &Path, preferred_path: &str) -> bool {
    !preferred_path.is_empty() && candidate == expand_home_path(preferred_path)
}

fn candidate_has_fixed_priority(
    candidate: &Path,
    preferred_path: &str,
    preferred_can_move: bool,
    explicit_env_candidates: &[PathBuf],
) -> bool {
    (candidate_matches_preferred(candidate, preferred_path) && !preferred_can_move)
        || explicit_env_candidates.iter().any(|path| path == candidate)
}

fn preferred_path_is_version_managed(preferred_path: &str) -> bool {
    if preferred_path.is_empty() {
        return false;
    }
    let path = expand_home_path(preferred_path)
        .to_string_lossy()
        .replace('\\', "/");
    path.contains("/.nvm/versions/node/")
        || path.contains("/.fnm/node-versions/")
        || path.contains("/.local/share/fnm/node-versions/")
        || path.contains("/.local/state/fnm_multishells/")
}

fn numeric_version_key(value: &str) -> Vec<u64> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u64>().ok())
        .collect()
}

fn compare_version_keys(left: &[u64], right: &[u64]) -> Ordering {
    let length = left.len().max(right.len());
    for index in 0..length {
        match left
            .get(index)
            .copied()
            .unwrap_or(0)
            .cmp(&right.get(index).copied().unwrap_or(0))
        {
            Ordering::Equal => continue,
            ordering => return ordering,
        }
    }
    Ordering::Equal
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
