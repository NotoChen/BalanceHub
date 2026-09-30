//! Lifecycle fixtures never launch a real CLI.

mod native_execution;
mod state_machine;

use super::{
    filesystem::FileStamp,
    npm::{self, NpmRuntime},
    planning::LifecycleContext,
};
use crate::{models::*, services::agent_cli};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) struct Fixture {
    _temporary: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) home: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let temporary = tempfile::Builder::new()
            .prefix("bh-lifecycle-")
            .tempdir()
            .unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        Self {
            _temporary: temporary,
            root,
            home,
        }
    }

    pub(super) fn context(&self) -> LifecycleContext {
        LifecycleContext {
            home: self.home.clone(),
            settings: AppSettings::default(),
        }
    }

    pub(super) fn runtime(&self) -> NpmRuntime {
        let prefix = self.root.join("runtime");
        let bin = if cfg!(windows) {
            prefix.clone()
        } else {
            prefix.join("bin")
        };
        let node = bin.join(if cfg!(windows) { "node.exe" } else { "node" });
        write_program(&node, b"\x7fELFfixture-node-never-executed");
        let npm_root = if cfg!(windows) {
            prefix.join("node_modules/npm")
        } else {
            prefix.join("lib/node_modules/npm")
        };
        let npm_cli = npm_root.join("bin/npm-cli.js");
        write_program(
            &npm_cli,
            b"#!/usr/bin/env node\n// fixture npm is never executed\n",
        );
        let manifest_path = npm_root.join("package.json");
        fs::write(
            &manifest_path,
            br#"{"name":"npm","version":"11.0.0","bin":{"npm":"bin/npm-cli.js"}}"#,
        )
        .unwrap();
        let manifest = FileStamp::read(&manifest_path, 64 * 1024).unwrap().0;
        NpmRuntime {
            node,
            npm_cli,
            node_version: "24.0.0".to_owned(),
            manifest,
        }
    }

    pub(super) fn prefix(&self, kind: AgentCliKind) -> PathBuf {
        self.root.join("npm").join(kind.key())
    }
}

pub(super) fn write_program(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}

pub(super) fn write_package(prefix: &Path, kind: AgentCliKind, version: &str) -> PathBuf {
    let definition = agent_cli::definition(kind);
    let package = npm::package_root(prefix, kind);
    let relative = if kind == AgentCliKind::ClaudeCode {
        "bin/cli.exe"
    } else {
        "bin/cli.js"
    };
    let bin = package.join(relative);
    let contents = if kind == AgentCliKind::ClaudeCode {
        format!("\u{7f}ELFfixture-{version}")
    } else {
        format!("#!/usr/bin/env node\n// fixture {version} never executed\n")
    };
    write_program(&bin, contents.as_bytes());
    let manifest = serde_json::json!({
        "name": definition.environment().package_name(), "version": version,
        "bin": { (definition.executable): relative },
    });
    fs::write(
        package.join("package.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        let launcher = prefix.join("bin").join(definition.executable);
        fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        if fs::symlink_metadata(&launcher).is_ok() {
            fs::remove_file(&launcher).unwrap();
        }
        std::os::unix::fs::symlink(&bin, launcher).unwrap();
    }
    bin
}

pub(super) fn installation(
    kind: AgentCliKind,
    executable: &Path,
    version: &str,
) -> AgentInstallation {
    AgentInstallation {
        id: format!("fixture-installation-{}", kind.key()),
        environment_id: "fixture-environment".to_owned(),
        agent_kind: kind,
        label: agent_cli::definition(kind).label.to_owned(),
        availability: AgentInstallationAvailability::Available,
        executable_path: Some(executable.to_string_lossy().into_owned()),
        executable_identity: Some(AgentExecutableIdentity {
            owner: format!("fixture-owner-{}", kind.key()),
            canonical_path: executable
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            installation_source: AgentDiscoverySource::Configured,
        }),
        executable_revision: Some("fixture-executable-revision".to_owned()),
        installed_version: Some(version.to_owned()),
        discovery_source: AgentDiscoverySource::Configured,
        distribution: AgentCliDistribution::Npm,
        channel: AgentInstallationChannel::Stable,
        installed_version_source: AgentVersionSource::LocalExecutable,
        diagnostics: Vec::new(),
    }
}

pub(super) fn target(
    kind: AgentCliKind,
    prefix: &Path,
    installation: AgentInstallation,
) -> AgentLifecycleTarget {
    AgentLifecycleTarget {
        id: format!("fixture-target-{}", kind.key()),
        agent_kind: kind,
        label: agent_cli::definition(kind).label.to_owned(),
        actions: vec![AgentLifecycleAction {
            kind: AgentLifecycleActionKind::Upgrade,
            available: true,
            reason: None,
            reason_message: None,
        }],
        installation,
        is_current: true,
        channel: AgentLifecycleChannel::Npm,
        channel_label: "隔离 npm 夹具".to_owned(),
        release_track: Some("latest".to_owned()),
        directory: Some(prefix.to_string_lossy().into_owned()),
        evidence_revision: "fixture-evidence-revision".to_owned(),
        version: AgentLifecycleVersion {
            state: AgentLifecycleVersionState::NotChecked,
            source: AgentLifecycleVersionSource::NpmRegistry,
            latest_version: None,
            checked_at: None,
            last_success_at: None,
            next_check_at: None,
            stale: false,
            message: None,
        },
    }
}

pub(super) fn public_plan(target: &AgentLifecycleTarget, to_version: &str) -> AgentLifecyclePlan {
    AgentLifecyclePlan {
        plan_token: String::new(),
        agent_kind: target.agent_kind,
        target_id: target.id.clone(),
        installation_id: target.installation.id.clone(),
        action: target.actions[0].kind,
        channel: target.channel,
        channel_label: target.channel_label.clone(),
        directory: target.directory.clone().unwrap(),
        from_version: target.installation.installed_version.clone(),
        to_version: to_version.to_owned(),
        mechanism_id: "fixture-lifecycle".to_owned(),
        changes: vec!["隔离升级夹具".to_owned()],
        command_preview: Vec::new(),
        affected_installation_ids: vec![target.installation.id.clone()],
        confirmation_message: "确认隔离升级".to_owned(),
        cancellation_boundary: "提交前取消".to_owned(),
        timeout_seconds: 300,
        expires_at: String::new(),
    }
}

pub(super) fn plan_request(target: &AgentLifecycleTarget) -> AgentLifecyclePlanRequest {
    AgentLifecyclePlanRequest {
        agent_kind: target.agent_kind,
        target_id: target.id.clone(),
        action: target.actions[0].kind,
        expected_evidence_revision: target.evidence_revision.clone(),
    }
}

pub(super) fn apply_request(plan: &AgentLifecyclePlan) -> AgentLifecycleApplyRequest {
    AgentLifecycleApplyRequest {
        plan_token: plan.plan_token.clone(),
        agent_kind: plan.agent_kind,
        target_id: plan.target_id.clone(),
        action: plan.action,
    }
}

#[test]
fn lifecycle_requests_reject_caller_supplied_authority() {
    let plan = serde_json::json!({
        "agentKind":"codex", "targetId":"target", "action":"upgrade", "expectedEvidenceRevision":"revision",
    });
    for field in ["actor", "directory", "argv", "toVersion", "commandPreview"] {
        let mut invalid = plan.clone();
        invalid[field] = serde_json::json!("untrusted");
        assert!(serde_json::from_value::<AgentLifecyclePlanRequest>(invalid).is_err());
    }
    let apply = serde_json::json!({ "planToken":"opaque", "agentKind":"codex", "targetId":"target", "action":"upgrade" });
    for field in ["actor", "path", "argv", "changes", "toVersion"] {
        let mut invalid = apply.clone();
        invalid[field] = serde_json::json!("untrusted");
        assert!(serde_json::from_value::<AgentLifecycleApplyRequest>(invalid).is_err());
    }
    assert!(serde_json::from_value::<AgentLifecycleCatalogRequest>(
        serde_json::json!({"home":"untrusted"})
    )
    .is_err());
    assert!(serde_json::from_value::<AgentLifecyclePlanRequest>(plan).is_ok());
    assert!(serde_json::from_value::<AgentLifecycleApplyRequest>(apply).is_ok());
}
