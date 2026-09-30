use super::candidates::{compare_version_keys, numeric_version_key};
use crate::{limits, platform::process::run_command_with_output_timeout};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

pub(in crate::services::agent_cli) fn runtime_binary_candidates(
    binary: &str,
    preferred_directory: Option<&Path>,
) -> Vec<PathBuf> {
    let snapshot = legacy_runtime_path_snapshot(false);
    let mut directories = Vec::new();
    if let Some(preferred) = preferred_directory {
        directories.push(preferred.to_path_buf());
    }
    directories.extend(snapshot.directories);
    let mut seen = BTreeSet::new();
    directories
        .into_iter()
        .flat_map(|directory| {
            binary_names(binary)
                .into_iter()
                .map(move |name| directory.join(name))
        })
        .filter(|path| path.is_file() && seen.insert(lexical_comparison_key(path)))
        .take(crate::models::AgentAssetLimits::DEFAULT.candidate_paths_per_agent)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::services::agent_cli) enum AgentHomeScanCompletion {
    Complete,
    Truncated,
    BudgetExceeded,
}

#[derive(Debug, Clone)]
enum AgentHomeScanMode {
    Legacy,
    InventoryBounded {
        deadline: Instant,
        max_candidates: usize,
        excluded_lexical_keys: BTreeSet<String>,
    },
}

#[derive(Debug, Clone)]
pub(in crate::services::agent_cli) struct AgentHomeCandidateScanRequest {
    home: PathBuf,
    mode: AgentHomeScanMode,
}

impl AgentHomeCandidateScanRequest {
    pub(super) fn legacy(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            mode: AgentHomeScanMode::Legacy,
        }
    }

    pub(super) fn inventory(
        home: &Path,
        deadline: Instant,
        max_candidates: usize,
        excluded_lexical_keys: BTreeSet<String>,
    ) -> Self {
        Self {
            home: home.to_path_buf(),
            mode: AgentHomeScanMode::InventoryBounded {
                deadline,
                max_candidates,
                excluded_lexical_keys,
            },
        }
    }

    pub(in crate::services::agent_cli) fn home(&self) -> &Path {
        &self.home
    }
}

#[derive(Debug, Clone)]
pub(in crate::services::agent_cli) struct AgentHomeCandidateScanResult {
    pub(super) candidates: Vec<PathBuf>,
    pub(super) runtime_dirs: Vec<PathBuf>,
    pub(super) completion: AgentHomeScanCompletion,
}

#[derive(Debug, Clone, Default)]
pub(super) struct RuntimePathSnapshot {
    directories: Vec<PathBuf>,
}

impl RuntimePathSnapshot {
    pub(super) fn from_parts(
        runtime_home_dirs: Vec<PathBuf>,
        shell_path: Option<&OsString>,
    ) -> Self {
        let mut directories = runtime_home_dirs;
        directories.extend(platform_global_dirs().iter().copied().map(PathBuf::from));
        if let Some(path) = shell_path {
            directories.extend(env::split_paths(path));
        }
        if let Some(path) = env::var_os("PATH") {
            directories.extend(env::split_paths(&path));
        }
        Self { directories }
    }

    pub(super) fn path_for(&self, cli_path: &Path) -> Option<OsString> {
        let mut directories = Vec::with_capacity(self.directories.len() + 1);
        if let Some(parent) = cli_path.parent() {
            directories.push(parent.to_path_buf());
        }
        directories.extend(self.directories.iter().cloned());

        let mut seen = BTreeSet::new();
        directories.retain(|directory| {
            !directory.as_os_str().is_empty()
                && seen.insert(lexical_comparison_key(directory.as_path()))
        });
        env::join_paths(directories).ok()
    }
}

pub(in crate::services::agent_cli) fn runtime_path_for(cli_path: &Path) -> Option<OsString> {
    legacy_runtime_path_snapshot(true).path_for(cli_path)
}

pub(in crate::services::agent_cli) fn runtime_path_without_shell_for(
    cli_path: &Path,
) -> Option<OsString> {
    legacy_runtime_path_snapshot(false).path_for(cli_path)
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

pub(super) fn process_path_candidates(binary: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = env::var("PATH") {
        for dir in env::split_paths(&path) {
            candidates.extend(binary_names(binary).into_iter().map(|name| dir.join(name)));
        }
    }
    candidates
}

#[derive(Debug, Clone, Default)]
pub(super) struct ShellDiscoverySnapshot {
    path: Option<OsString>,
    candidates: BTreeMap<String, Vec<PathBuf>>,
}

impl ShellDiscoverySnapshot {
    pub(super) fn path(&self) -> Option<&OsString> {
        self.path.as_ref()
    }

    pub(super) fn candidates_for(&self, binary: &str) -> Vec<PathBuf> {
        self.candidates.get(binary).cloned().unwrap_or_default()
    }
}

pub(super) fn capture_shell_discovery(binaries: &[String]) -> ShellDiscoverySnapshot {
    let binaries = binaries.iter().filter(|binary| !binary.is_empty()).fold(
        Vec::<String>::new(),
        |mut unique, binary| {
            if !unique.iter().any(|candidate| candidate == binary) {
                unique.push(binary.clone());
            }
            unique
        },
    );

    if cfg!(target_os = "windows") {
        let mut candidates = BTreeMap::new();
        for binary in binaries {
            let mut command = Command::new("cmd");
            command.arg("/C").arg(format!("where {binary}"));
            let paths = run_command_with_output_timeout(
                &mut command,
                Duration::from_secs(5),
                limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
            )
            .ok()
            .filter(|output| {
                !output.timed_out && output.status.is_some_and(|status| status.success())
            })
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
            candidates.insert(binary, paths);
        }
        return ShellDiscoverySnapshot {
            path: env::var_os("PATH"),
            candidates,
        };
    }

    let mut command = String::from("printf '__BALANCEHUB_PATH__%s\\n' \"$PATH\"\n");
    for (index, binary) in binaries.iter().enumerate() {
        command.push_str(&format!(
            "printf '__BALANCEHUB_COMMAND_{index}__\\n'\ncommand -v {} 2>/dev/null || true\nwhich -a {} 2>/dev/null || true\n",
            shell_escape_word(binary),
            shell_escape_word(binary)
        ));
    }
    let Some(output) = login_shell_output(&command) else {
        return ShellDiscoverySnapshot::default();
    };

    let mut snapshot = ShellDiscoverySnapshot::default();
    let mut active_binary: Option<&str> = None;
    for line in output.lines().map(str::trim) {
        if let Some(path) = line.strip_prefix("__BALANCEHUB_PATH__") {
            if !path.is_empty() {
                snapshot.path = Some(OsString::from(path));
            }
            active_binary = None;
            continue;
        }
        if let Some(index) = line
            .strip_prefix("__BALANCEHUB_COMMAND_")
            .and_then(|value| value.strip_suffix("__"))
            .and_then(|value| value.parse::<usize>().ok())
        {
            active_binary = binaries.get(index).map(String::as_str);
            continue;
        }
        if line.starts_with('/') {
            if let Some(binary) = active_binary {
                snapshot
                    .candidates
                    .entry(binary.to_string())
                    .or_default()
                    .push(PathBuf::from(line));
            }
        }
    }
    for paths in snapshot.candidates.values_mut() {
        let mut seen = BTreeSet::new();
        paths.retain(|path| seen.insert(lexical_comparison_key(path)));
    }
    snapshot
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

/// Build an absolute comparison key without resolving symlinks or consulting
/// metadata for the candidate itself. The runtime entrypoint is deliberately
/// kept separately and must remain the path passed to `Command`.
pub(super) fn lexical_comparison_key(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map(|current| current.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }

    let text = normalized.to_string_lossy().into_owned();
    #[cfg(target_os = "windows")]
    {
        let text = text.replace('\\', "/");
        let text = if let Some(rest) = text.strip_prefix("//?/UNC/") {
            format!("//{rest}")
        } else {
            text.strip_prefix("//?/").unwrap_or(&text).to_string()
        };
        text.to_lowercase()
    }
    #[cfg(not(target_os = "windows"))]
    {
        text
    }
}

pub(super) fn canonicalize_for_owner(path: &Path) -> std::io::Result<PathBuf> {
    fs::canonicalize(path)
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

pub(super) fn platform_global_dirs() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"]
    } else if cfg!(target_os = "windows") {
        &[]
    } else {
        &["/usr/local/bin", "/usr/bin", "/bin"]
    }
}

pub(in crate::services::agent_cli) fn node_cli_home_candidates(
    request: AgentHomeCandidateScanRequest,
    binary: &str,
    extra_candidates: impl IntoIterator<Item = PathBuf>,
) -> AgentHomeCandidateScanResult {
    let mut scanner = HomeCandidateScanner::new(request, Some(binary));
    if !scanner.add_runtime_dirs(fixed_runtime_home_dirs(scanner.home())) {
        return scanner.finish();
    }
    for (base, suffix) in node_manager_bin_sources(scanner.home()) {
        let result = versioned_bin_dirs(
            &base,
            suffix,
            scanner.deadline(),
            scanner.dynamic_directory_limit(),
            Some(binary),
        );
        if !scanner.add_dynamic_runtime_dirs(result) {
            return scanner.finish();
        }
    }
    let result = fnm_multishell_dirs(
        scanner.home(),
        scanner.deadline(),
        scanner.dynamic_directory_limit(),
        Some(binary),
    );
    if !scanner.add_dynamic_runtime_dirs(result) {
        return scanner.finish();
    }
    if !scanner.add_fixed_candidates(windows_npm_candidates(binary)) {
        return scanner.finish();
    }
    scanner.add_fixed_candidates(extra_candidates);
    scanner.finish()
}

pub(in crate::services::agent_cli) fn fixed_home_candidates(
    request: AgentHomeCandidateScanRequest,
    candidates: impl IntoIterator<Item = PathBuf>,
) -> AgentHomeCandidateScanResult {
    let mut scanner = HomeCandidateScanner::new(request, None);
    scanner.add_fixed_candidates(candidates);
    scanner.finish()
}

fn fixed_runtime_home_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".local/bin"),
        home.join(".npm-global/bin"),
        home.join("n/bin"),
        home.join(".volta/bin"),
        home.join(".asdf/shims"),
        home.join(".local/share/mise/shims"),
        home.join(".bun/bin"),
        home.join("Library/pnpm"),
        home.join(".local/share/pnpm"),
    ]
}

fn node_manager_bin_sources(home: &Path) -> [(PathBuf, &'static str); 3] {
    [
        (home.join(".nvm/versions/node"), "bin"),
        (home.join(".fnm/node-versions"), "installation/bin"),
        (
            home.join(".local/share/fnm/node-versions"),
            "installation/bin",
        ),
    ]
}

#[derive(Debug, Clone)]
struct RankedRuntimeDirectory {
    version_key: Vec<u64>,
    lexical_key: String,
    path: PathBuf,
}

#[derive(Debug)]
struct RuntimeDirectoryScanResult {
    directories: Vec<PathBuf>,
    candidate_directories: Vec<PathBuf>,
    completion: AgentHomeScanCompletion,
}

fn versioned_bin_dirs(
    base: &Path,
    suffix: &str,
    deadline: Option<Instant>,
    limit: usize,
    binary: Option<&str>,
) -> RuntimeDirectoryScanResult {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return RuntimeDirectoryScanResult {
            directories: Vec::new(),
            candidate_directories: Vec::new(),
            completion: AgentHomeScanCompletion::BudgetExceeded,
        };
    }
    let Ok(entries) = fs::read_dir(base) else {
        return RuntimeDirectoryScanResult {
            directories: Vec::new(),
            candidate_directories: Vec::new(),
            completion: AgentHomeScanCompletion::Complete,
        };
    };
    let mut ranked = Vec::new();
    let mut candidates = Vec::new();
    let mut truncated = false;
    for entry in entries {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return RuntimeDirectoryScanResult {
                directories: Vec::new(),
                candidate_directories: Vec::new(),
                completion: AgentHomeScanCompletion::BudgetExceeded,
            };
        }
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path().join(suffix);
        let directory = RankedRuntimeDirectory {
            version_key: numeric_version_key(&name),
            lexical_key: lexical_comparison_key(&path),
            path,
        };
        // Node runtimes are useful even when this Agent is not installed there.
        // Keep a bounded runtime list, but only actual CLI paths can truncate
        // the installation scan.
        insert_ranked_directory(
            &mut ranked,
            directory.clone(),
            limit,
            compare_versioned_runtime_dirs,
        );
        if has_candidate_binary(&directory.path, binary) {
            truncated |= insert_ranked_directory(
                &mut candidates,
                directory,
                limit,
                compare_versioned_runtime_dirs,
            );
        }
    }
    ranked.sort_by(compare_versioned_runtime_dirs);
    candidates.sort_by(compare_versioned_runtime_dirs);
    RuntimeDirectoryScanResult {
        directories: ranked.into_iter().map(|entry| entry.path).collect(),
        candidate_directories: candidates.into_iter().map(|entry| entry.path).collect(),
        completion: if truncated {
            AgentHomeScanCompletion::Truncated
        } else {
            AgentHomeScanCompletion::Complete
        },
    }
}

fn fnm_multishell_dirs(
    home: &Path,
    deadline: Option<Instant>,
    limit: usize,
    binary: Option<&str>,
) -> RuntimeDirectoryScanResult {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return RuntimeDirectoryScanResult {
            directories: Vec::new(),
            candidate_directories: Vec::new(),
            completion: AgentHomeScanCompletion::BudgetExceeded,
        };
    }
    let base = home.join(".local/state/fnm_multishells");
    let Ok(entries) = fs::read_dir(base) else {
        return RuntimeDirectoryScanResult {
            directories: Vec::new(),
            candidate_directories: Vec::new(),
            completion: AgentHomeScanCompletion::Complete,
        };
    };

    let mut ranked = Vec::new();
    let mut candidates = Vec::new();
    let mut truncated = false;
    for entry in entries {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return RuntimeDirectoryScanResult {
                directories: Vec::new(),
                candidate_directories: Vec::new(),
                completion: AgentHomeScanCompletion::BudgetExceeded,
            };
        }
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path().join("bin");
        let directory = RankedRuntimeDirectory {
            version_key: Vec::new(),
            lexical_key: lexical_comparison_key(&path),
            path,
        };
        insert_ranked_directory(
            &mut ranked,
            directory.clone(),
            limit,
            compare_lexical_runtime_dirs,
        );
        if has_candidate_binary(&directory.path, binary) {
            truncated |= insert_ranked_directory(
                &mut candidates,
                directory,
                limit,
                compare_lexical_runtime_dirs,
            );
        }
    }
    ranked.sort_by(compare_lexical_runtime_dirs);
    candidates.sort_by(compare_lexical_runtime_dirs);
    RuntimeDirectoryScanResult {
        directories: ranked.into_iter().map(|entry| entry.path).collect(),
        candidate_directories: candidates.into_iter().map(|entry| entry.path).collect(),
        completion: if truncated {
            AgentHomeScanCompletion::Truncated
        } else {
            AgentHomeScanCompletion::Complete
        },
    }
}

fn has_candidate_binary(directory: &Path, binary: Option<&str>) -> bool {
    binary.is_none_or(|binary| {
        binary_names(binary)
            .iter()
            .any(|name| directory.join(name).is_file())
    })
}

fn insert_ranked_directory(
    ranked: &mut Vec<RankedRuntimeDirectory>,
    entry: RankedRuntimeDirectory,
    limit: usize,
    compare: fn(&RankedRuntimeDirectory, &RankedRuntimeDirectory) -> Ordering,
) -> bool {
    if limit == 0 {
        return true;
    }
    ranked.push(entry);
    ranked.sort_by(compare);
    if ranked.len() > limit {
        ranked.pop();
        true
    } else {
        false
    }
}

fn compare_versioned_runtime_dirs(
    left: &RankedRuntimeDirectory,
    right: &RankedRuntimeDirectory,
) -> Ordering {
    compare_version_keys(&right.version_key, &left.version_key)
        .then_with(|| left.lexical_key.cmp(&right.lexical_key))
}

fn compare_lexical_runtime_dirs(
    left: &RankedRuntimeDirectory,
    right: &RankedRuntimeDirectory,
) -> Ordering {
    left.lexical_key.cmp(&right.lexical_key)
}

struct HomeCandidateScanner {
    home: PathBuf,
    binary: Option<String>,
    deadline: Option<Instant>,
    max_candidates: usize,
    seen_candidates: BTreeSet<String>,
    seen_runtime_dirs: BTreeSet<String>,
    candidates: Vec<PathBuf>,
    runtime_dirs: Vec<PathBuf>,
    completion: AgentHomeScanCompletion,
}

impl HomeCandidateScanner {
    fn new(request: AgentHomeCandidateScanRequest, binary: Option<&str>) -> Self {
        let (deadline, max_candidates, seen_candidates) = match request.mode {
            AgentHomeScanMode::Legacy => (None, usize::MAX, BTreeSet::new()),
            AgentHomeScanMode::InventoryBounded {
                deadline,
                max_candidates,
                excluded_lexical_keys,
            } => (Some(deadline), max_candidates, excluded_lexical_keys),
        };
        Self {
            home: request.home,
            binary: binary.map(str::to_string),
            deadline,
            max_candidates,
            seen_candidates,
            seen_runtime_dirs: BTreeSet::new(),
            candidates: Vec::new(),
            runtime_dirs: Vec::new(),
            completion: AgentHomeScanCompletion::Complete,
        }
    }

    fn home(&self) -> &Path {
        &self.home
    }

    fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    fn dynamic_directory_limit(&self) -> usize {
        if self.max_candidates == usize::MAX {
            usize::MAX
        } else {
            self.max_candidates
                .saturating_sub(self.candidates.len())
                .saturating_add(self.seen_candidates.len())
        }
    }

    fn add_runtime_dirs(&mut self, directories: impl IntoIterator<Item = PathBuf>) -> bool {
        for directory in directories {
            let runtime_key = lexical_comparison_key(&directory);
            if self.seen_runtime_dirs.insert(runtime_key) {
                self.runtime_dirs.push(directory.clone());
            }
            let Some(binary) = self.binary.as_deref() else {
                continue;
            };
            for name in binary_names(binary) {
                if !self.add_candidate(directory.join(name)) {
                    return false;
                }
            }
        }
        true
    }

    fn add_dynamic_runtime_dirs(&mut self, result: RuntimeDirectoryScanResult) -> bool {
        match result.completion {
            AgentHomeScanCompletion::BudgetExceeded => {
                self.completion = AgentHomeScanCompletion::BudgetExceeded;
                false
            }
            AgentHomeScanCompletion::Complete | AgentHomeScanCompletion::Truncated => {
                if !self.add_runtime_dirs(result.directories)
                    || !self.add_runtime_dirs(result.candidate_directories)
                {
                    return false;
                }
                if result.completion == AgentHomeScanCompletion::Truncated {
                    self.completion = AgentHomeScanCompletion::Truncated;
                    return false;
                }
                true
            }
        }
    }

    fn add_fixed_candidates(&mut self, candidates: impl IntoIterator<Item = PathBuf>) -> bool {
        for candidate in candidates {
            if !self.add_candidate(candidate) {
                return false;
            }
        }
        true
    }

    fn add_candidate(&mut self, candidate: PathBuf) -> bool {
        if !candidate.is_file() {
            return true;
        }
        let key = lexical_comparison_key(&candidate);
        if self.seen_candidates.contains(&key) {
            return true;
        }
        if self.candidates.len() >= self.max_candidates {
            self.completion = AgentHomeScanCompletion::Truncated;
            return false;
        }
        self.seen_candidates.insert(key);
        self.candidates.push(candidate);
        true
    }

    fn finish(self) -> AgentHomeCandidateScanResult {
        AgentHomeCandidateScanResult {
            candidates: self.candidates,
            runtime_dirs: self.runtime_dirs,
            completion: self.completion,
        }
    }
}

fn runtime_home_dirs_legacy(home: &Path) -> Vec<PathBuf> {
    let mut directories = fixed_runtime_home_dirs(home);
    for (base, suffix) in node_manager_bin_sources(home) {
        directories.extend(versioned_bin_dirs(&base, suffix, None, usize::MAX, None).directories);
    }
    directories.extend(fnm_multishell_dirs(home, None, usize::MAX, None).directories);
    directories
}

pub(super) fn legacy_runtime_path_snapshot(include_shell: bool) -> RuntimePathSnapshot {
    if let Some(home) = home_dir() {
        return legacy_runtime_path_snapshot_for_home(&home, include_shell);
    }
    let shell = include_shell.then(|| capture_shell_discovery(&[]));
    RuntimePathSnapshot::from_parts(
        Vec::new(),
        shell.as_ref().and_then(ShellDiscoverySnapshot::path),
    )
}

pub(super) fn legacy_runtime_path_snapshot_for_home(
    home: &Path,
    include_shell: bool,
) -> RuntimePathSnapshot {
    let shell = include_shell.then(|| capture_shell_discovery(&[]));
    RuntimePathSnapshot::from_parts(
        runtime_home_dirs_legacy(home),
        shell.as_ref().and_then(ShellDiscoverySnapshot::path),
    )
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
