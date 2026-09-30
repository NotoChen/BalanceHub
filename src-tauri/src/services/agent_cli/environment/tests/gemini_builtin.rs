//! Real bounded inventory coverage for installation-relative Gemini Skills.
use super::super::{snapshot::revision_for_missing, verified_path::VerifiedPathAnchor};
use super::*;
use crate::{
    models::{
        AgentAssetDiscoveryIncompleteReason, AgentAssetInstallationOrigin,
        AgentAssetProviderOrigin, AgentAssetProvision, AgentAssetRecord, AgentCliDistribution,
        AgentConfigurationContext,
    },
    services::agent_cli::{
        catalog::CatalogService,
        contracts::AgentContextDiscovery,
        environment::mutation::{MutationInventory, MutationService},
    },
};

struct IsolatedBuiltinSnapshots<'a> {
    root: &'a Path,
    real: RealSnapshotPort,
    files: Mutex<Vec<PathBuf>>,
}

impl SnapshotPort for IsolatedBuiltinSnapshots<'_> {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        if !request.source.path.starts_with(self.root) {
            // Never read the machine's managed Gemini configuration.
            assert!(matches!(
                request.source.native_source_key.as_str(),
                "system-defaults" | "system-settings"
            ));
            return AgentAssetSnapshot::Missing {
                revision: revision_for_missing(&request.source.path),
            };
        }
        let path = request.source.path.clone();
        let snapshot = self.real.snapshot(request, run);
        if matches!(snapshot, AgentAssetSnapshot::File { .. }) {
            self.files.lock().unwrap().push(path);
        }
        snapshot
    }

    fn access_anchor(&self, revision: &AgentAssetRevision) -> Option<VerifiedPathAnchor> {
        self.real.access_anchor(revision)
    }
}

fn fixture_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Vec<AgentConfigurationContext> {
    let mut contexts = definition(AgentCliKind::Gemini)
        .environment()
        .discover_contexts(request, output);
    for context in &mut contexts {
        context.config_root = request.home.join(".gemini").to_string_lossy().into_owned();
    }
    contexts
}

struct BuiltinFixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    package: PathBuf,
}

impl BuiltinFixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let home = root.join("home");
        let package = root.join("tools/lib/node_modules/@google/gemini-cli");
        fs::create_dir_all(home.join(".gemini")).unwrap();
        fs::create_dir_all(package.join("bundle/builtin")).unwrap();
        fs::write(home.join(".gemini/settings.json"), b"{}\n").unwrap();
        fs::write(
            package.join("bundle/gemini.js"),
            b"// Synthetic executable metadata fixture. Never execute.\n",
        )
        .unwrap();
        let fixture = Self {
            _temporary: temporary,
            root,
            home,
            package,
        };
        fixture.write_package("@google/gemini-cli", "0.60.0", "bundle/gemini.js");
        fixture
    }

    fn write_package(&self, name: &str, version: &str, bin: &str) {
        fs::write(
            self.package.join("package.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": name,
                "version": version,
                "bin": { "gemini": bin },
                "files": ["bundle/"]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn installation(&self) -> AgentInstallation {
        let executable = self.package.join("bundle/gemini.js");
        let mut installation = fixture_installation();
        installation.id = "installation:gemini-builtin-fixture".to_owned();
        installation.agent_kind = AgentCliKind::Gemini;
        installation.label = "Synthetic Gemini".to_owned();
        installation.installed_version = Some("0.60.0".to_owned());
        installation.distribution = AgentCliDistribution::Npm;
        installation.executable_path = Some(executable.to_string_lossy().into_owned());
        installation.executable_identity = Some(AgentExecutableIdentity {
            owner: "gemini-builtin-fixture".to_owned(),
            canonical_path: executable.to_string_lossy().into_owned(),
            installation_source: AgentDiscoverySource::Configured,
        });
        installation
    }

    fn scan(&self, installations: Vec<AgentInstallation>) -> (MutationInventory, Vec<PathBuf>) {
        self.scan_with_contexts(installations, fixture_contexts)
    }

    fn scan_with_contexts(
        &self,
        installations: Vec<AgentInstallation>,
        discover_contexts: AgentContextDiscovery,
    ) -> (MutationInventory, Vec<PathBuf>) {
        let native = definition(AgentCliKind::Gemini);
        let definitions = [AgentCliDefinition {
            environment: native.environment.with_test_contexts(discover_contexts),
            ..*native
        }];
        let (installations, _) = FakeInstallationPort::new(installations);
        let snapshots = IsolatedBuiltinSnapshots {
            root: &self.root,
            real: RealSnapshotPort::default(),
            files: Mutex::new(Vec::new()),
        };
        let inventory = build_inventory_with(
            InventoryInput {
                home: &self.home,
                workspace: None,
                settings: Some(&crate::models::AppSettings::default()),
            },
            InventoryPipelineDeps {
                definitions: &definitions,
                limits: AgentAssetLimits::DEFAULT,
                clock: Arc::new(ManualClock::new()),
                installations: &installations,
                snapshots: &snapshots,
                checkpoint_probe: None,
            },
        )
        .unwrap();
        let source_anchors = inventory
            .sources
            .iter()
            .filter_map(|source| {
                snapshots
                    .access_anchor(&source.revision)
                    .map(|anchor| (source.id.clone(), anchor))
            })
            .collect();
        (
            MutationInventory {
                inventory,
                source_anchors,
            },
            snapshots.files.into_inner().unwrap(),
        )
    }

    fn builtin(&self, name: &str) -> PathBuf {
        self.package.join("bundle/builtin").join(name)
    }
}

fn write_skill(directory: &Path, name: &str) {
    fs::create_dir_all(directory).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Synthetic fixture\n---\nFixture only.\n"),
    )
    .unwrap();
}

fn skills(snapshot: &MutationInventory) -> Vec<&AgentAssetRecord> {
    snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .collect()
}

fn skill_discovery_reasons(
    snapshot: &MutationInventory,
) -> Vec<AgentAssetDiscoveryIncompleteReason> {
    snapshot
        .inventory
        .diagnostics
        .iter()
        .filter_map(|diagnostic| match diagnostic {
            AgentAssetDiagnostic::DiscoveryIncomplete {
                agent_kind: AgentCliKind::Gemini,
                category: AgentAssetCategory::Skill,
                reason,
            } => Some(*reason),
            _ => None,
        })
        .collect()
}

#[test]
fn gemini_builtin_inventory_and_catalog_use_the_verified_package_outside_home() {
    let fixture = BuiltinFixture::new();
    for name in ["bundled-one", "bundled-two"] {
        write_skill(&fixture.builtin(name), name);
    }
    write_skill(&fixture.home.join(".gemini/builtin/decoy"), "home-decoy");
    write_skill(
        &fixture.package.join("bundle/sibling/decoy"),
        "package-decoy",
    );
    let (snapshot, read_paths) = fixture.scan(vec![fixture.installation()]);
    assert!(skill_discovery_reasons(&snapshot).is_empty());
    let assets = skills(&snapshot);
    assert_eq!(assets.len(), 2, "{:#?}", snapshot.inventory.diagnostics);
    assert_eq!(
        assets
            .iter()
            .map(|asset| asset.native_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["bundled-one", "bundled-two"])
    );
    let sources = snapshot
        .inventory
        .sources
        .iter()
        .filter(|source| source.origin == AgentAssetInstallationOrigin::Bundled)
        .collect::<Vec<_>>();
    assert_eq!(sources.len(), 3, "one package directory plus two manifests");
    for source in sources {
        assert!(Path::new(&source.path).starts_with(fixture.package.join("bundle/builtin")));
        assert!(!Path::new(&source.path).starts_with(&fixture.home));
        assert_eq!(source.scope, AgentAssetScope::System);
        assert!(!source.writable);
        assert!(snapshot.source_anchors.contains_key(&source.id));
    }
    for asset in assets {
        assert_eq!(asset.scope, AgentAssetScope::System);
        assert_eq!(asset.effective_state, AgentAssetState::Enabled);
        assert_eq!(asset.provenance.len(), 1);
        let provenance = &asset.provenance[0];
        assert_eq!(provenance.source_id, asset.inspection_source_id);
        assert_eq!(provenance.provision, AgentAssetProvision::AgentBuiltIn);
        assert_eq!(
            provenance.installation,
            AgentAssetInstallationOrigin::Bundled
        );
        assert_eq!(provenance.provider, AgentAssetProviderOrigin::AgentVendor);
        assert!(!asset.writable);
    }
    assert!(!read_paths
        .iter()
        .any(|path| path.to_string_lossy().contains("decoy")));
    let service = CatalogService::new(
        fixture.root.join("library"),
        Arc::new(MutationService::default()),
    );
    let catalog = service.catalog(&snapshot).unwrap();
    let catalog_skills = catalog
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(catalog_skills.len(), 2);
    for asset in catalog_skills {
        assert_eq!(asset.bindings.len(), 1);
        assert_eq!(asset.variants.len(), 1);
        assert!(asset.variants[0].complete);
        assert_eq!(
            asset.provenance.provisions,
            [AgentAssetProvision::AgentBuiltIn]
        );
        assert_eq!(
            asset.provenance.installations,
            [AgentAssetInstallationOrigin::Bundled]
        );
    }
    let (second, _) = fixture.scan(vec![fixture.installation()]);
    let stable_ids = |scan: &MutationInventory| {
        skills(scan)
            .iter()
            .map(|asset| asset.stable_id.clone())
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(stable_ids(&snapshot), stable_ids(&second));
    let second_catalog = service.catalog(&second).unwrap();
    assert_eq!(
        catalog
            .assets
            .iter()
            .map(|asset| &asset.id)
            .collect::<BTreeSet<_>>(),
        second_catalog
            .assets
            .iter()
            .map(|asset| &asset.id)
            .collect::<BTreeSet<_>>()
    );
}

#[test]
fn gemini_builtin_skill_yields_to_extension_then_user_with_own_provenance() {
    let fixture = BuiltinFixture::new();
    write_skill(&fixture.builtin("shared"), "shared");
    let extension = fixture.home.join(".gemini/extensions/fixture-extension");
    write_skill(&extension.join("skills/shared"), "shared");
    fs::write(
        extension.join("gemini-extension.json"),
        br#"{"name":"fixture-extension","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(
        fixture.home.join(".gemini/extension-enablement.json"),
        br#"{"fixture-extension":{"overrides":[]}}"#,
    )
    .unwrap();
    let (extension_snapshot, _) = fixture.scan(vec![fixture.installation()]);
    let extension_skills = skills(&extension_snapshot);
    assert_eq!(
        extension_skills.len(),
        2,
        "{:#?}",
        extension_snapshot.inventory.diagnostics
    );
    let winner = extension_skills
        .iter()
        .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
        .unwrap();
    assert_eq!(winner.precedence, 1);
    assert!(winner.relationships.provided_by.is_some());
    assert_eq!(winner.effective_state, AgentAssetState::Enabled);

    write_skill(&fixture.home.join(".gemini/skills/shared"), "shared");
    let (user_snapshot, _) = fixture.scan(vec![fixture.installation()]);
    let user_skills = skills(&user_snapshot);
    assert_eq!(
        user_skills.len(),
        3,
        "{:#?}",
        user_snapshot.inventory.diagnostics
    );
    let winner = user_skills
        .iter()
        .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
        .unwrap();
    assert_eq!(winner.scope, AgentAssetScope::User);
    assert_eq!(winner.precedence, 10);
    assert!(winner.relationships.provided_by.is_none());
    let own = winner
        .provenance
        .iter()
        .find(|value| value.source_id == winner.inspection_source_id)
        .unwrap();
    assert_eq!(own.installation, AgentAssetInstallationOrigin::LocalFiles);
    assert_eq!(own.provider, AgentAssetProviderOrigin::Unknown);
    let builtin = user_skills
        .iter()
        .find(|asset| asset.scope == AgentAssetScope::System)
        .unwrap();
    assert_eq!(
        builtin.resolution.relation,
        AgentAssetResolutionRelation::Replaced
    );
    assert_eq!(builtin.provenance.len(), 1);
    assert_eq!(
        builtin.provenance[0].installation,
        AgentAssetInstallationOrigin::Bundled
    );
}

#[test]
fn gemini_builtin_discovery_rejects_unverified_installations_without_hiding_user_skills() {
    use AgentAssetDiscoveryIncompleteReason::{
        InstallationUnverified, SourceUnavailable, UnsupportedEntryPoint,
    };

    fn incompatible_contexts(
        request: AgentContextDiscoveryRequest<'_>,
        output: &mut dyn AgentDiagnosticOutput,
    ) -> Vec<AgentConfigurationContext> {
        let mut contexts = fixture_contexts(request, output);
        for context in &mut contexts {
            context.compatible_installation_ids.clear();
        }
        contexts
    }

    for (scenario, expected_reason) in [
        ("missing-version", Some(InstallationUnverified)),
        ("version-mismatch", Some(InstallationUnverified)),
        ("package-name", Some(InstallationUnverified)),
        ("bin-mismatch", Some(InstallationUnverified)),
        ("missing-identity", Some(InstallationUnverified)),
        ("relative-identity", Some(InstallationUnverified)),
        ("unknown-layout", Some(UnsupportedEntryPoint)),
        ("missing-builtin", Some(SourceUnavailable)),
        ("builtin-is-file", Some(SourceUnavailable)),
        ("unavailable", None),
        ("unknown-channel", Some(InstallationUnverified)),
        ("unsupported-distribution", Some(UnsupportedEntryPoint)),
        ("no-installation", None),
        ("other-agent", None),
        ("incompatible-context", None),
        ("incompatible-unverified", None),
    ] {
        let fixture = BuiltinFixture::new();
        write_skill(&fixture.builtin("must-not-load"), "must-not-load");
        write_skill(
            &fixture.home.join(".gemini/skills/user-skill"),
            "user-skill",
        );
        let mut installation = fixture.installation();
        match scenario {
            "missing-version" => installation.installed_version = None,
            "version-mismatch" => {
                fixture.write_package("@google/gemini-cli", "0.58.0", "bundle/gemini.js")
            }
            "package-name" => {
                fixture.write_package("@example/gemini-fixture", "0.60.0", "bundle/gemini.js")
            }
            "bin-mismatch" => {
                fixture.write_package("@google/gemini-cli", "0.60.0", "bundle/other.js")
            }
            "missing-identity" => installation.executable_identity = None,
            "relative-identity" => {
                installation
                    .executable_identity
                    .as_mut()
                    .unwrap()
                    .canonical_path = "bundle/gemini.js".to_owned();
            }
            "unknown-layout" => {
                let executable = fixture.package.join("alternate/gemini.js");
                fs::create_dir_all(executable.parent().unwrap()).unwrap();
                fs::write(&executable, b"// Synthetic fixture. Never execute.\n").unwrap();
                fixture.write_package("@google/gemini-cli", "0.60.0", "alternate/gemini.js");
                installation.executable_path = Some(executable.to_string_lossy().into_owned());
                installation
                    .executable_identity
                    .as_mut()
                    .unwrap()
                    .canonical_path = executable.to_string_lossy().into_owned();
            }
            "missing-builtin" | "builtin-is-file" => {
                let root = fixture.package.join("bundle/builtin");
                fs::rename(&root, fixture.package.join("bundle/previous-builtin")).unwrap();
                if scenario == "builtin-is-file" {
                    fs::write(root, b"Not a resource directory.\n").unwrap();
                }
            }
            "unavailable" => installation.availability = AgentInstallationAvailability::Unavailable,
            "unknown-channel" => installation.distribution = AgentCliDistribution::Unknown,
            "unsupported-distribution" => {
                installation.distribution = AgentCliDistribution::Homebrew
            }
            "other-agent" => installation.agent_kind = AgentCliKind::Codex,
            "incompatible-unverified" => installation.installed_version = None,
            "no-installation" | "incompatible-context" => {}
            _ => unreachable!(),
        }
        let installations = if scenario == "no-installation" {
            Vec::new()
        } else {
            vec![installation]
        };
        let contexts = if matches!(scenario, "incompatible-context" | "incompatible-unverified") {
            incompatible_contexts as AgentContextDiscovery
        } else {
            fixture_contexts as AgentContextDiscovery
        };
        let (snapshot, read_paths) = fixture.scan_with_contexts(installations, contexts);
        assert_eq!(
            skill_discovery_reasons(&snapshot),
            expected_reason.into_iter().collect::<Vec<_>>(),
            "{scenario}: {:#?}",
            snapshot.inventory.diagnostics
        );
        let assets = skills(&snapshot);
        assert_eq!(
            assets.len(),
            1,
            "{scenario}: {:#?}",
            snapshot.inventory.diagnostics
        );
        assert_eq!(assets[0].native_id, "user-skill", "{scenario}");
        assert_eq!(
            assets[0].effective_state,
            AgentAssetState::Enabled,
            "{scenario}"
        );
        assert!(
            !snapshot
                .inventory
                .sources
                .iter()
                .any(|source| source.origin == AgentAssetInstallationOrigin::Bundled),
            "{scenario}"
        );
        assert!(
            !read_paths
                .iter()
                .any(|path| path.starts_with(&fixture.package)),
            "{scenario}"
        );
    }
}

#[test]
fn gemini_builtin_reverifies_package_changes_after_external_root_admission() {
    fn changed_package_contexts(
        request: AgentContextDiscoveryRequest<'_>,
        output: &mut dyn AgentDiagnosticOutput,
        version: &str,
        bin: &str,
    ) -> Vec<AgentConfigurationContext> {
        // The real pipeline has already registered the verified external root.
        // Change only this synthetic package before builtin source discovery.
        let identity = request.installations[0]
            .executable_identity
            .as_ref()
            .unwrap();
        let package = Path::new(&identity.canonical_path)
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        assert!(package.starts_with(request.home.parent().unwrap()));
        let manifest_path = package.join("package.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        assert_eq!(manifest["version"], "0.60.0");
        assert_eq!(manifest["bin"]["gemini"], "bundle/gemini.js");
        manifest["version"] = version.into();
        manifest["bin"]["gemini"] = bin.into();
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        fixture_contexts(request, output)
    }

    let scenarios: [(&str, AgentContextDiscovery); 2] = [
        ("version-change", |request, output| {
            changed_package_contexts(request, output, "0.61.0", "bundle/gemini.js")
        }),
        ("bin-change", |request, output| {
            changed_package_contexts(request, output, "0.60.0", "bundle/replaced.js")
        }),
    ];
    for (scenario, contexts) in scenarios {
        let fixture = BuiltinFixture::new();
        write_skill(&fixture.builtin("must-not-load"), "must-not-load");
        write_skill(
            &fixture.home.join(".gemini/skills/user-skill"),
            "user-skill",
        );
        fs::write(
            fixture.package.join("bundle/replaced.js"),
            b"// Synthetic replacement metadata fixture. Never execute.\n",
        )
        .unwrap();
        let installation = fixture.installation();
        assert_eq!(
            definition(AgentCliKind::Gemini)
                .environment()
                .readonly_external_roots(std::slice::from_ref(&installation)),
            [fixture.package.join("bundle/builtin")],
            "{scenario} starts with an admissible package"
        );
        let (snapshot, read_paths) = fixture.scan_with_contexts(vec![installation], contexts);
        assert_eq!(
            skill_discovery_reasons(&snapshot),
            [AgentAssetDiscoveryIncompleteReason::InstallationUnverified],
            "{scenario}: {:#?}",
            snapshot.inventory.diagnostics
        );
        let assets = skills(&snapshot);
        assert_eq!(assets.len(), 1, "{scenario}");
        assert_eq!(assets[0].native_id, "user-skill", "{scenario}");
        assert_eq!(
            assets[0].effective_state,
            AgentAssetState::Enabled,
            "{scenario}"
        );
        assert!(
            !snapshot
                .inventory
                .sources
                .iter()
                .any(|source| source.origin == AgentAssetInstallationOrigin::Bundled),
            "{scenario}"
        );
        assert!(
            !read_paths
                .iter()
                .any(|path| path.starts_with(&fixture.package)),
            "{scenario} must not open builtin resource files"
        );
    }
}

#[cfg(unix)]
#[test]
fn gemini_builtin_resources_keep_no_follow_boundaries() {
    use std::os::unix::fs::symlink;

    let fixture = BuiltinFixture::new();
    write_skill(&fixture.builtin("valid"), "valid");
    let outside = fixture.root.join("outside-package");
    write_skill(&outside, "outside-package");
    symlink(&outside, fixture.builtin("linked-directory")).unwrap();
    let leaf = fixture.builtin("linked-leaf");
    fs::create_dir(&leaf).unwrap();
    symlink(outside.join("SKILL.md"), leaf.join("SKILL.md")).unwrap();
    let (snapshot, read_paths) = fixture.scan(vec![fixture.installation()]);
    let assets = skills(&snapshot);
    assert_eq!(assets.len(), 1, "{:#?}", snapshot.inventory.diagnostics);
    assert_eq!(assets[0].native_id, "valid");
    assert!(!read_paths
        .iter()
        .any(|path| path.starts_with(&outside) || path.starts_with(&leaf)));

    let builtin_root = fixture.package.join("bundle/builtin");
    fs::rename(
        &builtin_root,
        fixture.package.join("bundle/previous-builtin"),
    )
    .unwrap();
    symlink(&outside, &builtin_root).unwrap();
    let (changed, read_paths) = fixture.scan(vec![fixture.installation()]);
    assert_eq!(
        skill_discovery_reasons(&changed),
        [AgentAssetDiscoveryIncompleteReason::SourceUnavailable]
    );
    assert!(skills(&changed).is_empty());
    assert!(!changed
        .inventory
        .sources
        .iter()
        .any(|source| source.origin == AgentAssetInstallationOrigin::Bundled));
    assert!(!read_paths.iter().any(|path| path.starts_with(&outside)));
}
