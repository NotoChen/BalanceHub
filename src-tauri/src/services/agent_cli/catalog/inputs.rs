//! Display freshness belongs to the published catalog, including complete Skill
//! package inputs. Checking it reads metadata, never definitions or CLI output.
use super::super::{cache, definitions, discovery};
use crate::{
    models::{AgentAssetCategory, AgentAssetDiagnostic, AgentCliKind, AppSettings},
    services::agent_cli::environment::{
        mutation::{GuardedFile, MutationInventory},
        verified_path::{reopen_verified_path, VerifiedPathAnchor},
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Serialize, Deserialize)]
enum PathWatch {
    Identity,
    Contents,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct CatalogInputs {
    paths: BTreeMap<PathBuf, PathWatch>,
    signature: Option<String>,
    agents: BTreeMap<AgentCliKind, AgentInputs>,
}

#[derive(Clone, Serialize, Deserialize)]
struct AgentInputs {
    paths: BTreeMap<PathBuf, PathWatch>,
    signature: String,
}

impl CatalogInputs {
    pub(super) fn capture(
        snapshot: &MutationInventory,
        packages: &[VerifiedPathAnchor],
        library: &GuardedFile,
        settings: &AppSettings,
        environment_before: &str,
    ) -> Self {
        let mut paths = BTreeMap::from([(library.path().to_owned(), PathWatch::Contents)]);
        for source in &snapshot.inventory.sources {
            include_path(
                &mut paths,
                source.path.as_ref(),
                Some(source.allowed_root.as_ref()),
            );
        }
        for context in &snapshot.inventory.contexts {
            paths.insert(PathBuf::from(&context.config_root), PathWatch::Contents);
        }
        let anchors = snapshot
            .source_anchors
            .values()
            .chain(packages)
            .chain(library.verified_anchor());
        for anchor in anchors.clone() {
            include_path(
                &mut paths,
                anchor.physical_path(),
                Some(anchor.allowed_root()),
            );
            paths.insert(anchor.display_path().to_owned(), PathWatch::Contents);
        }
        let environment = environment_signature(settings);
        let signature = path_signature(&environment, &paths);
        // Do not bless a file change between the scan and the metadata stamp.
        // Unknown sources remain unknown; a newly created missing source must
        // also invalidate the original scan, even though it had no anchor.
        let stable = environment == environment_before
            && anchors
                .into_iter()
                .all(|anchor| reopen_verified_path(anchor).is_ok())
            && snapshot.inventory.sources.iter().all(|source| {
                !source.revision.is_missing || std::fs::symlink_metadata(&source.path).is_err()
            });
        let agents = definitions()
            .iter()
            .map(|definition| {
                let kind = definition.kind;
                let contexts = snapshot
                    .inventory
                    .contexts
                    .iter()
                    .filter(|context| context.agent_kind == kind)
                    .collect::<Vec<_>>();
                let mut paths = contexts
                    .iter()
                    .map(|context| (PathBuf::from(&context.config_root), PathWatch::Contents))
                    .collect::<BTreeMap<_, _>>();
                let skill_sources = snapshot
                    .inventory
                    .assets
                    .iter()
                    .filter(|asset| {
                        asset.agent_kind == kind && asset.category == AgentAssetCategory::Skill
                    })
                    .flat_map(|asset| &asset.source_ids)
                    .collect::<BTreeSet<_>>();
                for source in snapshot.inventory.sources.iter().filter(|source| {
                    contexts
                        .iter()
                        .any(|context| context.id == source.context_id)
                }) {
                    include_path(
                        &mut paths,
                        source.path.as_ref(),
                        Some(source.allowed_root.as_ref()),
                    );
                    if let Some(anchor) = snapshot.source_anchors.get(&source.id) {
                        include_path(
                            &mut paths,
                            anchor.physical_path(),
                            Some(anchor.allowed_root()),
                        );
                        if skill_sources.contains(&source.id) {
                            let root = if anchor.source_kind()
                                == crate::models::AgentAssetSourceKind::Directory
                            {
                                Some(anchor.physical_path())
                            } else {
                                anchor.physical_path().parent()
                            };
                            for package in packages.iter().filter(|package| {
                                root.is_some_and(|root| package.physical_path().starts_with(root))
                            }) {
                                include_path(
                                    &mut paths,
                                    package.physical_path(),
                                    Some(package.allowed_root()),
                                );
                            }
                        }
                    }
                }
                let signature =
                    path_signature(&agent_environment_signature(settings, kind), &paths);
                (kind, AgentInputs { paths, signature })
            })
            .collect();
        let incomplete = snapshot
            .inventory
            .diagnostics
            .iter()
            .chain(
                snapshot
                    .inventory
                    .sources
                    .iter()
                    .flat_map(|source| &source.diagnostics),
            )
            .chain(
                snapshot
                    .inventory
                    .installations
                    .iter()
                    .flat_map(|item| &item.diagnostics),
            )
            .chain(
                snapshot
                    .inventory
                    .assets
                    .iter()
                    .flat_map(|item| &item.diagnostics),
            )
            .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::BudgetExceeded { .. }));
        let stable = stable
            && !incomplete
            && signature == path_signature(&environment_signature(settings), &paths);
        Self {
            paths,
            signature: stable.then_some(signature),
            agents,
        }
    }

    pub(crate) fn changed_agents(&self, settings: &AppSettings) -> BTreeSet<AgentCliKind> {
        definitions()
            .iter()
            .filter_map(|definition| {
                let kind = definition.kind;
                let current = self.signature.is_some()
                    && self.agents.get(&kind).is_some_and(|input| {
                        input.signature
                            == path_signature(
                                &agent_environment_signature(settings, kind),
                                &input.paths,
                            )
                    });
                (!current).then_some(kind)
            })
            .collect()
    }

    pub(super) fn cacheable(&self) -> bool {
        self.signature.is_some()
    }

    pub(crate) fn is_current(&self, settings: &AppSettings) -> bool {
        self.signature.as_ref().is_some_and(|current| {
            current == &path_signature(&environment_signature(settings), &self.paths)
        })
    }
}

pub(crate) fn environment_signature(settings: &AppSettings) -> String {
    let mut digest = Sha256::new();
    for definition in definitions() {
        digest.update(agent_environment_signature(settings, definition.kind));
    }
    format!("{:x}", digest.finalize())
}

fn agent_environment_signature(settings: &AppSettings, kind: AgentCliKind) -> String {
    let mut digest = Sha256::new();
    digest.update(env!("CARGO_PKG_VERSION"));
    // A rebuilt parser invalidates persisted display facts, even at the same version.
    if let Ok(executable) = std::env::current_exe() {
        cache::observe_path(&mut digest, &executable);
    }
    digest.update(settings.agent_cli_path(kind));
    cache::observe_environment(&mut digest, kind);
    discovery::observe_candidates(
        &mut digest,
        settings.agent_cli_path(kind),
        super::super::definition(kind),
    );
    format!("{:x}", digest.finalize())
}

fn path_signature(environment: &str, paths: &BTreeMap<PathBuf, PathWatch>) -> String {
    let mut digest = Sha256::new();
    digest.update(environment);
    for (path, watch) in paths {
        match watch {
            PathWatch::Contents => cache::observe_path(&mut digest, path),
            PathWatch::Identity => cache::observe_path_identity(&mut digest, path),
        }
    }
    format!("{:x}", digest.finalize())
}

fn include_path(paths: &mut BTreeMap<PathBuf, PathWatch>, path: &Path, root: Option<&Path>) {
    let mut ancestors = BTreeSet::new();
    cache::include_path(&mut ancestors, path, root);
    for ancestor in ancestors {
        paths.entry(ancestor).or_insert(PathWatch::Identity);
    }
    paths.insert(path.to_owned(), PathWatch::Contents);
}
