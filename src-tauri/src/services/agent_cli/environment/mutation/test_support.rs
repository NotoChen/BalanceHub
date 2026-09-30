use super::super::verified_path::inspect_verified_path;
#[cfg(unix)]
use super::{ExactCliCommand, GuardedFile};
use super::{
    MutationExecution, MutationInspector, MutationInventory, MutationPreparation,
    MutationVerification, PreparedMutation,
};
use crate::models::*;
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::ffi::OsString;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

pub(crate) const CONFIG: &str = "enabled = true\nfixture_secret = 'private-fixture-value'\n";

#[derive(Clone)]
pub(crate) enum TestExecution {
    Atomic,
    #[cfg(unix)]
    Cli(Vec<String>),
}

pub(crate) struct TestInspector {
    pub(crate) root: PathBuf,
    pub(crate) executable: PathBuf,
    pub(crate) calls: AtomicUsize,
    pub(crate) failure_at: AtomicUsize,
    pub(crate) execution: Mutex<TestExecution>,
    settings: AppSettings,
}

impl TestInspector {
    pub(crate) fn new(root: &Path) -> Self {
        let root = root.canonicalize().unwrap();
        let executable = root.join("fixture-cli");
        fs::write(root.join("config.toml"), CONFIG).unwrap();
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self {
            root,
            executable,
            calls: AtomicUsize::new(0),
            failure_at: AtomicUsize::new(usize::MAX),
            execution: Mutex::new(TestExecution::Atomic),
            settings: AppSettings::default(),
        }
    }

    pub(crate) fn config(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub(crate) fn snapshot(&self) -> MutationInventory {
        let path = self.config();
        let bytes = fs::read(&path).ok();
        let revision = revision(bytes.as_deref());
        let mut source_anchors = BTreeMap::new();
        if let Some(bytes) = &bytes {
            let guard =
                inspect_verified_path(&[&self.root], &self.root, &path, AgentAssetSourceKind::File)
                    .unwrap();
            let observed = guard.read_file_bounded(64 * 1024).unwrap();
            assert_eq!(&observed, bytes);
            source_anchors.insert("source".into(), guard.anchor(&revision, Some(&observed)));
        }
        let mut inventory = inventory();
        inventory.sources[0].path = path.to_string_lossy().into_owned();
        inventory.sources[0].allowed_root = self.root.to_string_lossy().into_owned();
        inventory.sources[0].revision = revision.clone();
        inventory.contexts[0].config_root = self.root.to_string_lossy().into_owned();
        let installation = &mut inventory.installations[0];
        installation.executable_path = Some(self.executable.to_string_lossy().into_owned());
        installation
            .executable_identity
            .as_mut()
            .unwrap()
            .canonical_path = self.executable.to_string_lossy().into_owned();
        let state = if bytes
            .as_deref()
            .is_some_and(|bytes| String::from_utf8_lossy(bytes).contains("enabled = false"))
        {
            AgentAssetState::Disabled
        } else {
            AgentAssetState::Enabled
        };
        inventory.assets[0].path = Some(path.to_string_lossy().into_owned());
        inventory.assets[0].revision = revision;
        inventory.assets[0].declared_state = state;
        inventory.assets[0].effective_state = state;
        MutationInventory {
            inventory,
            source_anchors,
        }
    }

    #[cfg(unix)]
    pub(crate) fn request(&self, action: AgentAssetActionKind) -> AgentAssetPlanRequest {
        AgentAssetPlanRequest {
            asset_id: "asset".into(),
            action,
            workspace: None,
            expected_revision: self.snapshot().inventory.assets[0]
                .revision
                .identity
                .clone(),
            installation_id: Some("installation".into()),
        }
    }

    #[cfg(unix)]
    pub(crate) fn file(&self) -> GuardedFile {
        let snapshot = self.snapshot();
        GuardedFile::capture(&snapshot.inventory.sources[0], &snapshot.source_anchors).unwrap()
    }
}

impl MutationInspector for TestInspector {
    fn inspect(&self) -> Result<MutationInventory, AgentAssetMutationError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.failure_at.load(Ordering::SeqCst) {
            return Err(AgentAssetMutationError::new(
                AgentAssetMutationErrorKind::PreparationFailed,
            ));
        }
        Ok(self.snapshot())
    }
    fn home(&self) -> &Path {
        &self.root
    }
    fn workspace(&self) -> Option<&Path> {
        None
    }
    fn settings(&self) -> &AppSettings {
        &self.settings
    }
    fn prepare(
        &self,
        request: MutationPreparation<'_>,
    ) -> Result<PreparedMutation, AgentAssetMutationError> {
        let file = request.file(request.source("source")?)?;
        let execution = match &*self.execution.lock().unwrap() {
            TestExecution::Atomic => MutationExecution::AtomicFile {
                source_id: "source".into(),
                replacement: String::from_utf8_lossy(file.bytes().unwrap_or_default())
                    .replace(
                        if request.action == AgentAssetActionKind::Enable {
                            "enabled = false"
                        } else {
                            "enabled = true"
                        },
                        if request.action == AgentAssetActionKind::Enable {
                            "enabled = true"
                        } else {
                            "enabled = false"
                        },
                    )
                    .into_bytes(),
            },
            #[cfg(unix)]
            TestExecution::Cli(args) => {
                MutationExecution::ExactCli(Box::new(ExactCliCommand::new(
                    request.installation,
                    args.clone(),
                    &self.root,
                    vec![(OsString::from("HOME"), self.root.as_os_str().to_owned())],
                )?))
            }
        };
        PreparedMutation::new(
            request,
            execution,
            vec![file],
            vec!["source".into()],
            vec![request.boolean_change(&self.config(), "fixture state")],
            vec![],
            verify,
        )
    }
}

fn verify(request: MutationVerification<'_>) -> bool {
    request.effective_state_matches()
}

pub(crate) fn revision(bytes: Option<&[u8]>) -> AgentAssetRevision {
    AgentAssetRevision {
        identity: format!("{:x}", Sha256::digest(bytes.unwrap_or_default())),
        observed_at: String::new(),
        size_bytes: bytes.map(|bytes| bytes.len() as u64),
        is_missing: bytes.is_none(),
        is_directory: false,
        is_symlink: false,
    }
}

pub(crate) fn mechanism(action: AgentAssetActionKind) -> AgentAssetMechanismRecord {
    AgentAssetMechanismRecord {
        id: AgentMechanismId(format!("fixture-{action:?}")),
        agent_kind: AgentCliKind::Codex,
        category: AgentAssetCategory::Mcp,
        action,
        platforms: vec![AgentHostPlatform::Macos],
        scopes: vec![AgentAssetScope::User],
        adapter_schema_version: 1,
        source_schema: Some("test boolean".into()),
        executable_argv: vec![],
        inspection: "test inspect".into(),
        idempotent: true,
        commit_point: "test replace".into(),
        reload_effect: None,
        redaction_rules: vec!["no raw fixture value".into()],
    }
}

pub(crate) fn inventory() -> AgentEnvironmentInventory {
    let revision = revision(Some(CONFIG.as_bytes()));
    let installation = AgentInstallation {
        id: "installation".into(),
        environment_id: "environment".into(),
        agent_kind: AgentCliKind::Codex,
        label: "fixture".into(),
        availability: AgentInstallationAvailability::Available,
        executable_path: Some("/fixture/cli".into()),
        executable_identity: Some(AgentExecutableIdentity {
            owner: "fixture".into(),
            canonical_path: "/fixture/cli".into(),
            installation_source: AgentDiscoverySource::Configured,
        }),
        executable_revision: Some("fixture-executable".into()),
        installed_version: Some("1.2.3".into()),
        discovery_source: AgentDiscoverySource::Configured,
        distribution: AgentCliDistribution::Npm,
        channel: AgentInstallationChannel::Stable,
        installed_version_source: AgentVersionSource::LocalExecutable,
        diagnostics: vec![],
    };
    let source = AgentAssetSource {
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        id: "source".into(),
        context_id: "context".into(),
        label: "fixture source".into(),
        scope: AgentAssetScope::User,
        environment_id: "environment".into(),
        workspace_id: None,
        path: "/fixture/config.toml".into(),
        allowed_root: "/fixture".into(),
        precedence: 10,
        writable: true,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Mcp],
        revision: revision.clone(),
        diagnostics: vec![],
        access: AgentAssetAccess::default(),
        actions: vec![],
    };
    let actions = [AgentAssetActionKind::Enable, AgentAssetActionKind::Disable]
        .into_iter()
        .map(|action| AgentAssetAction {
            action,
            available: true,
            reason: None,
            mechanism_id: Some(mechanism(action).id.0),
            confirmation_required: true,
            reload_effect: None,
            trust_effect: None,
            selected_installation_id: Some("installation".into()),
            risks: vec![],
        })
        .collect();
    let asset = AgentAssetRecord {
        provenance: Vec::new(),
        stable_id: "asset".into(),
        agent_kind: AgentCliKind::Codex,
        category: AgentAssetCategory::Mcp,
        native_id: "fixture".into(),
        label: "fixture".into(),
        source_ids: vec!["source".into()],
        inspection_source_id: "source".into(),
        scope: AgentAssetScope::User,
        environment_id: "environment".into(),
        workspace_id: None,
        path: Some("/fixture/config.toml".into()),
        precedence: 10,
        writable: true,
        declared_state: AgentAssetState::Enabled,
        effective_state: AgentAssetState::Enabled,
        trust_state: AgentTrustState::Trusted,
        diagnostics: vec![],
        revision,
        sensitive: true,
        is_directory: false,
        context_id: "context".into(),
        represented_declaration_ids: vec!["definition".into()],
        resolution: AgentAssetResolution {
            relation: AgentAssetResolutionRelation::Independent,
            qualified_collision: false,
            terminal: None,
            contributor_ids: vec!["definition".into()],
            winner_id: None,
            control_source: None,
            diagnostics: vec![],
        },
        relationships: AgentAssetRelationships::default(),
        actions,
        access: AgentAssetAccess::default(),
        compatible_installation_ids: vec!["installation".into()],
        selected_action_installation_id: None,
        details: AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Stdio,
            declared_state: AgentAssetDeclaredState::Enabled,
            approval_state: AgentMcpApprovalState::NotRequired,
            effective_availability: AgentAssetEffectiveAvailability::Available,
        },
    };
    AgentEnvironmentInventory {
        environment: AgentEnvironmentDescriptor {
            id: "environment".into(),
            kind: AgentEnvironmentKind::Native,
            host_platform: AgentHostPlatform::Macos,
            host_architecture: AgentHostArchitecture::Aarch64,
            guest_platform: None,
            display_name: "fixture".into(),
            capabilities: vec![],
        },
        installations: vec![installation],
        sources: vec![source],
        capabilities: vec![],
        assets: vec![asset],
        hook_rule_counts: vec![],
        scanned_at: String::new(),
        workspace: None,
        contexts: vec![AgentConfigurationContext {
            id: "context".into(),
            environment_id: "environment".into(),
            agent_kind: AgentCliKind::Codex,
            config_root: "/fixture".into(),
            profile: "fixture".into(),
            workspace_id: None,
            trust_context: AgentTrustState::Trusted,
            parser_version: 1,
            schema_facts: BTreeMap::new(),
            compatible_installation_ids: vec!["installation".into()],
        }],
        declarations: vec![],
        limits: AgentAssetLimits::DEFAULT,
        diagnostics: vec![],
        mechanisms: vec![
            mechanism(AgentAssetActionKind::Enable),
            mechanism(AgentAssetActionKind::Disable),
        ],
    }
}
