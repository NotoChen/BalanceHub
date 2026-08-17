use crate::{limits, platform::process::run_command_with_output_timeout};

pub(super) mod paths;
#[cfg(test)]
mod tests;

use super::{AgentCliDefinition, AgentCliExecutable};
use std::{
    cmp::Ordering,
    env,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub(super) use paths::runtime_path_for;
use paths::{
    binary_names, clean_preferred_path, expand_home_path, has_path_separator, home_dir,
    normalize_path, path_candidates, platform_global_dirs, shell_command_candidates,
};

const CLI_VERSION_TIMEOUT: Duration = Duration::from_secs(5);

fn explicit_env_candidates(spec: &AgentCliDefinition, include_shell: bool) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let balancehub_key = balancehub_cli_path_env_key(spec.kind);
    for key in
        std::iter::once(balancehub_key.as_str()).chain(spec.additional_env_keys.iter().copied())
    {
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
    spec: &AgentCliDefinition,
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
    for dir in platform_global_dirs() {
        candidates.extend(
            binary_names(spec.executable)
                .into_iter()
                .map(|name| PathBuf::from(dir).join(name)),
        );
    }
    if let Ok(path) = env::var("PATH") {
        for dir in env::split_paths(&path) {
            candidates.extend(
                binary_names(spec.executable)
                    .into_iter()
                    .map(|name| dir.join(name)),
            );
        }
    }
    if include_shell {
        candidates.extend(shell_command_candidates(spec.executable));
    }
    candidates
}

/// 显式自定义路径有效时优先使用；NVM/FNM 版本路径与自动发现候选则选择最高版本。
pub(super) fn find_cli(
    preferred_path: &str,
    spec: &AgentCliDefinition,
    include_shell: bool,
) -> Result<AgentCliExecutable, String> {
    let preferred_path = clean_preferred_path(preferred_path);
    let preferred_can_move = preferred_path_is_version_managed(&preferred_path);
    let explicit_env_candidates = explicit_env_candidates(spec, include_shell);
    let mut seen = Vec::new();
    let mut best: Option<(AgentCliExecutable, Vec<u64>)> = None;
    let mut failures = Vec::new();

    // 固定路径不需要等待登录 shell 扫描。优先探测它们既符合用户选择，
    // 也避免 shell 插件或版本管理器让一次本地 CLI 扫描无谓阻塞数秒。
    let mut fixed_candidates = Vec::new();
    if !preferred_path.is_empty() && !preferred_can_move {
        fixed_candidates.push(expand_home_path(&preferred_path));
    }
    fixed_candidates.extend(explicit_env_candidates.iter().cloned());
    for candidate in fixed_candidates {
        if seen.iter().any(|item: &PathBuf| item == &candidate) {
            continue;
        }
        seen.push(candidate.clone());
        match probe_cli_candidate(&candidate, spec, include_shell) {
            Ok(result) => return Ok(result),
            Err(message) => failures.push(format!("{}: {message}", candidate.display())),
        }
    }

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
        match probe_cli_candidate(&candidate, spec, include_shell) {
            Ok(result) => {
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
    let not_found_message = format!(
        "未自动检测到可用的 {}{}",
        spec.label,
        if spec.label.to_ascii_lowercase().ends_with("cli") {
            ""
        } else {
            " CLI"
        }
    );
    if failures.is_empty() {
        Err(not_found_message)
    } else {
        Err(format!("{not_found_message}；{}", failures.join("；")))
    }
}

fn probe_cli_candidate(
    candidate: &Path,
    spec: &AgentCliDefinition,
    include_shell: bool,
) -> Result<AgentCliExecutable, String> {
    if let Some(validate) = spec.invalid_path_reason {
        if let Some(message) = validate(&normalize_path(candidate.to_path_buf())) {
            return Err(message.to_string());
        }
    }
    if !candidate.is_file() {
        return Err("文件不存在".to_string());
    }
    let version = cli_version(candidate, spec.require_version_substring, include_shell)?;
    Ok(AgentCliExecutable {
        path: candidate.to_string_lossy().to_string(),
        version,
    })
}

fn balancehub_cli_path_env_key(kind: crate::models::AgentCliKind) -> String {
    let mut key = String::from("BALANCEHUB_");
    let mut previous_was_lowercase = false;
    for character in kind.key().chars() {
        if character.is_ascii_uppercase() && previous_was_lowercase {
            key.push('_');
        }
        key.push(character.to_ascii_uppercase());
        previous_was_lowercase = character.is_ascii_lowercase();
    }
    key.push_str("_CLI_PATH");
    key
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

fn cli_version(
    path: &Path,
    require_substring: Option<&str>,
    include_shell: bool,
) -> Result<String, String> {
    let mut command = Command::new(path);
    let runtime_path = if include_shell {
        paths::runtime_path_for(path)
    } else {
        paths::runtime_path_for_without_shell(path)
    };
    if let Some(path_env) = runtime_path {
        command.env("PATH", path_env);
    }
    command.arg("--version");
    let outcome = run_command_with_output_timeout(
        &mut command,
        CLI_VERSION_TIMEOUT,
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    )
    .map_err(|err| err.to_string())?;
    if outcome.timed_out {
        return Err("CLI 版本探测超时".to_string());
    }
    if !outcome.status.is_some_and(|status| status.success()) {
        let detail = outcome
            .stderr
            .lines()
            .chain(outcome.stdout.lines())
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| line.chars().take(180).collect::<String>());
        return Err(detail
            .map(|detail| format!("CLI 不可用：{detail}"))
            .unwrap_or_else(|| "CLI 不可用".to_string()));
    }
    let version = outcome
        .stdout
        .lines()
        .chain(outcome.stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string();
    if version.is_empty() {
        return Err("CLI 未返回版本信息".to_string());
    }
    if let Some(substring) = require_substring {
        if !version.to_ascii_lowercase().contains(substring) {
            return Err("CLI 版本信息不匹配".to_string());
        }
    }
    Ok(version)
}
