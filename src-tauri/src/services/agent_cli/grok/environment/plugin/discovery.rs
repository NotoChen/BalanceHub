use super::{
    incomplete, manifest, revision, PackageRoot, Parent, ParentEvidence, SourceKey, SourceKind,
    CONVENTIONS, MANIFESTS,
};
use crate::models::{
    AgentAssetCategory, AgentAssetDiagnostic, AgentAssetDiscoveryIncompleteReason,
    AgentAssetDocumentFormat, AgentAssetInstallationOrigin, AgentAssetScope, AgentAssetSourceKind,
};
use crate::services::agent_cli::contracts::{
    AgentAssetSnapshot, AgentAssetSourceSpec, AgentSourceDiscoveryRequest, InitialSourceOutput,
};
use crate::services::agent_cli::environment::{
    config_document::{self, ConfigDocumentFormat},
    source, SourceInput,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::ControlFlow,
    path::{Path, PathBuf},
};

struct Candidate {
    base: AgentAssetSourceSpec,
    parent: Parent,
    descriptor: manifest::Descriptor,
    directory_snapshots: BTreeMap<PathBuf, AgentAssetSnapshot>,
    file_sources: BTreeSet<PathBuf>,
}

pub(super) fn spec(base: &AgentAssetSourceSpec, key: &SourceKey) -> Option<AgentAssetSourceSpec> {
    let package = base.path.join(&key.package.directory);
    let allowed_root = if matches!(key.kind, SourceKind::Package) {
        &base.path
    } else {
        &package
    };
    let mut spec = source(SourceInput {
        native_source_key: &key.encode(),
        label: match key.category() {
            AgentAssetCategory::Skill => "Grok Plugin Skill 来源",
            AgentAssetCategory::Mcp => "Grok Plugin MCP 来源",
            AgentAssetCategory::Hook => "Grok Plugin Hook 来源",
            _ => "Grok Plugin 包证据",
        },
        path: package.join(key.relative_path()?),
        allowed_root,
        scope: key.scope(),
        origin: AgentAssetInstallationOrigin::NativePackage,
        precedence: key.precedence(),
        sensitive: true,
        source_kind: key.source_kind()?,
        categories: key.categories(),
    });
    // Package ownership grants no edit/delete/toggle authority.
    spec.writable = false;
    Some(spec)
}

fn root(
    key: &str,
    path: PathBuf,
    allowed_root: &Path,
    scope: AgentAssetScope,
    precedence: u32,
) -> AgentAssetSourceSpec {
    let mut spec = source(SourceInput {
        native_source_key: key,
        label: "Grok Build Plugins",
        path,
        allowed_root,
        scope,
        origin: AgentAssetInstallationOrigin::NativePackage,
        precedence,
        sensitive: false,
        source_kind: AgentAssetSourceKind::Directory,
        categories: &[AgentAssetCategory::Plugin],
    });
    spec.writable = false;
    spec
}

pub(in crate::services::agent_cli::grok::environment) fn discover_sources(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let config_root = Path::new(&request.context.config_root);
    let mut roots = vec![root(
        "plugins",
        config_root.join("plugins"),
        config_root,
        AgentAssetScope::User,
        10,
    )];
    if let Some(workspace) = request.workspace {
        roots.push(root(
            "workspace-plugins",
            workspace.join(".grok/plugins"),
            workspace,
            AgentAssetScope::Workspace,
            20,
        ));
    }
    let mut candidates = Vec::new();
    for base in roots {
        let ControlFlow::Continue(Some(snapshot)) = output.snapshot_initial(base.clone()) else {
            return;
        };
        let entries = match snapshot {
            AgentAssetSnapshot::DirectoryManifest {
                entries,
                complete: true,
                ..
            } => entries,
            AgentAssetSnapshot::Missing { .. } => continue,
            _ => {
                incomplete(
                    output,
                    AgentAssetCategory::Plugin,
                    AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                );
                incomplete(
                    output,
                    AgentAssetCategory::Hook,
                    AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                );
                continue;
            }
        };
        let mut entries = entries;
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        for entry in entries {
            if !super::single_component(&entry.name)
                || (!entry.is_symlink && entry.source_kind != AgentAssetSourceKind::Directory)
            {
                continue;
            }
            match discover_package(&base, entry.name, output) {
                Ok(Some(candidate)) => candidates.push(candidate),
                Ok(None) => {}
                Err(()) => return,
            }
        }
    }
    // No-follow descendants of a verified root preserve canonical path order.
    // All parent definitions remain in the source queue, including losers.
    candidates.sort_by(|left, right| {
        right
            .base
            .precedence
            .cmp(&left.base.precedence)
            .then_with(|| {
                left.base
                    .path
                    .join(&left.parent.package.directory)
                    .cmp(&right.base.path.join(&right.parent.package.directory))
            })
    });
    let mut selected = BTreeSet::new();
    for candidate in candidates {
        if selected.insert(candidate.parent.namespace.clone())
            && expand(&candidate, output).is_err()
        {
            return;
        }
    }
    // This batch does not claim exact-version bundled package semantics. A
    // bounded metadata observation makes a configured installation visible as
    // incomplete without inventing assets or scanning arbitrary roots.
    let mut bundled = source(SourceInput {
        native_source_key: "grok-bundled-entrypoints",
        label: "Grok 未展开的内置来源",
        path: config_root.join("bundled"),
        allowed_root: config_root,
        scope: AgentAssetScope::User,
        origin: AgentAssetInstallationOrigin::Unknown,
        precedence: 0,
        sensitive: false,
        source_kind: AgentAssetSourceKind::Directory,
        categories: &[],
    });
    bundled.writable = false;
    let ControlFlow::Continue(Some(snapshot)) = output.snapshot_initial(bundled) else {
        return;
    };
    match snapshot {
        AgentAssetSnapshot::DirectoryManifest {
            entries, complete, ..
        } => {
            for (name, category) in [
                ("skills", AgentAssetCategory::Skill),
                ("plugins", AgentAssetCategory::Plugin),
            ] {
                if entries.iter().any(|entry| entry.name == name) {
                    incomplete(
                        output,
                        category,
                        AgentAssetDiscoveryIncompleteReason::UnsupportedEntryPoint,
                    );
                } else if !complete {
                    incomplete(
                        output,
                        category,
                        AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                    );
                }
            }
        }
        AgentAssetSnapshot::Missing { .. } => {}
        _ => {
            for category in [AgentAssetCategory::Skill, AgentAssetCategory::Plugin] {
                incomplete(
                    output,
                    category,
                    AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                );
            }
        }
    }
}

fn read(
    base: &AgentAssetSourceSpec,
    key: &SourceKey,
    output: &mut dyn InitialSourceOutput,
) -> Result<AgentAssetSnapshot, ()> {
    match output.snapshot_initial(spec(base, key).ok_or(())?) {
        ControlFlow::Continue(Some(snapshot)) => Ok(snapshot),
        ControlFlow::Continue(None) | ControlFlow::Break(_) => Err(()),
    }
}

fn discover_package(
    base: &AgentAssetSourceSpec,
    directory: String,
    output: &mut dyn InitialSourceOutput,
) -> Result<Option<Candidate>, ()> {
    let package = PackageRoot {
        source: base.native_source_key.clone(),
        directory,
    };
    let snapshot = read(
        base,
        &SourceKey {
            package: package.clone(),
            kind: SourceKind::Package,
        },
        output,
    )?;
    if !matches!(
        snapshot,
        AgentAssetSnapshot::DirectoryManifest { complete: true, .. }
    ) {
        if !matches!(snapshot, AgentAssetSnapshot::Missing { .. }) {
            incomplete(
                output,
                AgentAssetCategory::Plugin,
                AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            );
            incomplete(
                output,
                AgentAssetCategory::Hook,
                AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            );
        }
        return Ok(None);
    }
    let package_revision = revision(&snapshot).to_owned();
    let mut directory_snapshots = BTreeMap::from([(PathBuf::new(), snapshot)]);
    let mut file_sources = BTreeSet::new();
    for rank in 0..MANIFESTS.len() {
        let key = SourceKey {
            package: package.clone(),
            kind: SourceKind::Manifest {
                rank: rank as u8,
                package_revision: package_revision.clone(),
            },
        };
        let snapshot = read(base, &key, output)?;
        file_sources.insert(key.relative_path().ok_or(())?);
        match &snapshot {
            AgentAssetSnapshot::Missing { .. } => continue,
            AgentAssetSnapshot::File { bytes, .. } => {
                let descriptor = config_document::parse(bytes, ConfigDocumentFormat::Json)
                    .and_then(|value| manifest::decode(&value));
                let Some(descriptor) = descriptor else {
                    output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
                        format: AgentAssetDocumentFormat::Manifest,
                        location: None,
                    });
                    incomplete(
                        output,
                        AgentAssetCategory::Hook,
                        AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                    );
                    return Ok(None);
                };
                return Ok(Some(Candidate {
                    base: base.clone(),
                    parent: Parent {
                        package,
                        package_revision,
                        evidence: ParentEvidence::Manifest(rank as u8),
                        revision: revision(&snapshot).to_owned(),
                        namespace: descriptor.namespace.clone(),
                    },
                    descriptor,
                    directory_snapshots,
                    file_sources,
                }));
            }
            _ => {
                // An unreadable or wrong-type higher manifest is not Missing.
                incomplete(
                    output,
                    AgentAssetCategory::Plugin,
                    AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                );
                incomplete(
                    output,
                    AgentAssetCategory::Hook,
                    AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                );
                return Ok(None);
            }
        }
    }
    let Some(descriptor) = manifest::convention(&package.directory) else {
        return Ok(None);
    };
    for (component, (_, kind)) in CONVENTIONS.iter().enumerate() {
        let key = SourceKey {
            package: package.clone(),
            kind: SourceKind::Convention {
                component: component as u8,
                package_revision: package_revision.clone(),
            },
        };
        let snapshot = read(base, &key, output)?;
        if *kind == AgentAssetSourceKind::Directory {
            directory_snapshots.insert(key.relative_path().ok_or(())?, snapshot.clone());
        } else {
            file_sources.insert(key.relative_path().ok_or(())?);
        }
        let exists = matches!(
            (&snapshot, kind),
            (AgentAssetSnapshot::File { .. }, AgentAssetSourceKind::File)
                | (
                    AgentAssetSnapshot::DirectoryManifest { complete: true, .. },
                    AgentAssetSourceKind::Directory
                )
        );
        if exists {
            let evidence_revision = revision(&snapshot).to_owned();
            return Ok(Some(Candidate {
                base: base.clone(),
                parent: Parent {
                    package,
                    package_revision,
                    evidence: ParentEvidence::Convention(component as u8),
                    revision: evidence_revision,
                    namespace: descriptor.namespace.clone(),
                },
                descriptor,
                directory_snapshots,
                file_sources,
            }));
        }
        if !matches!(snapshot, AgentAssetSnapshot::Missing { .. }) {
            incomplete(
                output,
                AgentAssetCategory::Plugin,
                AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            );
        }
    }
    Ok(None)
}

fn expand(candidate: &Candidate, output: &mut dyn InitialSourceOutput) -> Result<(), ()> {
    let package = candidate
        .base
        .path
        .join(&candidate.parent.package.directory);
    let mut directory_snapshots = candidate.directory_snapshots.clone();
    let mut file_sources = candidate.file_sources.clone();
    for (command, roots) in [
        (false, &candidate.descriptor.skills),
        (true, &candidate.descriptor.commands),
    ] {
        let mut unique = BTreeSet::new();
        for raw_path in roots {
            let Some(path) = manifest::component_path(&package, raw_path) else {
                incomplete(
                    output,
                    AgentAssetCategory::Skill,
                    AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                );
                continue;
            };
            if unique.insert(path.clone()) {
                walk_skills(
                    candidate,
                    &path,
                    command,
                    0,
                    &mut directory_snapshots,
                    &mut file_sources,
                    output,
                )?;
            }
        }
    }
    if let Some(path) = manifest::component_path(&package, &candidate.descriptor.mcp_path) {
        // A manifest/convention file owns both its package declaration and
        // co-located MCP entries. Missing earlier manifests/conventions also
        // keep their original source; no second role may register the same path.
        read_component(candidate, &path, &mut file_sources, output)?;
    } else {
        incomplete(
            output,
            AgentAssetCategory::Mcp,
            AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
        );
    }
    if let Some(raw_path) = &candidate.descriptor.hooks_path {
        if let Some(path) = manifest::component_path(&package, raw_path) {
            read_component(candidate, &path, &mut file_sources, output)?;
        } else {
            incomplete(
                output,
                AgentAssetCategory::Hook,
                AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            );
        }
    }
    Ok(())
}

fn read_component(
    candidate: &Candidate,
    path: &Path,
    file_sources: &mut BTreeSet<PathBuf>,
    output: &mut dyn InitialSourceOutput,
) -> Result<(), ()> {
    // The cache is scoped to this package's fixed allowed_root and File kind.
    // No-follow snapshots keep these lexical paths tied to physical identity.
    if !file_sources.insert(path.to_owned()) {
        return Ok(());
    }
    let package = candidate
        .base
        .path
        .join(&candidate.parent.package.directory);
    let key = SourceKey {
        package: candidate.parent.package.clone(),
        kind: SourceKind::ComponentFile {
            parent: candidate.parent.clone(),
            path: path.to_owned(),
            roles: manifest::file_roles(&package, &candidate.descriptor, path),
        },
    };
    read(&candidate.base, &key, output)?;
    Ok(())
}

fn walk_skills(
    candidate: &Candidate,
    path: &Path,
    command: bool,
    depth: usize,
    directory_snapshots: &mut BTreeMap<PathBuf, AgentAssetSnapshot>,
    file_sources: &mut BTreeSet<PathBuf>,
    output: &mut dyn InitialSourceOutput,
) -> Result<(), ()> {
    let snapshot = if let Some(snapshot) = directory_snapshots.get(path) {
        snapshot.clone()
    } else {
        let directory = SourceKey {
            package: candidate.parent.package.clone(),
            kind: SourceKind::SkillDirectory {
                parent: candidate.parent.clone(),
                path: path.to_owned(),
            },
        };
        let snapshot = read(&candidate.base, &directory, output)?;
        directory_snapshots.insert(path.to_owned(), snapshot.clone());
        snapshot
    };
    let entries = match snapshot {
        AgentAssetSnapshot::DirectoryManifest {
            entries,
            complete: true,
            ..
        } => entries,
        AgentAssetSnapshot::Missing { .. } => return Ok(()),
        _ => {
            incomplete(
                output,
                AgentAssetCategory::Skill,
                AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            );
            return Ok(());
        }
    };
    let mut entries = entries;
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    for entry in &entries {
        if !super::single_component(&entry.name) {
            continue;
        }
        let eligible = if command {
            Path::new(&entry.name)
                .extension()
                .is_some_and(|extension| extension == "md")
        } else {
            entry.name == "SKILL.md"
        };
        if eligible && (entry.source_kind == AgentAssetSourceKind::File || entry.is_symlink) {
            read_component(candidate, &path.join(&entry.name), file_sources, output)?;
        }
    }
    // Native depth=5 still checks immediate child SKILL.md files. A directory
    // six levels below the root is visited, but its children are not traversed.
    if !command && depth < 6 {
        for entry in entries {
            if super::single_component(&entry.name)
                && (entry.source_kind == AgentAssetSourceKind::Directory || entry.is_symlink)
                && entry.name != "SKILL.md"
            {
                walk_skills(
                    candidate,
                    &path.join(&entry.name),
                    false,
                    depth + 1,
                    directory_snapshots,
                    file_sources,
                    output,
                )?;
            }
        }
    }
    Ok(())
}
