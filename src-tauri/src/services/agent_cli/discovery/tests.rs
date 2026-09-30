#[cfg(target_os = "windows")]
use super::paths::binary_names;
#[cfg(unix)]
use super::paths::{
    legacy_runtime_path_snapshot_for_home, AgentHomeCandidateScanResult, AgentHomeScanCompletion,
};
use super::paths::{AgentHomeCandidateScanRequest, RuntimePathSnapshot};
use super::*;
use crate::models::{
    AgentAssetDiagnostic, AgentAssetLimitKind, AgentAssetLimits, AgentCliKind,
    AgentConfigurationContext, AgentExecutableProbeErrorKind, AgentInstallation,
};
use crate::services::agent_cli::definition;
use crate::services::agent_cli::environment::run::{AgentInventoryRun, SystemMonotonicClock};
#[cfg(unix)]
use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
struct TestInstallationDiscoveryResult {
    installations: Vec<AgentInstallation>,
    diagnostics: Vec<AgentAssetDiagnostic>,
}

fn discover_installations_with<F>(
    candidates: Vec<CliCandidate>,
    spec: &AgentCliDefinition,
    runtime_path: &RuntimePathSnapshot,
    limits: &AgentAssetLimits,
    deadline: Instant,
    probe_candidate: &F,
) -> TestInstallationDiscoveryResult
where
    F: Fn(
            &CliCandidate,
            &AgentCliDefinition,
            &RuntimePathSnapshot,
            Instant,
            Duration,
            usize,
        ) -> Result<ProbedCli, ProbeFailure>
        + Sync,
{
    let mut run = AgentInventoryRun::with_clock(limits.clone(), Arc::new(SystemMonotonicClock));
    let mut installations = discover_installations_with_run(
        candidates,
        spec,
        runtime_path,
        deadline,
        probe_candidate,
        &mut run,
    );
    for installation in &mut installations {
        installation.diagnostics = run.take_installation_diagnostics(&installation.id);
    }
    TestInstallationDiscoveryResult {
        installations,
        diagnostics: run.finish_diagnostics(),
    }
}

#[cfg(unix)]
use crate::services::agent_cli::contracts::EndpointAdapter;

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};
#[cfg(unix)]
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use std::{
    env,
    path::{Path, PathBuf},
};

#[cfg(unix)]
const REAL_PROCESS_SUCCESS_WATCHDOG: Duration = Duration::from_secs(10);

fn candidate(
    path: impl Into<PathBuf>,
    source: AgentDiscoverySource,
    ordinal: usize,
) -> CliCandidate {
    let runtime_entrypoint = path.into();
    CliCandidate {
        lexical_key: lexical_comparison_key(&runtime_entrypoint),
        runtime_entrypoint,
        source,
        fixed_priority: source == AgentDiscoverySource::Configured,
        ordinal,
    }
}

fn fake_probed(candidate: &CliCandidate, owner: &str, version: &str) -> ProbedCli {
    ProbedCli {
        executable: AgentCliExecutable {
            path: candidate.runtime_entrypoint.to_string_lossy().into_owned(),
            version: version.to_string(),
        },
        owner_key: owner.to_string(),
        canonical_path: format!("/canonical/{owner}"),
        executable_revision: format!("revision:{owner}:{version}"),
        output_truncated: false,
    }
}

#[cfg(unix)]
fn expect_single_installation<'a>(
    phase: &str,
    result: &'a TestInstallationDiscoveryResult,
) -> &'a AgentInstallation {
    assert_eq!(
        result.installations.len(),
        1,
        "phase {phase}: expected exactly one installation; typed diagnostics: {:?}",
        result.diagnostics
    );
    &result.installations[0]
}

fn test_limits(cli_concurrency: usize) -> AgentAssetLimits {
    AgentAssetLimits {
        candidate_paths_per_agent: 32,
        installations_per_agent: 32,
        cli_concurrency,
        ..AgentAssetLimits::DEFAULT
    }
}

#[cfg(unix)]
fn unix_test_root(label: &str) -> PathBuf {
    static NEXT_ROOT: AtomicUsize = AtomicUsize::new(0);
    env::temp_dir().join(format!(
        "balancehub-cli-{label}-{}-{}",
        std::process::id(),
        NEXT_ROOT.fetch_add(1, AtomicOrdering::Relaxed)
    ))
}

#[cfg(unix)]
fn real_process_fixture_guard() -> MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(unix)]
fn write_executable(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn synchronized_probe_script(started: &Path, release: &Path, version: &str) -> String {
    format!(
        "#!/bin/sh\nset -eu\n: > '{}'\nwhile [ ! -f '{}' ]; do :; done\nprintf '{}\\n'\n",
        started.display(),
        release.display(),
        version
    )
}

#[cfg(unix)]
fn wait_for_probe_start(started: &Path) {
    let deadline = Instant::now() + REAL_PROCESS_SUCCESS_WATCHDOG;
    while !started.is_file() {
        assert!(
            Instant::now() < deadline,
            "probe did not reach the synchronized execution window"
        );
        std::thread::yield_now();
    }
}

#[cfg(unix)]
fn discover_real_candidates(
    candidates: Vec<CliCandidate>,
    output_limit: usize,
) -> TestInstallationDiscoveryResult {
    let mut limits = test_limits(4);
    limits.cli_output_bytes = output_limit;
    discover_installations_with(
        candidates,
        definition(AgentCliKind::Codex),
        &RuntimePathSnapshot::default(),
        &limits,
        Instant::now() + REAL_PROCESS_SUCCESS_WATCHDOG,
        &|candidate, spec, runtime_path, deadline, _timeout, output_limit| {
            probe::probe_cli_candidate_with_gate(
                &candidate.runtime_entrypoint,
                spec,
                runtime_path,
                deadline,
                REAL_PROCESS_SUCCESS_WATCHDOG,
                output_limit,
                probe::global_cli_process_gate(),
            )
        },
    )
}

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
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let installed = [
        ".volta/bin/codex",
        ".asdf/shims/codex",
        ".local/share/mise/shims/codex",
        ".npm-global/bin/codex",
        "n/bin/codex",
        ".bun/bin/codex",
    ]
    .map(|relative| home.join(relative));
    for path in &installed {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }
    let candidates =
        (definition(AgentCliKind::Codex).home_scan)(AgentHomeCandidateScanRequest::legacy(home))
            .candidates;
    for path in installed {
        assert!(candidates.contains(&path));
    }
}

#[test]
fn codex_home_candidates_include_codex_home_bin() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let executable = home.join(".codex/bin/codex");
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::write(&executable, b"").unwrap();
    let candidates =
        (definition(AgentCliKind::Codex).home_scan)(AgentHomeCandidateScanRequest::legacy(home))
            .candidates;
    assert!(candidates.contains(&executable));
}

#[test]
fn codex_home_candidates_do_not_include_desktop_app_binary() {
    let home = Path::new("/Users/example");
    let candidates =
        (definition(AgentCliKind::Codex).home_scan)(AgentHomeCandidateScanRequest::legacy(home))
            .candidates;
    assert!(!candidates.contains(&PathBuf::from(
        "/Applications/Codex.app/Contents/Resources/codex"
    )));
}

#[test]
fn codex_desktop_app_binary_is_not_a_supported_cli_path() {
    let codex = definition(AgentCliKind::Codex);
    let invalid_path_reason = codex.invalid_path_reason.unwrap();
    assert!(invalid_path_reason(Path::new(
        "/Applications/Codex.app/Contents/Resources/codex"
    ))
    .is_some());
    assert!(invalid_path_reason(Path::new("/opt/homebrew/bin/codex")).is_none());
}

#[test]
fn project_cli_path_environment_key_is_derived_from_the_catalog_key() {
    assert_eq!(
        balancehub_cli_path_env_key(AgentCliKind::Codex),
        "BALANCEHUB_CODEX_CLI_PATH"
    );
    assert_eq!(
        balancehub_cli_path_env_key(AgentCliKind::ClaudeCode),
        "BALANCEHUB_CLAUDE_CODE_CLI_PATH"
    );
    assert_eq!(
        balancehub_cli_path_env_key(AgentCliKind::Grok),
        "BALANCEHUB_GROK_CLI_PATH"
    );
}

#[test]
fn candidate_limit_reports_only_real_overflow() {
    let mut exact = CandidateCollector::new(2);
    exact.push(PathBuf::from("one"), AgentDiscoverySource::Automatic, false);
    exact.push(PathBuf::from("two"), AgentDiscoverySource::Automatic, false);
    assert_eq!(exact.candidates.len(), 2);
    assert!(!exact.truncated);
    assert!(candidate_truncation_diagnostic(&exact).is_none());

    exact.push(PathBuf::from("one"), AgentDiscoverySource::Configured, true);
    assert!(!exact.truncated);
    assert_eq!(exact.candidates[0].source, AgentDiscoverySource::Configured);
    assert!(exact.candidates[0].fixed_priority);

    exact.push(
        PathBuf::from("three"),
        AgentDiscoverySource::Automatic,
        false,
    );
    assert_eq!(exact.candidates.len(), 2);
    assert!(exact.truncated);
    assert!(matches!(
        candidate_truncation_diagnostic(&exact),
        Some(AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::CandidatePathsPerAgent,
            accepted: 2,
            observed_at_least: 3,
        })
    ));

    let mut empty = CandidateCollector::new(0);
    empty.push(PathBuf::from("one"), AgentDiscoverySource::Automatic, false);
    assert!(empty.candidates.is_empty());
    assert!(empty.truncated);
}

#[test]
fn legacy_fixed_path_missing_error_remains_stable() {
    let missing = env::temp_dir().join(format!("balancehub-cli-missing-{}", std::process::id()));
    assert_eq!(
        find_cli_at_path(&missing, definition(AgentCliKind::Codex)).unwrap_err(),
        "文件不存在"
    );
}

#[cfg(unix)]
#[test]
fn cli_probe_keeps_symlink_entrypoint_runtime_path() {
    fn normalize_test_endpoint(base_url: &str) -> String {
        base_url.to_string()
    }

    fn unexpected_home_scan(_: AgentHomeCandidateScanRequest) -> AgentHomeCandidateScanResult {
        panic!("a valid fixed path must return before home or shell discovery")
    }

    let _real_process_fixture_guard = real_process_fixture_guard();
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
        &AgentCliDefinition {
            kind: AgentCliKind::Codex,
            label: "Test CLI",
            executable: "balancehub-test-cli",
            session_name_hint: "",
            additional_env_keys: &[],
            home_scan: unexpected_home_scan,
            invalid_path_reason: None,
            require_version_substring: None,
            endpoint: EndpointAdapter::new(normalize_test_endpoint),
            temporary_launch: None,
            sessions: None,
            liveness: None,
            default_config: None,
            environment: super::super::contracts::EnvironmentAdapter::with_pipeline(
                no_asset_contexts,
                no_asset_sources,
                None,
                "test-agent",
                no_asset_parse,
                no_asset_resolve,
                super::super::environment::fixture_assessor,
            ),
            ..*definition(AgentCliKind::Codex)
        },
        true,
    )
    .unwrap();

    assert_eq!(result.path, entrypoint.to_string_lossy());
    assert_eq!(result.version, "test-cli 1.0");
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn multi_install_discovery_keeps_symlink_entrypoint_and_groups_by_owner() {
    let _real_process_fixture_guard = real_process_fixture_guard();
    let root = unix_test_root("multi-symlink");
    let entrypoint = root.join("codex");
    let target = root.join("codex-target");
    fs::create_dir_all(&root).unwrap();
    write_executable(&target, "#!/bin/sh\nprintf 'codex-cli 1.0.0\\n'\n");
    symlink(&target, &entrypoint).unwrap();

    let result = discover_real_candidates(
        vec![
            candidate(&target, AgentDiscoverySource::Automatic, 0),
            candidate(&entrypoint, AgentDiscoverySource::Configured, 1),
        ],
        1024,
    );

    let installation = expect_single_installation("multi_install", &result);
    assert_eq!(
        installation.executable_path.as_deref(),
        Some(entrypoint.to_string_lossy().as_ref())
    );
    let identity = installation.executable_identity.as_ref().unwrap();
    let canonical_target = fs::canonicalize(&target).unwrap();
    assert_eq!(identity.canonical_path, canonical_target.to_string_lossy());
    assert_eq!(identity.owner, lexical_comparison_key(&canonical_target));
    assert_eq!(
        installation.discovery_source,
        AgentDiscoverySource::Configured
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn executable_revision_changes_for_version_and_in_place_replacement() {
    let _real_process_fixture_guard = real_process_fixture_guard();
    let root = unix_test_root("revision");
    let executable = root.join("codex");
    fs::create_dir_all(&root).unwrap();
    write_executable(&executable, "#!/bin/sh\nprintf 'codex-cli 1.0.0\\n'\n");

    let first = discover_real_candidates(
        vec![candidate(&executable, AgentDiscoverySource::Configured, 0)],
        1024,
    );
    write_executable(&executable, "#!/bin/sh\nprintf 'codex-cli 1.1.0\\n'\n");
    let version_changed = discover_real_candidates(
        vec![candidate(&executable, AgentDiscoverySource::Configured, 0)],
        1024,
    );

    let first_installation = expect_single_installation("first", &first);
    let version_changed_installation =
        expect_single_installation("version_changed", &version_changed);

    assert_eq!(first_installation.id, version_changed_installation.id);
    assert_ne!(
        first_installation.executable_revision,
        version_changed_installation.executable_revision
    );

    let replacement = root.join("replacement");
    write_executable(&replacement, "#!/bin/sh\nprintf 'codex-cli 1.1.0\\n'\n");
    fs::rename(&replacement, &executable).unwrap();
    let replaced = discover_real_candidates(
        vec![candidate(&executable, AgentDiscoverySource::Configured, 0)],
        1024,
    );
    let replaced_installation = expect_single_installation("replaced", &replaced);
    assert_eq!(version_changed_installation.id, replaced_installation.id);
    assert_ne!(
        version_changed_installation.executable_revision,
        replaced_installation.executable_revision
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn version_manager_candidate_order_is_deterministic_for_equal_versions() {
    let root = unix_test_root("candidate-order");
    for directory in [
        root.join(".nvm/versions/node/v1.0.0/bin"),
        root.join(".nvm/versions/node/1.0.0/bin"),
        root.join(".local/state/fnm_multishells/zeta/bin"),
        root.join(".local/state/fnm_multishells/alpha/bin"),
    ] {
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("codex"), b"").unwrap();
    }

    let candidates =
        (definition(AgentCliKind::Codex).home_scan)(AgentHomeCandidateScanRequest::legacy(&root))
            .candidates;
    let nvm = candidates
        .iter()
        .filter(|path| path.to_string_lossy().contains("/.nvm/versions/node/"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        nvm,
        vec![
            root.join(".nvm/versions/node/1.0.0/bin/codex"),
            root.join(".nvm/versions/node/v1.0.0/bin/codex"),
        ]
    );

    let multishells = candidates
        .iter()
        .filter(|path| {
            path.to_string_lossy()
                .contains("/.local/state/fnm_multishells/")
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        multishells,
        vec![
            root.join(".local/state/fnm_multishells/alpha/bin/codex"),
            root.join(".local/state/fnm_multishells/zeta/bin/codex"),
        ]
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
fn scan_bounded_gemini_home(
    home: &Path,
    max_candidates: usize,
    excluded: BTreeSet<String>,
    deadline: Instant,
) -> AgentHomeCandidateScanResult {
    (definition(AgentCliKind::Gemini).home_scan)(AgentHomeCandidateScanRequest::inventory(
        home,
        deadline,
        max_candidates,
        excluded,
    ))
}

#[cfg(unix)]
fn create_nvm_versions(home: &Path, versions: impl IntoIterator<Item = String>) {
    for version in versions {
        let directory = home.join(".nvm/versions/node").join(version).join("bin");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("gemini"), b"").unwrap();
    }
}

#[cfg(unix)]
fn scanned_nvm_versions(result: &AgentHomeCandidateScanResult) -> Vec<String> {
    result
        .candidates
        .iter()
        .filter(|path| path.to_string_lossy().contains("/.nvm/versions/node/"))
        .filter_map(|path| {
            path.parent()?
                .parent()?
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
        })
        .collect()
}

#[cfg(unix)]
#[test]
fn bounded_home_scan_reports_only_real_one_over() {
    let exact_root = unix_test_root("bounded-home-exact");
    create_nvm_versions(
        &exact_root,
        ["v1.0.0", "v2.0.0", "v3.0.0"].map(str::to_string),
    );
    let exact = scan_bounded_gemini_home(
        &exact_root,
        3,
        BTreeSet::new(),
        Instant::now() + Duration::from_secs(1),
    );
    assert_eq!(exact.candidates.len(), 3);
    assert_eq!(exact.completion, AgentHomeScanCompletion::Complete);
    assert_eq!(scanned_nvm_versions(&exact), ["v3.0.0", "v2.0.0", "v1.0.0"]);

    let overflow_root = unix_test_root("bounded-home-overflow");
    create_nvm_versions(
        &overflow_root,
        ["v1.0.0", "v2.0.0", "v3.0.0", "v4.0.0"].map(str::to_string),
    );
    let overflow = scan_bounded_gemini_home(
        &overflow_root,
        3,
        BTreeSet::new(),
        Instant::now() + Duration::from_secs(1),
    );
    assert_eq!(overflow.candidates.len(), 3);
    assert_eq!(overflow.completion, AgentHomeScanCompletion::Truncated);
    assert_eq!(
        scanned_nvm_versions(&overflow),
        ["v4.0.0", "v3.0.0", "v2.0.0"]
    );

    fs::remove_dir_all(exact_root).unwrap();
    fs::remove_dir_all(overflow_root).unwrap();
}

#[cfg(unix)]
#[test]
fn bounded_home_scan_filters_prior_duplicates_before_filling_top_k() {
    let root = unix_test_root("bounded-home-duplicates");
    create_nvm_versions(
        &root,
        ["v2.0.0", "v3.0.0", "v4.0.0", "v5.0.0"].map(str::to_string),
    );
    let excluded_path = root.join(".nvm/versions/node/v5.0.0/bin/gemini");
    let result = scan_bounded_gemini_home(
        &root,
        3,
        BTreeSet::from([lexical_comparison_key(&excluded_path)]),
        Instant::now() + Duration::from_secs(1),
    );

    assert_eq!(result.candidates.len(), 3);
    assert_eq!(result.completion, AgentHomeScanCompletion::Complete);
    assert_eq!(
        scanned_nvm_versions(&result),
        ["v4.0.0", "v3.0.0", "v2.0.0"]
    );
    assert!(!result.candidates.contains(&excluded_path));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn pathological_version_directory_keeps_only_deterministic_top_k() {
    let root = unix_test_root("bounded-home-pathological");
    create_nvm_versions(&root, (0..512).map(|index| format!("v{index}.0.0")));

    let result = scan_bounded_gemini_home(
        &root,
        3,
        BTreeSet::new(),
        Instant::now() + Duration::from_secs(3),
    );

    assert_eq!(result.candidates.len(), 3);
    assert_eq!(result.completion, AgentHomeScanCompletion::Truncated);
    assert_eq!(
        scanned_nvm_versions(&result),
        ["v511.0.0", "v510.0.0", "v509.0.0"]
    );
    assert!(
        result
            .runtime_dirs
            .iter()
            .filter(|path| path.to_string_lossy().contains("/.nvm/versions/node/"))
            .count()
            <= 4
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn expired_home_scan_discards_unfinished_dynamic_provider() {
    let root = unix_test_root("bounded-home-deadline");
    create_nvm_versions(&root, ["v1.0.0", "v2.0.0"].map(str::to_string));

    let result = scan_bounded_gemini_home(
        &root,
        32,
        BTreeSet::new(),
        Instant::now() - Duration::from_millis(1),
    );

    assert_eq!(result.completion, AgentHomeScanCompletion::BudgetExceeded);
    assert!(scanned_nvm_versions(&result).is_empty());
    assert!(result.candidates.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn bounded_home_scan_is_independent_of_directory_creation_order() {
    let first_root = unix_test_root("bounded-home-order-a");
    let second_root = unix_test_root("bounded-home-order-b");
    create_nvm_versions(
        &first_root,
        ["v1.0.0", "v4.0.0", "v2.0.0", "v3.0.0"].map(str::to_string),
    );
    create_nvm_versions(
        &second_root,
        ["v3.0.0", "v2.0.0", "v4.0.0", "v1.0.0"].map(str::to_string),
    );

    let scan = |root: &Path| {
        scanned_nvm_versions(&scan_bounded_gemini_home(
            root,
            3,
            BTreeSet::new(),
            Instant::now() + Duration::from_secs(1),
        ))
    };
    assert_eq!(scan(&first_root), scan(&second_root));

    fs::remove_dir_all(first_root).unwrap();
    fs::remove_dir_all(second_root).unwrap();
}

#[cfg(unix)]
#[test]
fn symlink_retarget_changes_owner_id_and_revision() {
    let _real_process_fixture_guard = real_process_fixture_guard();
    let root = unix_test_root("retarget");
    let entrypoint = root.join("codex");
    let first_target = root.join("target-a");
    let second_target = root.join("target-b");
    fs::create_dir_all(&root).unwrap();
    write_executable(&first_target, "#!/bin/sh\nprintf 'codex-cli 1.0.0\\n'\n");
    write_executable(&second_target, "#!/bin/sh\nprintf 'codex-cli 1.0.0\\n'\n");
    symlink(&first_target, &entrypoint).unwrap();
    let first = discover_real_candidates(
        vec![candidate(&entrypoint, AgentDiscoverySource::Configured, 0)],
        1024,
    );

    fs::remove_file(&entrypoint).unwrap();
    symlink(&second_target, &entrypoint).unwrap();
    let second = discover_real_candidates(
        vec![candidate(&entrypoint, AgentDiscoverySource::Configured, 0)],
        1024,
    );
    let first_installation = expect_single_installation("first", &first);
    let second_installation = expect_single_installation("second", &second);

    assert_eq!(
        first_installation.executable_path,
        second_installation.executable_path
    );
    assert_ne!(first_installation.id, second_installation.id);
    assert_ne!(
        first_installation.executable_revision,
        second_installation.executable_revision
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn target_retargeted_during_probe_is_rejected() {
    let _real_process_fixture_guard = real_process_fixture_guard();
    let root = unix_test_root("changed-during-probe");
    let entrypoint = root.join("codex");
    let first_target = root.join("target-a");
    let second_target = root.join("target-b");
    let started = root.join("probe-started");
    let release = root.join("probe-release");
    fs::create_dir_all(&root).unwrap();
    write_executable(&second_target, "#!/bin/sh\nprintf 'codex-cli 2.0.0\\n'\n");
    write_executable(
        &first_target,
        &synchronized_probe_script(&started, &release, "codex-cli 1.0.0"),
    );
    symlink(&first_target, &entrypoint).unwrap();

    let result = std::thread::scope(|scope| {
        let discovery = scope.spawn(|| {
            discover_real_candidates(
                vec![candidate(&entrypoint, AgentDiscoverySource::Configured, 0)],
                1024,
            )
        });
        wait_for_probe_start(&started);
        fs::remove_file(&entrypoint).unwrap();
        symlink(&second_target, &entrypoint).unwrap();
        fs::write(&release, b"release").unwrap();
        discovery.join().unwrap()
    });
    assert!(result.installations.is_empty());
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InstallationProbeFailed {
            error_kind: AgentExecutableProbeErrorKind::ChangedDuringProbe,
            ..
        }
    )));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn post_probe_canonicalization_failure_never_serializes_a_fallback_owner() {
    let _real_process_fixture_guard = real_process_fixture_guard();
    let root = unix_test_root("removed-during-probe");
    let entrypoint = root.join("codex");
    let target = root.join("target");
    let started = root.join("probe-started");
    let release = root.join("probe-release");
    fs::create_dir_all(&root).unwrap();
    write_executable(
        &target,
        &synchronized_probe_script(&started, &release, "codex-cli 1.0.0"),
    );
    symlink(&target, &entrypoint).unwrap();

    let result = std::thread::scope(|scope| {
        let discovery = scope.spawn(|| {
            discover_real_candidates(
                vec![candidate(&entrypoint, AgentDiscoverySource::Configured, 0)],
                1024,
            )
        });
        wait_for_probe_start(&started);
        fs::remove_file(&entrypoint).unwrap();
        fs::write(&release, b"release").unwrap();
        discovery.join().unwrap()
    });
    assert!(result.installations.is_empty());
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InstallationProbeFailed {
            error_kind: AgentExecutableProbeErrorKind::ChangedDuringProbe,
            ..
        }
    )));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn untruncated_stderr_can_supply_version_when_stdout_is_truncated() {
    let _real_process_fixture_guard = real_process_fixture_guard();
    let root = unix_test_root("mixed-output");
    let executable = root.join("codex");
    fs::create_dir_all(&root).unwrap();
    write_executable(
        &executable,
        "#!/bin/sh\nprintf '0123456789abcdef'\nprintf 'codex 1.0\\n' >&2\n",
    );

    let result = discover_real_candidates(
        vec![candidate(&executable, AgentDiscoverySource::Configured, 0)],
        12,
    );
    let installation = expect_single_installation("stderr_truncation", &result);
    assert_eq!(installation.installed_version.as_deref(), Some("codex 1.0"));
    assert!(installation.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::CliOutput,
            ..
        }
    )));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn fully_truncated_version_output_is_never_accepted() {
    let _real_process_fixture_guard = real_process_fixture_guard();
    let root = unix_test_root("truncated-version");
    let executable = root.join("codex");
    fs::create_dir_all(&root).unwrap();
    write_executable(
        &executable,
        "#!/bin/sh\nprintf 'stdout-version-too-long'\nprintf 'stderr-version-too-long' >&2\n",
    );

    let result = discover_real_candidates(
        vec![candidate(&executable, AgentDiscoverySource::Configured, 0)],
        8,
    );
    assert!(result.installations.is_empty());
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::CliOutput,
            accepted: 8,
            observed_at_least: 9,
        }
    )));
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InstallationProbeFailed {
            error_kind: AgentExecutableProbeErrorKind::InvalidVersion,
            ..
        }
    )));
    fs::remove_dir_all(root).unwrap();
}

fn no_asset_parse(
    _request: super::super::contracts::AgentAssetParseRequest<'_>,
    _output: &mut dyn super::super::contracts::AgentParseOutput,
) {
}

fn no_asset_contexts(
    _request: super::super::contracts::AgentContextDiscoveryRequest<'_>,
    _output: &mut dyn super::super::contracts::AgentDiagnosticOutput,
) -> Vec<AgentConfigurationContext> {
    Vec::new()
}

fn no_asset_sources(
    _request: super::super::contracts::AgentSourceDiscoveryRequest<'_>,
    _output: &mut dyn super::super::contracts::InitialSourceOutput,
) {
}

fn no_asset_resolve(
    _request: super::super::contracts::AgentAssetResolveRequest<'_>,
    _output: &mut dyn super::super::contracts::AgentResolveOutput,
) {
}

#[test]
fn claude_home_candidates_include_native_installer_path() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let executable = home.join(".claude/local/claude");
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::write(&executable, b"").unwrap();
    let candidates = (definition(AgentCliKind::ClaudeCode).home_scan)(
        AgentHomeCandidateScanRequest::legacy(home),
    )
    .candidates;
    assert!(candidates.contains(&executable));
}

#[test]
fn gemini_home_candidates_include_node_package_installations() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let installed =
        [".volta/bin/gemini", ".npm-global/bin/gemini"].map(|relative| home.join(relative));
    for path in &installed {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }
    let candidates =
        (definition(AgentCliKind::Gemini).home_scan)(AgentHomeCandidateScanRequest::legacy(home))
            .candidates;
    for path in installed {
        assert!(candidates.contains(&path));
    }
}

#[test]
fn grok_home_candidates_include_official_installer_path() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let installed = [".grok/bin/grok", ".grok/bin/grok.exe"].map(|relative| home.join(relative));
    for path in &installed {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }
    let candidates =
        (definition(AgentCliKind::Grok).home_scan)(AgentHomeCandidateScanRequest::legacy(home))
            .candidates;
    for path in installed {
        assert!(candidates.contains(&path));
    }
}

#[test]
fn cli_version_comparison_prefers_newer_versions() {
    assert_eq!(
        compare_version_keys(
            &numeric_version_key("codex-cli 0.144.4"),
            &numeric_version_key("codex-cli 0.99.0"),
        ),
        Ordering::Greater
    );
    assert_eq!(
        compare_version_keys(
            &numeric_version_key("2.1.10 (Claude Code)"),
            &numeric_version_key("2.1.9 (Claude Code)"),
        ),
        Ordering::Greater
    );
}

#[test]
fn only_version_manager_paths_are_eligible_for_automatic_path_migration() {
    assert!(preferred_path_is_version_managed(
        "/Users/example/.nvm/versions/node/v24.1.0/bin/codex"
    ));
    assert!(preferred_path_is_version_managed(
        "/Users/example/.local/share/fnm/node-versions/v24.1.0/installation/bin/claude"
    ));
    assert!(!preferred_path_is_version_managed(
        "/opt/homebrew/bin/codex"
    ));
    assert!(!preferred_path_is_version_managed("/tmp/custom/claude"));
}

#[test]
fn explicit_environment_path_stays_pinned_while_saved_version_path_can_migrate() {
    let saved = "/Users/example/.nvm/versions/node/v20.1.0/bin/codex";
    let environment = PathBuf::from("/opt/homebrew/bin/codex");
    let mut collector = CandidateCollector::new(2);
    collector.push(
        PathBuf::from(saved),
        AgentDiscoverySource::Configured,
        !preferred_path_is_version_managed(saved),
    );
    collector.push(environment, AgentDiscoverySource::Configured, true);

    assert!(!collector.candidates[0].fixed_priority);
    assert!(collector.candidates[1].fixed_priority);
}

#[test]
fn configured_candidate_explicitly_wins_a_shared_owner_group() {
    let candidates = vec![
        candidate("automatic-launcher", AgentDiscoverySource::Automatic, 0),
        candidate("configured-launcher", AgentDiscoverySource::Configured, 1),
    ];
    let limits = test_limits(2);
    let result = discover_installations_with(
        candidates,
        definition(AgentCliKind::Codex),
        &RuntimePathSnapshot::default(),
        &limits,
        Instant::now() + Duration::from_secs(1),
        &|candidate, _, _, _, _, _| Ok(fake_probed(candidate, "shared-owner", "1.0.0")),
    );

    assert_eq!(result.installations.len(), 1);
    assert_eq!(
        result.installations[0].executable_path.as_deref(),
        Some("configured-launcher")
    );
    assert_eq!(
        result.installations[0].discovery_source,
        AgentDiscoverySource::Configured
    );
}

#[test]
fn configured_owner_groups_sort_before_automatic_groups() {
    let candidates = vec![
        candidate("automatic-launcher", AgentDiscoverySource::Automatic, 0),
        candidate("configured-launcher", AgentDiscoverySource::Configured, 1),
    ];
    let limits = test_limits(2);
    let result = discover_installations_with(
        candidates,
        definition(AgentCliKind::Codex),
        &RuntimePathSnapshot::default(),
        &limits,
        Instant::now() + Duration::from_secs(1),
        &|candidate, _, _, _, _, _| {
            let owner = candidate.runtime_entrypoint.to_string_lossy();
            Ok(fake_probed(candidate, &owner, "1.0.0"))
        },
    );

    assert_eq!(result.installations.len(), 2);
    assert_eq!(
        result.installations[0].discovery_source,
        AgentDiscoverySource::Configured
    );
    assert_eq!(
        result.installations[1].discovery_source,
        AgentDiscoverySource::Automatic
    );
}

#[test]
fn configured_failure_does_not_hide_a_valid_automatic_installation() {
    let candidates = vec![
        candidate("configured-launcher", AgentDiscoverySource::Configured, 0),
        candidate("automatic-launcher", AgentDiscoverySource::Automatic, 1),
    ];
    let limits = test_limits(2);
    let result = discover_installations_with(
        candidates,
        definition(AgentCliKind::Codex),
        &RuntimePathSnapshot::default(),
        &limits,
        Instant::now() + Duration::from_secs(1),
        &|candidate, _, _, _, _, _| {
            if candidate.source == AgentDiscoverySource::Configured {
                Err(ProbeFailure {
                    kind: AgentExecutableProbeErrorKind::NotFound,
                    message: "missing".to_string(),
                    output_truncated: false,
                    budget_exceeded: false,
                })
            } else {
                Ok(fake_probed(candidate, "automatic-owner", "1.0.0"))
            }
        },
    );

    assert_eq!(result.installations.len(), 1);
    assert_eq!(
        result.installations[0].discovery_source,
        AgentDiscoverySource::Automatic
    );
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InstallationProbeFailed {
            candidate_source: AgentDiscoverySource::Configured,
            error_kind: AgentExecutableProbeErrorKind::NotFound,
        }
    )));
}

#[test]
fn mutable_version_evidence_changes_revision_but_not_installation_id() {
    let limits = test_limits(1);
    let run = |version: &str| {
        discover_installations_with(
            vec![candidate(
                "configured-launcher",
                AgentDiscoverySource::Configured,
                0,
            )],
            definition(AgentCliKind::Codex),
            &RuntimePathSnapshot::default(),
            &limits,
            Instant::now() + Duration::from_secs(1),
            &|candidate, _, _, _, _, _| Ok(fake_probed(candidate, "same-owner", version)),
        )
    };
    let first = run("1.0.0");
    let second = run("1.1.0");

    assert_eq!(first.installations[0].id, second.installations[0].id);
    assert_ne!(
        first.installations[0].executable_revision,
        second.installations[0].executable_revision
    );
    assert_ne!(
        first.installations[0].installed_version,
        second.installations[0].installed_version
    );
}

#[test]
fn owner_retarget_changes_installation_identity() {
    let limits = test_limits(1);
    let run = |owner: &str| {
        discover_installations_with(
            vec![candidate(
                "configured-launcher",
                AgentDiscoverySource::Configured,
                0,
            )],
            definition(AgentCliKind::Codex),
            &RuntimePathSnapshot::default(),
            &limits,
            Instant::now() + Duration::from_secs(1),
            &|candidate, _, _, _, _, _| Ok(fake_probed(candidate, owner, "1.0.0")),
        )
    };

    assert_ne!(
        run("owner-a").installations[0].id,
        run("owner-b").installations[0].id
    );
}

#[test]
fn successful_truncated_probe_attaches_typed_installation_diagnostic() {
    let mut limits = test_limits(1);
    limits.cli_output_bytes = 8;
    let result = discover_installations_with(
        vec![candidate(
            "configured-launcher",
            AgentDiscoverySource::Configured,
            0,
        )],
        definition(AgentCliKind::Codex),
        &RuntimePathSnapshot::default(),
        &limits,
        Instant::now() + Duration::from_secs(1),
        &|candidate, _, _, _, _, _| {
            let mut probed = fake_probed(candidate, "owner", "1.0.0");
            probed.output_truncated = true;
            Ok(probed)
        },
    );

    assert!(result.installations[0]
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::CliOutput,
                accepted: 8,
                observed_at_least: 9,
            }
        )));
}

#[test]
fn installation_limit_keeps_the_deterministic_prefix_and_reports_one_over() {
    let candidates = vec![
        candidate("configured", AgentDiscoverySource::Configured, 0),
        candidate("automatic-a", AgentDiscoverySource::Automatic, 1),
        candidate("automatic-b", AgentDiscoverySource::Automatic, 2),
    ];
    let mut limits = test_limits(2);
    limits.installations_per_agent = 2;
    let result = discover_installations_with(
        candidates,
        definition(AgentCliKind::Codex),
        &RuntimePathSnapshot::default(),
        &limits,
        Instant::now() + Duration::from_secs(1),
        &|candidate, _, _, _, _, _| Ok(fake_probed(candidate, &candidate.lexical_key, "1.0.0")),
    );

    assert_eq!(
        result
            .installations
            .iter()
            .map(|installation| installation.executable_path.as_deref().unwrap())
            .collect::<Vec<_>>(),
        vec!["configured", "automatic-a"]
    );
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::InstallationsPerAgent,
            accepted: 2,
            observed_at_least: 3,
        }
    )));
}

#[test]
fn per_discovery_worker_window_enforces_one_two_and_zero() {
    for (worker_limit, expected_max) in [(1, 1), (2, 2)] {
        let active = AtomicUsize::new(0);
        let maximum = AtomicUsize::new(0);
        let started = AtomicUsize::new(0);
        let candidates = (0..4)
            .map(|index| {
                candidate(
                    format!("automatic-{index}"),
                    AgentDiscoverySource::Automatic,
                    index,
                )
            })
            .collect();
        let limits = test_limits(worker_limit);
        let result = discover_installations_with(
            candidates,
            definition(AgentCliKind::Codex),
            &RuntimePathSnapshot::default(),
            &limits,
            Instant::now() + Duration::from_secs(2),
            &|candidate, _, _, _, _, _| {
                started.fetch_add(1, AtomicOrdering::SeqCst);
                let current = active.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                maximum.fetch_max(current, AtomicOrdering::SeqCst);
                std::thread::sleep(Duration::from_millis(25));
                active.fetch_sub(1, AtomicOrdering::SeqCst);
                Ok(fake_probed(candidate, &candidate.lexical_key, "1.0.0"))
            },
        );
        assert_eq!(result.installations.len(), 4);
        assert_eq!(started.load(AtomicOrdering::SeqCst), 4);
        assert_eq!(maximum.load(AtomicOrdering::SeqCst), expected_max);
    }

    let started = AtomicUsize::new(0);
    let result = discover_installations_with(
        vec![candidate("automatic", AgentDiscoverySource::Automatic, 0)],
        definition(AgentCliKind::Codex),
        &RuntimePathSnapshot::default(),
        &test_limits(0),
        Instant::now() + Duration::from_secs(1),
        &|candidate, _, _, _, _, _| {
            started.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(fake_probed(candidate, "owner", "1.0.0"))
        },
    );
    assert_eq!(started.load(AtomicOrdering::SeqCst), 0);
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::CliConcurrency,
            accepted: 0,
            observed_at_least: 1,
        }
    )));
}

#[test]
fn idle_worker_takes_the_next_candidate_instead_of_waiting_for_a_slow_lane() {
    let fast_candidate_started = AtomicBool::new(false);
    let slow_candidate_finished = AtomicBool::new(false);
    let third_started_before_slow_finished = AtomicBool::new(false);
    let candidates = (0..3)
        .map(|index| {
            candidate(
                format!("automatic-{index}"),
                AgentDiscoverySource::Automatic,
                index,
            )
        })
        .collect();

    let result = discover_installations_with(
        candidates,
        definition(AgentCliKind::Codex),
        &RuntimePathSnapshot::default(),
        &test_limits(2),
        Instant::now() + Duration::from_secs(2),
        &|candidate, _, _, _, _, _| {
            match candidate.ordinal {
                0 => {
                    let wait_deadline = Instant::now() + Duration::from_millis(500);
                    while !fast_candidate_started.load(AtomicOrdering::SeqCst) {
                        assert!(
                            Instant::now() < wait_deadline,
                            "second worker never started the next candidate"
                        );
                        std::thread::yield_now();
                    }
                    std::thread::sleep(Duration::from_millis(80));
                    slow_candidate_finished.store(true, AtomicOrdering::SeqCst);
                }
                1 => fast_candidate_started.store(true, AtomicOrdering::SeqCst),
                2 => {
                    third_started_before_slow_finished.store(
                        !slow_candidate_finished.load(AtomicOrdering::SeqCst),
                        AtomicOrdering::SeqCst,
                    );
                }
                _ => unreachable!(),
            }
            Ok(fake_probed(candidate, &candidate.lexical_key, "1.0.0"))
        },
    );

    assert_eq!(result.installations.len(), 3);
    assert!(third_started_before_slow_finished.load(AtomicOrdering::SeqCst));
}

#[test]
fn unbounded_input_limits_are_clamped_before_workers_and_probe_output() {
    let active = AtomicUsize::new(0);
    let maximum = AtomicUsize::new(0);
    let observed_output_limit = AtomicUsize::new(0);
    let candidates = (0..8)
        .map(|index| {
            candidate(
                format!("automatic-{index}"),
                AgentDiscoverySource::Automatic,
                index,
            )
        })
        .collect();
    let mut limits = test_limits(usize::MAX);
    limits.cli_output_bytes = usize::MAX;

    let result = discover_installations_with(
        candidates,
        definition(AgentCliKind::Codex),
        &RuntimePathSnapshot::default(),
        &limits,
        Instant::now() + Duration::from_secs(2),
        &|candidate, _, _, _, _, output_limit| {
            observed_output_limit.store(output_limit, AtomicOrdering::SeqCst);
            let current = active.fetch_add(1, AtomicOrdering::SeqCst) + 1;
            maximum.fetch_max(current, AtomicOrdering::SeqCst);
            std::thread::sleep(Duration::from_millis(25));
            active.fetch_sub(1, AtomicOrdering::SeqCst);
            Ok(fake_probed(candidate, &candidate.lexical_key, "1.0.0"))
        },
    );

    assert_eq!(result.installations.len(), 8);
    assert_eq!(maximum.load(AtomicOrdering::SeqCst), 4);
    assert_eq!(
        observed_output_limit.load(AtomicOrdering::SeqCst),
        AgentAssetLimits::HARD_CAP.cli_output_bytes
    );
}

#[test]
fn simultaneous_discoveries_share_the_same_hard_gate() {
    let gate = probe::CliProcessGate::new(4);
    let active = AtomicUsize::new(0);
    let maximum = AtomicUsize::new(0);
    let runner = |candidate: &CliCandidate,
                  _: &AgentCliDefinition,
                  _: &RuntimePathSnapshot,
                  deadline: Instant,
                  _: Duration,
                  _: usize| {
        let _permit = gate
            .acquire_until(deadline)
            .ok_or_else(ProbeFailure::budget_exceeded)?;
        let current = active.fetch_add(1, AtomicOrdering::SeqCst) + 1;
        maximum.fetch_max(current, AtomicOrdering::SeqCst);
        std::thread::sleep(Duration::from_millis(30));
        active.fetch_sub(1, AtomicOrdering::SeqCst);
        Ok(fake_probed(candidate, &candidate.lexical_key, "1.0.0"))
    };
    let limits = test_limits(4);
    let first = (0..4)
        .map(|index| {
            candidate(
                format!("first-{index}"),
                AgentDiscoverySource::Automatic,
                index,
            )
        })
        .collect::<Vec<_>>();
    let second = (0..4)
        .map(|index| {
            candidate(
                format!("second-{index}"),
                AgentDiscoverySource::Automatic,
                index,
            )
        })
        .collect::<Vec<_>>();
    let deadline = Instant::now() + Duration::from_secs(2);
    std::thread::scope(|scope| {
        let first_handle = scope.spawn(|| {
            discover_installations_with(
                first,
                definition(AgentCliKind::Codex),
                &RuntimePathSnapshot::default(),
                &limits,
                deadline,
                &runner,
            )
        });
        let second_handle = scope.spawn(|| {
            discover_installations_with(
                second,
                definition(AgentCliKind::Codex),
                &RuntimePathSnapshot::default(),
                &limits,
                deadline,
                &runner,
            )
        });
        assert_eq!(first_handle.join().unwrap().installations.len(), 4);
        assert_eq!(second_handle.join().unwrap().installations.len(), 4);
    });
    assert_eq!(maximum.load(AtomicOrdering::SeqCst), 4);
}

#[cfg(unix)]
#[test]
fn real_cli_probe_path_uses_the_shared_raii_gate() {
    let _real_process_fixture_guard = real_process_fixture_guard();
    let root = unix_test_root("probe-gate");
    let executable = root.join("codex");
    fs::create_dir_all(&root).unwrap();
    write_executable(
        &executable,
        "#!/bin/sh\nsleep 0.15\nprintf 'codex-cli 1.0.0\\n'\n",
    );
    let gate = probe::CliProcessGate::new(2);
    let monitoring = AtomicBool::new(true);
    let maximum = AtomicUsize::new(0);

    std::thread::scope(|scope| {
        let _monitoring_stop_guard = MonitoringStopGuard {
            monitoring: &monitoring,
        };
        let monitor = scope.spawn(|| {
            while monitoring.load(AtomicOrdering::SeqCst) {
                maximum.fetch_max(gate.active(), AtomicOrdering::SeqCst);
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        let handles = (0..6)
            .map(|_| {
                scope.spawn(|| {
                    probe::probe_cli_candidate_with_gate(
                        &executable,
                        definition(AgentCliKind::Codex),
                        &RuntimePathSnapshot::default(),
                        Instant::now() + REAL_PROCESS_SUCCESS_WATCHDOG,
                        REAL_PROCESS_SUCCESS_WATCHDOG,
                        1024,
                        &gate,
                    )
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }
        monitoring.store(false, AtomicOrdering::SeqCst);
        monitor.join().unwrap();
    });

    assert_eq!(maximum.load(AtomicOrdering::SeqCst), 2);
    assert_eq!(gate.active(), 0);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
struct MonitoringStopGuard<'a> {
    monitoring: &'a AtomicBool,
}

#[cfg(unix)]
impl Drop for MonitoringStopGuard<'_> {
    fn drop(&mut self) {
        self.monitoring.store(false, AtomicOrdering::SeqCst);
    }
}

#[cfg(unix)]
#[test]
fn runtime_snapshot_resolves_an_external_env_interpreter() {
    let _real_process_fixture_guard = real_process_fixture_guard();
    let root = unix_test_root("runtime-snapshot-interpreter");
    let launcher_dir = root.join("launcher");
    let runtime_dir = root.join("runtime");
    let launcher = launcher_dir.join("codex");
    let interpreter = runtime_dir.join("balancehub-test-node");
    fs::create_dir_all(&launcher_dir).unwrap();
    fs::create_dir_all(&runtime_dir).unwrap();
    write_executable(&interpreter, "#!/bin/sh\nprintf 'codex-cli 7.0.0\\n'\n");
    write_executable(&launcher, "#!/usr/bin/env balancehub-test-node\n");

    let snapshot = RuntimePathSnapshot::from_parts(vec![runtime_dir], None);
    let result = probe::probe_cli_candidate_with_gate(
        &launcher,
        definition(AgentCliKind::Codex),
        &snapshot,
        Instant::now() + REAL_PROCESS_SUCCESS_WATCHDOG,
        REAL_PROCESS_SUCCESS_WATCHDOG,
        1024,
        probe::global_cli_process_gate(),
    )
    .unwrap();

    assert_eq!(result.executable.version, "codex-cli 7.0.0");
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn legacy_runtime_snapshot_keeps_a_late_version_manager_interpreter() {
    let _real_process_fixture_guard = real_process_fixture_guard();
    let root = unix_test_root("legacy-runtime-late-interpreter");
    let launcher_dir = root.join("launcher");
    let launcher = launcher_dir.join("codex");
    fs::create_dir_all(&launcher_dir).unwrap();
    create_nvm_versions(&root, (0..13).map(|index| format!("v{index}.0.0")));
    let late_runtime = root.join(".nvm/versions/node/v0.0.0/bin");
    write_executable(
        &late_runtime.join("balancehub-test-node"),
        "#!/bin/sh\nprintf 'codex-cli 8.0.0\\n'\n",
    );
    write_executable(&launcher, "#!/usr/bin/env balancehub-test-node\n");

    let bounded = scan_bounded_gemini_home(
        &root,
        12,
        BTreeSet::new(),
        Instant::now() + REAL_PROCESS_SUCCESS_WATCHDOG,
    );
    assert!(!bounded.runtime_dirs.contains(&late_runtime));

    let snapshot = legacy_runtime_path_snapshot_for_home(&root, false);
    let legacy_path = snapshot
        .path_for(&launcher)
        .expect("legacy runtime snapshot PATH");
    assert!(env::split_paths(&legacy_path).any(|path| path == late_runtime));
    let result = probe::probe_cli_candidate_with_gate(
        &launcher,
        definition(AgentCliKind::Codex),
        &snapshot,
        Instant::now() + REAL_PROCESS_SUCCESS_WATCHDOG,
        REAL_PROCESS_SUCCESS_WATCHDOG,
        1024,
        probe::global_cli_process_gate(),
    )
    .unwrap();
    assert_eq!(result.executable.version, "codex-cli 8.0.0");
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn expired_deadline_releases_permit_without_spawning_cli() {
    let root = unix_test_root("probe-expired-before-spawn");
    let executable = root.join("codex");
    let marker = root.join("spawned");
    fs::create_dir_all(&root).unwrap();
    write_executable(
        &executable,
        &format!(
            "#!/bin/sh\nprintf spawned > '{}'\nprintf 'codex-cli 1.0.0\\n'\n",
            marker.display()
        ),
    );
    let gate = probe::CliProcessGate::new(1);

    let error = probe::probe_cli_candidate_with_gate(
        &executable,
        definition(AgentCliKind::Codex),
        &RuntimePathSnapshot::default(),
        Instant::now() - Duration::from_millis(1),
        CLI_VERSION_TIMEOUT,
        1024,
        &gate,
    )
    .unwrap_err();

    assert!(error.budget_exceeded);
    assert!(!marker.exists());
    assert_eq!(gate.active(), 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn deadline_while_waiting_for_gate_starts_no_probe() {
    let gate = probe::CliProcessGate::new(1);
    let held = gate
        .acquire_until(Instant::now() + Duration::from_secs(1))
        .unwrap();
    let started = AtomicUsize::new(0);
    let limits = test_limits(1);
    let result = discover_installations_with(
        vec![candidate("waiting", AgentDiscoverySource::Configured, 0)],
        definition(AgentCliKind::Codex),
        &RuntimePathSnapshot::default(),
        &limits,
        Instant::now() + Duration::from_millis(40),
        &|candidate, _, _, deadline, _, _| {
            let _permit = gate
                .acquire_until(deadline)
                .ok_or_else(ProbeFailure::budget_exceeded)?;
            started.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(fake_probed(candidate, "owner", "1.0.0"))
        },
    );
    drop(held);

    assert_eq!(started.load(AtomicOrdering::SeqCst), 0);
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::BudgetExceeded { .. })));
}

#[test]
fn completion_order_does_not_change_installation_order() {
    let run = || {
        let candidates = (0..4)
            .map(|index| {
                candidate(
                    format!("launcher-{index}"),
                    AgentDiscoverySource::Automatic,
                    index,
                )
            })
            .collect();
        discover_installations_with(
            candidates,
            definition(AgentCliKind::Codex),
            &RuntimePathSnapshot::default(),
            &test_limits(4),
            Instant::now() + Duration::from_secs(2),
            &|candidate, _, _, _, _, _| {
                std::thread::sleep(Duration::from_millis(
                    (4_usize.saturating_sub(candidate.ordinal) * 5) as u64,
                ));
                Ok(fake_probed(candidate, &candidate.lexical_key, "1.0.0"))
            },
        )
        .installations
        .into_iter()
        .map(|installation| installation.executable_path.unwrap())
        .collect::<Vec<_>>()
    };

    assert_eq!(run(), run());
    assert_eq!(
        run(),
        vec!["launcher-0", "launcher-1", "launcher-2", "launcher-3"]
    );
}

#[cfg(target_os = "windows")]
#[test]
fn windows_binary_names_include_cmd_and_exe() {
    assert_eq!(binary_names("codex"), ["codex", "codex.cmd", "codex.exe"]);
}

#[cfg(target_os = "windows")]
#[test]
fn windows_lexical_keys_normalize_extended_paths_case_and_parent_segments() {
    assert_eq!(
        lexical_comparison_key(Path::new(r"\\?\C:\Users\Example\bin\..\bin\CODEX.CMD")),
        lexical_comparison_key(Path::new(r"c:/users/example/bin/codex.cmd"))
    );
    assert_eq!(
        lexical_comparison_key(Path::new(r"\\?\UNC\Server\Share\Agent\GROK.EXE")),
        lexical_comparison_key(Path::new(r"\\server\share\agent\grok.exe"))
    );
}
