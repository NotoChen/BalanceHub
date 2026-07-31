use super::*;

#[cfg(unix)]
use std::fs;
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
    let explicit = vec![environment.clone()];

    assert!(!candidate_has_fixed_priority(
        Path::new(saved),
        saved,
        true,
        &explicit,
    ));
    assert!(candidate_has_fixed_priority(
        &environment,
        saved,
        true,
        &explicit,
    ));
}

#[cfg(target_os = "windows")]
#[test]
fn windows_binary_names_include_cmd_and_exe() {
    assert_eq!(binary_names("codex"), ["codex", "codex.cmd", "codex.exe"]);
}
