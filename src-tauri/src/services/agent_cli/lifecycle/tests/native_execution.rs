#[cfg(unix)]
use super::super::filesystem::owned_writable_directory;
#[cfg(target_os = "macos")]
use super::super::native::vendor_installation;
use super::super::{
    filesystem::FileStamp,
    native::{LifecycleRunner, Mechanism, NativeExecution},
    npm::{self, NpmRuntime},
    planning::{LifecycleContext, LifecycleExecution, RunEvidence},
};
use super::{installation, target, write_package, write_program, Fixture};
use crate::{
    models::*,
    services::agent_cli::{
        self,
        environment::mutation::execution::{CliApplyEvidence, ExactCliCommand},
    },
};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

struct NpmFixtureRunner {
    kind: AgentCliKind,
    prefix: PathBuf,
    version: String,
    reported_version: Mutex<Option<String>>,
    calls: AtomicUsize,
}

impl NpmFixtureRunner {
    fn new(kind: AgentCliKind, prefix: PathBuf) -> Self {
        Self {
            kind,
            prefix,
            version: "2.0.0".to_owned(),
            reported_version: Mutex::new(None),
            calls: AtomicUsize::new(0),
        }
    }
}

impl LifecycleRunner for NpmFixtureRunner {
    fn run(
        &self,
        command: &ExactCliCommand,
        _settings: &AppSettings,
        preflight: &mut dyn FnMut() -> Result<(), AgentAssetMutationError>,
        commit: &mut dyn FnMut() -> Result<(), AgentAssetMutationError>,
    ) -> RunEvidence {
        if preflight().and_then(|_| commit()).is_err() {
            return RunEvidence {
                kind: CliApplyEvidence::RejectedBeforeSpawn,
                output_truncated: false,
            };
        }
        self.calls.fetch_add(1, Ordering::AcqRel);
        assert!(command
            .argv
            .windows(2)
            .any(|parts| parts == ["--prefix", self.prefix.to_str().unwrap()]));
        assert!(command.argv.contains(&format!(
            "{}@latest",
            agent_cli::definition(self.kind)
                .environment()
                .package_name()
        )));
        let environment = command
            .environment
            .iter()
            .cloned()
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            environment.get(&OsString::from("DISABLE_AUTOUPDATER")),
            Some(&OsString::from("1"))
        );
        for forbidden in [
            "NODE_OPTIONS",
            "NPM_CONFIG_PREFIX",
            "NPM_TOKEN",
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
        ] {
            assert!(!environment.contains_key(&OsString::from(forbidden)));
        }
        write_package(&self.prefix, self.kind, &self.version);
        RunEvidence {
            kind: CliApplyEvidence::ExitedSuccessfully,
            output_truncated: false,
        }
    }

    fn version(
        &self,
        _context: &LifecycleContext,
        executable: &Path,
        runtime: Option<&NpmRuntime>,
        _environment: &[(OsString, OsString)],
    ) -> Result<String, AgentLifecycleError> {
        assert!(runtime.is_some());
        let (owner, _) = npm::installed_package(&self.prefix, self.kind)?;
        assert_eq!(executable, owner.bin_path);
        Ok(self
            .reported_version
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(owner.version))
    }
}

fn prepared(
    fixture: &Fixture,
    kind: AgentCliKind,
) -> (NativeExecution, Arc<NpmFixtureRunner>, NpmRuntime) {
    let runtime = fixture.runtime();
    let prefix = fixture.prefix(kind);
    let executable = write_package(&prefix, kind, "1.0.0");
    let current = installation(kind, &executable, "1.0.0");
    let package = npm::installed_package(&prefix, kind).unwrap().1;
    let mechanism = Mechanism::Npm {
        prefix: prefix.clone(),
        runtime: Box::new(runtime.clone()),
        package: Box::new(package),
    };
    let runner = Arc::new(NpmFixtureRunner::new(kind, prefix.clone()));
    let execution = NativeExecution::prepare_with_runner(
        fixture.context(),
        &target(kind, &prefix, current),
        mechanism,
        "2.0.0",
        runner.clone(),
    )
    .unwrap();
    (execution, runner, runtime)
}

#[test]
fn prepared_npm_upgrade_revalidates_executable_script_manifest_and_package() {
    for changed_source in [
        "node",
        "npm-script",
        "npm-manifest",
        "agent-manifest",
        "agent-bin",
        "global-config",
    ] {
        let fixture = Fixture::new();
        let kind = AgentCliKind::Codex;
        let (execution, runner, runtime) = prepared(&fixture, kind);
        execution.revalidate().unwrap();
        let prefix = fixture.prefix(kind);
        let path = match changed_source {
            "node" => runtime.node,
            "npm-script" => runtime.npm_cli,
            "npm-manifest" => runtime
                .npm_cli
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("package.json"),
            "agent-manifest" => npm::package_root(&prefix, kind).join("package.json"),
            "agent-bin" => npm::installed_package(&prefix, kind).unwrap().0.bin_path,
            _ => prefix.join(".balancehub-empty.npmrc"),
        };
        fs::write(path, b"fixture drift after confirmation").unwrap();
        assert!(execution.revalidate().is_err(), "{changed_source}");
        let mut committed = false;
        let evidence = execution.run(&mut || {
            committed = true;
            Ok(())
        });
        assert_eq!(evidence.kind, CliApplyEvidence::RejectedBeforeSpawn);
        assert!(!committed);
        assert_eq!(runner.calls.load(Ordering::Acquire), 0);
    }
}

struct FailedNpmRunner;

impl LifecycleRunner for FailedNpmRunner {
    fn run(
        &self,
        _command: &ExactCliCommand,
        _settings: &AppSettings,
        preflight: &mut dyn FnMut() -> Result<(), AgentAssetMutationError>,
        commit: &mut dyn FnMut() -> Result<(), AgentAssetMutationError>,
    ) -> RunEvidence {
        RunEvidence {
            kind: if preflight().and_then(|_| commit()).is_ok() {
                CliApplyEvidence::ExitedUnsuccessfully
            } else {
                CliApplyEvidence::RejectedBeforeSpawn
            },
            output_truncated: false,
        }
    }

    fn version(
        &self,
        _context: &LifecycleContext,
        _executable: &Path,
        runtime: Option<&NpmRuntime>,
        _environment: &[(OsString, OsString)],
    ) -> Result<String, AgentLifecycleError> {
        assert!(runtime.is_some());
        Ok("1.0.0".to_owned())
    }
}

#[test]
fn failed_installer_with_unchanged_entrypoint_does_not_claim_all_dependencies_unchanged() {
    let fixture = Fixture::new();
    let kind = AgentCliKind::Codex;
    let runtime = fixture.runtime();
    let prefix = fixture.prefix(kind);
    let executable = write_package(&prefix, kind, "1.0.0");
    let current = installation(kind, &executable, "1.0.0");
    let mechanism = Mechanism::Npm {
        prefix: prefix.clone(),
        runtime: Box::new(runtime),
        package: Box::new(npm::installed_package(&prefix, kind).unwrap().1),
    };
    let execution = NativeExecution::prepare_with_runner(
        fixture.context(),
        &target(kind, &prefix, current),
        mechanism,
        "2.0.0",
        Arc::new(FailedNpmRunner),
    )
    .unwrap();
    assert!(execution.verify().unchanged);
    assert_eq!(
        execution.run(&mut || Ok(())).kind,
        CliApplyEvidence::ExitedUnsuccessfully
    );
    let observed = execution.verify();
    assert_eq!(observed.version.as_deref(), Some("1.0.0"));
    assert!(!observed.changed && !observed.unchanged);
}

#[test]
fn npm_version_verification_requires_matching_manifest_and_named_bin() {
    let fixture = Fixture::new();
    let kind = AgentCliKind::Codex;
    let (execution, runner, _) = prepared(&fixture, kind);
    execution.run(&mut || Ok(()));
    *runner.reported_version.lock().unwrap() = Some("9.9.9".to_owned());
    let mismatched = execution.verify();
    assert!(mismatched.version.is_none() && mismatched.executable_path.is_none());
    let manifest = npm::package_root(&fixture.prefix(kind), kind).join("package.json");
    fs::write(
        manifest,
        br#"{"name":"@openai/codex","version":"2.0.0","bin":{"other":"bin/cli.js"}}"#,
    )
    .unwrap();
    assert!(npm::installed_package(&fixture.prefix(kind), kind).is_err());
}

#[test]
fn runtime_candidates_skip_unreadable_wrappers_missing_npm_and_failed_probes() {
    let first = Fixture::new();
    let second = Fixture::new();
    let first_runtime = first.runtime();
    let second_runtime = second.runtime();
    let wrapper = first.root.join("wrapper/node");
    write_program(&wrapper, b"#!/bin/sh\nexit 1\n");
    let no_npm = first.root.join("no-npm/bin/node");
    write_program(&no_npm, b"\x7fELFfixture-without-npm");
    let candidates = vec![
        first.root.join("missing-node"),
        wrapper,
        no_npm,
        first_runtime.node.clone(),
        second_runtime.node.clone(),
    ];
    let mut probes = 0;
    let selected = NpmRuntime::discover_candidates(&first.context(), candidates, |command, _| {
        probes += 1;
        assert_eq!(command.argv, ["--version"]);
        if command.executable_path() == first_runtime.node {
            None
        } else {
            Some("24.1.0".to_owned())
        }
    })
    .unwrap();
    assert_eq!(selected.node, second_runtime.node);
    assert_eq!(selected.node_version, "24.1.0");
    assert_eq!(probes, 2);
}

#[cfg(unix)]
#[test]
fn substituted_prefix_ancestor_and_cache_links_are_rejected_without_writes() {
    for cache in [false, true] {
        let fixture = Fixture::new();
        let kind = AgentCliKind::Codex;
        let (execution, runner, _) = prepared(&fixture, kind);
        let prefix = fixture.prefix(kind);
        let outside = fixture.root.join("unselected");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"unchanged").unwrap();
        let replaced = if cache {
            prefix.join(".balancehub-npm-cache")
        } else {
            fs::rename(&prefix, fixture.root.join("original-prefix")).unwrap();
            prefix.clone()
        };
        std::os::unix::fs::symlink(&outside, replaced).unwrap();
        let mut committed = false;
        let evidence = execution.run(&mut || {
            committed = true;
            Ok(())
        });
        assert_eq!(evidence.kind, CliApplyEvidence::RejectedBeforeSpawn);
        assert!(!committed && runner.calls.load(Ordering::Acquire) == 0);
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"unchanged");
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    }
}

#[cfg(unix)]
#[test]
fn npm_launcher_must_still_resolve_to_the_exact_named_package_bin() {
    let fixture = Fixture::new();
    let kind = AgentCliKind::Codex;
    let (execution, _, _) = prepared(&fixture, kind);
    execution.run(&mut || Ok(()));
    let launcher = fixture.prefix(kind).join("bin/codex");
    fs::remove_file(&launcher).unwrap();
    let other = fixture.root.join("other-cli");
    write_program(&other, b"#!/usr/bin/env node\n// different owner\n");
    std::os::unix::fs::symlink(other, launcher).unwrap();
    let verified = execution.verify();
    assert_eq!(verified.version.as_deref(), Some("2.0.0"));
    assert!(verified.executable_path.is_none());
}

#[cfg(unix)]
#[test]
fn owned_prefix_requires_owner_write_and_search_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let prefix = fixture.prefix(AgentCliKind::Codex);
    fs::create_dir_all(&prefix).unwrap();
    fs::set_permissions(&prefix, fs::Permissions::from_mode(0o540)).unwrap();
    assert!(!owned_writable_directory(&prefix));
    fs::set_permissions(&prefix, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(owned_writable_directory(&prefix));
}

#[test]
fn native_version_sources_never_substitute_npm_release_urls() {
    let fixture = Fixture::new();
    let vendor = Mechanism::Vendor {
        directory: fixture.home.clone(),
        launcher: fixture.home.join("launcher"),
        track: "stable".to_owned(),
        receipts: Vec::new(),
        absent_receipts: Vec::new(),
    };
    assert_eq!(
        vendor.release_url(AgentCliKind::ClaudeCode).as_deref(),
        Some("https://downloads.claude.ai/claude-code-releases/stable")
    );
    assert_eq!(
        vendor.release_url(AgentCliKind::Grok).as_deref(),
        Some("https://x.ai/cli/stable")
    );
    assert!(vendor.release_url(AgentCliKind::Codex).is_none());
    let prefix = fixture.prefix(AgentCliKind::Codex);
    write_package(&prefix, AgentCliKind::Codex, "1.0.0");
    let npm = Mechanism::Npm {
        package: Box::new(
            npm::installed_package(&prefix, AgentCliKind::Codex)
                .unwrap()
                .1,
        ),
        prefix,
        runtime: Box::new(fixture.runtime()),
    };
    assert!(npm.release_url(AgentCliKind::Codex).is_none());
}

struct NativeVersionFixtureRunner;

impl LifecycleRunner for NativeVersionFixtureRunner {
    fn run(
        &self,
        _command: &ExactCliCommand,
        _settings: &AppSettings,
        _preflight: &mut dyn FnMut() -> Result<(), AgentAssetMutationError>,
        _commit: &mut dyn FnMut() -> Result<(), AgentAssetMutationError>,
    ) -> RunEvidence {
        panic!("this native receipt fixture must not execute an updater")
    }

    fn version(
        &self,
        _context: &LifecycleContext,
        _executable: &Path,
        runtime: Option<&NpmRuntime>,
        _environment: &[(OsString, OsString)],
    ) -> Result<String, AgentLifecycleError> {
        assert!(runtime.is_none());
        Ok("3.0.0".to_owned())
    }
}

fn prepared_vendor(
    fixture: &Fixture,
    kind: AgentCliKind,
) -> (NativeExecution, PathBuf, PathBuf, PathBuf) {
    let directory = fixture.home.join(format!("native-{}", kind.key()));
    let launcher = directory
        .join("bin")
        .join(agent_cli::definition(kind).executable);
    write_program(&launcher, b"\x7fELFfixture-native-never-executed");
    let receipt = directory.join("receipt.json");
    fs::write(&receipt, br#"{"fixture":"native"}"#).unwrap();
    let absent = directory.join("optional-settings.json");
    let mut selected = installation(kind, &launcher, "2.1.270");
    selected.distribution = AgentCliDistribution::VendorNative;
    let mut target = target(kind, &directory, selected);
    target.channel = AgentLifecycleChannel::VendorNative;
    let mechanism = Mechanism::Vendor {
        directory,
        launcher: launcher.clone(),
        track: "stable".to_owned(),
        receipts: vec![FileStamp::read(&receipt, 1024).unwrap().0],
        absent_receipts: vec![absent.clone()],
    };
    let execution = NativeExecution::prepare_with_runner(
        fixture.context(),
        &target,
        mechanism,
        "3.0.0",
        Arc::new(NativeVersionFixtureRunner),
    )
    .unwrap();
    (execution, launcher, receipt, absent)
}

#[test]
fn native_preparation_pins_exact_vendor_argv_receipts_and_absence() {
    for kind in [AgentCliKind::ClaudeCode, AgentCliKind::Grok] {
        for change_absent in [false, true] {
            let fixture = Fixture::new();
            let (execution, launcher, receipt, absent) = prepared_vendor(&fixture, kind);
            let preview = execution.command_preview();
            assert_eq!(Path::new(&preview[0]), launcher);
            assert_eq!(&preview[1..], &["update"]);
            execution.revalidate().unwrap();
            let changed = if change_absent { absent } else { receipt };
            fs::write(changed, b"fixture confirmation drift").unwrap();
            assert!(execution.revalidate().is_err());
        }
    }
}

#[cfg(unix)]
#[test]
fn native_verification_refuses_a_launcher_redirected_outside_its_planned_tree() {
    let fixture = Fixture::new();
    let (execution, launcher, _, _) = prepared_vendor(&fixture, AgentCliKind::Grok);
    assert_eq!(execution.verify().version.as_deref(), Some("3.0.0"));
    fs::remove_file(&launcher).unwrap();
    let outside = fixture.root.join("different-installation");
    write_program(&outside, b"\x7fELFfixture-unselected");
    std::os::unix::fs::symlink(outside, launcher).unwrap();
    let verified = execution.verify();
    assert!(verified.version.is_none() && verified.executable_path.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn native_receipts_pin_the_proven_install_layout() {
    let fixture = Fixture::new();
    let directory = fixture.home.join(".grok");
    let binary = directory.join("downloads/grok-1.0.24-fixture");
    write_program(&binary, b"\x7fELFfixture-vendor");
    fs::create_dir_all(directory.join("bin")).unwrap();
    let launcher = directory.join("bin/grok");
    std::os::unix::fs::symlink(&binary, &launcher).unwrap();
    let receipt = directory.join("config.toml");
    fs::write(
        &receipt,
        "[cli]\ninstaller = \"internal\"\nchannel = \"stable\"\n",
    )
    .unwrap();
    let mut selected = installation(AgentCliKind::Grok, &launcher, "1.0.24");
    selected.distribution = AgentCliDistribution::VendorNative;
    let mechanism = vendor_installation(&fixture.context(), &selected).unwrap();
    assert_eq!(mechanism.directory(), directory);
    fs::write(&receipt, "[cli]\ninstaller = \"npm\"\n").unwrap();
    assert!(vendor_installation(&fixture.context(), &selected).is_err());
    fs::write(
        &receipt,
        "[cli]\ninstaller = \"internal\"\nchannel = \"unknown\"\n",
    )
    .unwrap();
    assert!(vendor_installation(&fixture.context(), &selected).is_err());
}

#[test]
fn file_stamp_binds_exact_bytes_even_when_json_values_are_equivalent() {
    let fixture = Fixture::new();
    let path = fixture.root.join("receipt.json");
    fs::write(&path, br#"{"installMethod":"native"}"#).unwrap();
    let stamp = FileStamp::read(&path, 1024).unwrap().0;
    fs::write(path, b"{ \"installMethod\": \"native\" }\n").unwrap();
    assert!(stamp.revalidate().is_err());
}
