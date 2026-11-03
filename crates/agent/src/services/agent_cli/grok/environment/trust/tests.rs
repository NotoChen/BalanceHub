use super::*;
use crate::{
    models::{AgentAssetDiagnostic, AgentAssetSourceKind},
    services::agent_cli::contracts::{
        AgentAssetDirectoryEntry, AgentAssetSourceSpec, AgentDiagnosticEmission, AgentOutputStop,
    },
};
use std::{ops::ControlFlow, path::PathBuf};

const GIT_CONFIG: &[u8] = b"[core]\nrepositoryformatversion = 0\nfilemode = true\nbare = false\nlogallrefupdates = true\n";

#[derive(Default)]
struct Output {
    sources: Vec<AgentAssetSourceSpec>,
    diagnostics: Vec<AgentAssetDiagnostic>,
}

impl AgentDiagnosticOutput for Output {
    fn has_regular_capacity(&self) -> bool {
        true
    }
    fn emit_diagnostic(&mut self, diagnostic: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.diagnostics.push(diagnostic);
        AgentDiagnosticEmission::Accepted
    }
}

impl InitialSourceOutput for Output {
    fn emit_initial(&mut self, source: AgentAssetSourceSpec) -> ControlFlow<AgentOutputStop> {
        self.sources.push(source);
        ControlFlow::Continue(())
    }
}

fn directory(entries: &[(&str, AgentAssetSourceKind)]) -> AgentAssetSnapshot {
    AgentAssetSnapshot::DirectoryManifest {
        entries: entries
            .iter()
            .map(|(name, source_kind)| AgentAssetDirectoryEntry {
                name: (*name).to_owned(),
                source_kind: *source_kind,
                is_symlink: false,
            })
            .collect(),
        complete: true,
        revision: Default::default(),
    }
}

fn file(bytes: &[u8]) -> AgentAssetSnapshot {
    AgentAssetSnapshot::File {
        bytes: bytes.to_vec(),
        revision: Default::default(),
    }
}

fn home() -> PathBuf {
    PathBuf::from(if cfg!(windows) {
        "C:/bh-trust-home"
    } else {
        "/bh-trust-home"
    })
}

fn trust_document(decisions: &[(&Path, bool)]) -> Vec<u8> {
    let folders = decisions
        .iter()
        .map(|(path, trusted)| {
            (
                path.to_string_lossy().into_owned(),
                toml::Value::Table(
                    [("trusted".to_owned(), toml::Value::Boolean(*trusted))]
                        .into_iter()
                        .collect(),
                ),
            )
        })
        .collect();
    toml::to_string(&toml::Value::Table(
        [("folders".to_owned(), toml::Value::Table(folders))]
            .into_iter()
            .collect(),
    ))
    .unwrap()
    .into_bytes()
}

struct Fixture {
    workspace: PathBuf,
    sources: Vec<(AgentAssetSourceSpec, AgentAssetSnapshot)>,
}

impl Fixture {
    fn new(workspace: PathBuf, repos: &[PathBuf]) -> Self {
        let home = home();
        let mut output = Output::default();
        discover_workspace_trust_sources(
            AgentWorkspaceTrustSourceRequest {
                home: &home,
                workspace: &workspace,
                config_root: &home.join(".grok"),
            },
            &mut output,
        );
        let sources = output
            .sources
            .into_iter()
            .map(|spec| {
                let snapshot = if spec.native_source_key == STORE_KEY {
                    file(&trust_document(&[(&workspace, true)]))
                } else if spec.native_source_key == REGISTRY_KEY {
                    directory(&[])
                } else if spec.native_source_key.starts_with(ROOT_PREFIX)
                    || spec.native_source_key == HOME_ROOT_KEY
                {
                    if repos.contains(&spec.path) {
                        directory(&[(".git", AgentAssetSourceKind::Directory)])
                    } else {
                        directory(&[])
                    }
                } else if repos.iter().any(|repo| spec.path == repo.join(".git")) {
                    directory(&[
                        ("HEAD", AgentAssetSourceKind::File),
                        ("config", AgentAssetSourceKind::File),
                        ("objects", AgentAssetSourceKind::Directory),
                        ("refs", AgentAssetSourceKind::Directory),
                    ])
                } else if repos.iter().any(|repo| spec.path == repo.join(".git/HEAD")) {
                    file(b"ref: refs/heads/main\n")
                } else if repos
                    .iter()
                    .any(|repo| spec.path == repo.join(".git/config"))
                {
                    file(GIT_CONFIG)
                } else {
                    AgentAssetSnapshot::Missing {
                        revision: Default::default(),
                    }
                };
                (spec, snapshot)
            })
            .collect();
        Self { workspace, sources }
    }

    fn views(&self) -> Vec<AgentAssetResolveSource<'_>> {
        self.sources
            .iter()
            .map(|(spec, snapshot)| AgentAssetResolveSource { spec, snapshot })
            .collect()
    }

    fn resolve(&self) -> (AgentTrustState, Vec<AgentAssetDiagnostic>) {
        let mut output = Output::default();
        let state = resolve_workspace_trust(
            AgentWorkspaceTrustResolveRequest {
                workspace: &self.workspace,
                workspace_lexical: None,
                sources: &self.views(),
            },
            &mut output,
        );
        (state, output.diagnostics)
    }

    fn snapshot_mut(&mut self, path: &Path) -> &mut AgentAssetSnapshot {
        &mut self
            .sources
            .iter_mut()
            .find(|(spec, _)| spec.path == path)
            .unwrap()
            .1
    }

    fn decisions(&mut self, decisions: &[(&Path, bool)]) {
        *self.snapshot_mut(&home().join(".grok/trusted_folders.toml")) =
            file(&trust_document(decisions));
    }
}

#[test]
fn durable_native_grant_and_repo_root_normalization() {
    let repo = home().join("repo");
    let workspace = repo.join("child");
    let mut fixture = Fixture::new(workspace.clone(), std::slice::from_ref(&repo));
    assert_eq!(
        fixture.resolve().0,
        AgentTrustState::Unknown,
        "cwd-only grant must not replace the native repo key"
    );
    fixture.decisions(&[(&repo, true), (&workspace, false)]);
    assert_eq!(
        fixture.resolve().0,
        AgentTrustState::Trusted,
        "caller queries the repo root first"
    );
    let views = fixture.views();
    let topology = topology::Topology::new(&views);
    let document = toml::from_str(
        std::str::from_utf8(&trust_document(&[(&repo, true), (&workspace, false)])).unwrap(),
    )
    .unwrap();
    assert_eq!(
        durable_grant(&document, &workspace, &topology),
        Some(false),
        "store itself honors the most specific same-repo decision"
    );
}

#[test]
fn tied_alias_decisions_require_all_grants_and_nested_repos_do_not_inherit() {
    let repo = home().join("repo");
    let child = repo.join("child");
    let mut fixture = Fixture::new(child.clone(), &[repo.clone(), child.clone()]);
    fixture.decisions(&[(&repo, true)]);
    assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
    let alias = PathBuf::from(format!("{}/", child.display()));
    fixture.decisions(&[(&child, true), (&alias, false)]);
    assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
    fixture.decisions(&[(&repo, false), (&child, true), (&alias, true)]);
    assert_eq!(fixture.resolve().0, AgentTrustState::Trusted);
}

#[test]
fn missing_malformed_false_and_project_stores_never_grant_trust() {
    let repo = home().join("repo");
    let mut fixture = Fixture::new(repo.clone(), std::slice::from_ref(&repo));
    assert_eq!(fixture.resolve().0, AgentTrustState::Trusted);
    for bytes in [
        b"".as_slice(),
        b"[folder_trust]\nenabled = false\n",
        b"folders = false\n",
        b"[folders.bad]\ntrusted = true\ndecided_at = 'bad'\n",
    ] {
        *fixture.snapshot_mut(&home().join(".grok/trusted_folders.toml")) = file(bytes);
        assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
    }
    assert!(!fixture.resolve().1.is_empty());
    fixture.decisions(&[(&repo, false)]);
    assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
    *fixture.snapshot_mut(&home().join(".grok/trusted_folders.toml")) =
        AgentAssetSnapshot::Missing {
            revision: Default::default(),
        };
    assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
    fixture.decisions(&[(&repo, true)]);
    fixture
        .sources
        .iter_mut()
        .find(|(spec, _)| spec.native_source_key == STORE_KEY)
        .unwrap()
        .0
        .path = repo.join(".grok/trusted_folders.toml");
    assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
}

#[test]
fn unsafe_roots_blocked_store_and_incomplete_topology_remain_unknown() {
    let repo = home().join("repo");
    let mut fixture = Fixture::new(repo.clone(), std::slice::from_ref(&repo));
    fixture.decisions(&[(&home(), true)]);
    assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
    fixture.decisions(&[(&repo, true)]);
    let diagnostic = AgentAssetDiagnostic::Malformed {
        format: AgentAssetDocumentFormat::Toml,
        location: None,
    };
    *fixture.snapshot_mut(&home().join(".grok/trusted_folders.toml")) =
        AgentAssetSnapshot::Blocked {
            revision: Default::default(),
            diagnostic: diagnostic.clone(),
        };
    assert_eq!(
        fixture.resolve(),
        (AgentTrustState::Unknown, vec![diagnostic])
    );
    fixture.decisions(&[(&repo, true)]);
    if let AgentAssetSnapshot::DirectoryManifest { complete, .. } = fixture.snapshot_mut(&repo) {
        *complete = false;
    }
    assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
}

#[test]
fn conventional_repo_proof_rejects_links_worktrees_and_invalid_git_metadata() {
    let repo = home().join("repo");
    for config in [
        b"[core]\nrepositoryformatversion=0\nbare=true\n".as_slice(),
        b"[core]\nrepositoryformatversion=0\nbare=false\nworktree=elsewhere\n",
        b"[core]\nrepositoryformatversion=0\nbare=false\n[include]\npath=elsewhere\n",
        b"[core]\nrepositoryformatversion=0\nbare=false\n[extensions]\nworktreeConfig=true\n",
    ] {
        let mut fixture = Fixture::new(repo.clone(), std::slice::from_ref(&repo));
        *fixture.snapshot_mut(&repo.join(".git/config")) = file(config);
        assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
    }
    for linked in [false, true] {
        let mut fixture = Fixture::new(repo.clone(), std::slice::from_ref(&repo));
        if let AgentAssetSnapshot::DirectoryManifest { entries, .. } = fixture.snapshot_mut(&repo) {
            entries[0].source_kind = if linked {
                AgentAssetSourceKind::Directory
            } else {
                AgentAssetSourceKind::File
            };
            entries[0].is_symlink = linked;
        }
        assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
    }
    let mut fixture = Fixture::new(repo.clone(), std::slice::from_ref(&repo));
    *fixture.snapshot_mut(&repo.join(".git/HEAD")) = file(b"not a git HEAD\n");
    assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
}

#[test]
fn registry_only_gates_its_native_worktrees_subtree() {
    let repo = home().join(".grok/worktrees/repo");
    let mut fixture = Fixture::new(repo.clone(), std::slice::from_ref(&repo));
    assert_eq!(fixture.resolve().0, AgentTrustState::Trusted);
    *fixture.snapshot_mut(&home().join(".grok")) =
        directory(&[("worktrees.db", AgentAssetSourceKind::File)]);
    assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
    let outside = home().join("repo");
    let mut outside_fixture = Fixture::new(outside.clone(), std::slice::from_ref(&outside));
    let (spec, _) = fixture
        .sources
        .into_iter()
        .find(|(spec, _)| spec.native_source_key == REGISTRY_KEY)
        .unwrap();
    outside_fixture.sources.push((
        spec,
        directory(&[("worktrees.db", AgentAssetSourceKind::File)]),
    ));
    assert_eq!(outside_fixture.resolve().0, AgentTrustState::Trusted);
}

#[test]
fn discovery_never_widens_roots_or_turns_authority_into_asset_definitions() {
    let outside = home().parent().unwrap().join("bh-outside-workspace");
    let fixture = Fixture::new(outside.clone(), &[]);
    assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
    assert!(fixture
        .sources
        .iter()
        .all(|(source, _)| source.categories.is_empty()
            && !source.writable
            && !source.hook_definition_source));
    assert!(fixture
        .sources
        .iter()
        .filter(|(source, _)| source.native_source_key != STORE_KEY)
        .all(|(source, _)| source.path.starts_with(&outside) && source.allowed_root == outside));
    let conventional = Fixture::new(outside.clone(), std::slice::from_ref(&outside));
    assert_eq!(conventional.resolve().0, AgentTrustState::Trusted);
}

#[test]
fn feature_settings_do_not_replace_durable_authority() {
    let repo = home().join("repo");
    for enabled in [true, false] {
        let mut fixture = Fixture::new(repo.clone(), std::slice::from_ref(&repo));
        let mut config = fixture
            .sources
            .iter()
            .find(|(spec, _)| spec.native_source_key == STORE_KEY)
            .unwrap()
            .0
            .clone();
        config.native_source_key = "config".to_owned();
        config.path = home().join(".grok/config.toml");
        fixture.sources.push((
            config,
            file(format!("[folder_trust]\nenabled = {enabled}\n").as_bytes()),
        ));
        assert_eq!(fixture.resolve().0, AgentTrustState::Trusted);
        fixture.decisions(&[]);
        assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
    }
}

#[test]
fn home_repo_is_rescoped_to_cwd_and_native_optional_fields_are_validated() {
    let workspace = home().join("child");
    let mut fixture = Fixture::new(workspace.clone(), &[home()]);
    let mut store = trust_document(&[(&workspace, true)]);
    store.extend_from_slice(b"decided_at = 42\nfuture_field = 'ignored'\n");
    *fixture.snapshot_mut(&home().join(".grok/trusted_folders.toml")) = file(&store);
    assert_eq!(fixture.resolve().0, AgentTrustState::Trusted);
    fixture.decisions(&[(&home(), true)]);
    assert_eq!(fixture.resolve().0, AgentTrustState::Unknown);
}
