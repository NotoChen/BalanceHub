use super::*;
use crate::models::AgentTrustState;
use crate::services::agent_cli::contracts::{
    AgentAssetSourceSpec, AgentSourceDiscoveryRequest, InitialSourceOutput,
};
use std::{collections::BTreeMap, ops::ControlFlow};

type PhysicalKey = (String, String, u8);

pub(in crate::services::agent_cli::claude::environment) fn discover(
    request: AgentSourceDiscoveryRequest<'_>,
    registry: AgentAssetSourceSpec,
    output: &mut dyn InitialSourceOutput,
) {
    let ControlFlow::Continue(Some(snapshot)) = output.snapshot_initial(registry) else {
        return;
    };
    let Some(root) = json(&snapshot, output) else {
        return;
    };
    let Some(root) = root.as_object() else { return };
    let entries = registry_entries(request.context, request.workspace, root, output);
    let mut groups = BTreeMap::<String, Vec<RegisteredPlugin>>::new();
    for entry in entries {
        groups.entry(entry.key.id.clone()).or_default().push(entry);
    }
    let mut packages = BTreeMap::<PhysicalKey, Vec<RegisteredPlugin>>::new();
    for mut entries in groups.into_values() {
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.origin.precedence));
        if entries.len() > 1 && entries[0].origin.precedence == entries[1].origin.precedence {
            unobserved(output, "registry.ambiguousInstallation");
            continue;
        }
        let entry = entries.remove(0);
        if matches!(
            entry.origin.scope,
            AgentAssetScope::Workspace | AgentAssetScope::Local
        ) && request.context.trust_context != AgentTrustState::Trusted
        {
            continue;
        }
        packages
            .entry(physical_key(
                &entry.path,
                &entry.path,
                AgentAssetSourceKind::Directory,
            ))
            .or_default()
            .push(entry);
    }
    for entries in packages.into_values() {
        if !discover_package(&entries, output) {
            return;
        }
    }
}

struct PendingFile {
    path: PathBuf,
    bindings: Vec<SourceBinding>,
}

struct PackageDiscovery<'a, 'b> {
    entries: &'a [RegisteredPlugin],
    output: &'b mut dyn InitialSourceOutput,
    directories: BTreeMap<PhysicalKey, AgentAssetSnapshot>,
    files: BTreeMap<PhysicalKey, PendingFile>,
}
impl PackageDiscovery<'_, '_> {
    fn root(&self) -> &Path {
        &self.entries[0].path
    }

    fn directory(&mut self, path: &Path) -> Option<AgentAssetSnapshot> {
        let key = physical_key(path, self.root(), AgentAssetSourceKind::Directory);
        if let Some(snapshot) = self.directories.get(&key) {
            return Some(snapshot.clone());
        }
        let role = if key == physical_key(self.root(), self.root(), AgentAssetSourceKind::Directory)
        {
            SourceRole::Root
        } else {
            SourceRole::Directory
        };
        let source = source_spec(
            path.to_path_buf(),
            self.root(),
            AgentAssetSourceKind::Directory,
            self.entries
                .iter()
                .map(|entry| SourceBinding::new(entry, role.clone()))
                .collect(),
        );
        let ControlFlow::Continue(Some(snapshot)) = self.output.snapshot_initial(source) else {
            return None;
        };
        self.directories.insert(key, snapshot.clone());
        Some(snapshot)
    }

    fn file(&mut self, entry: &RegisteredPlugin, path: PathBuf, role: SourceRole) {
        let key = physical_key(&path, self.root(), AgentAssetSourceKind::File);
        self.files
            .entry(key)
            .or_insert_with(|| PendingFile {
                path,
                bindings: Vec::new(),
            })
            .bindings
            .push(SourceBinding::new(entry, role));
    }

    fn flush(self) -> bool {
        for file in self.files.into_values() {
            if self
                .output
                .emit_initial(source_spec(
                    file.path,
                    &self.entries[0].path,
                    AgentAssetSourceKind::File,
                    file.bindings,
                ))
                .is_break()
            {
                return false;
            }
        }
        true
    }
}

fn discover_package(entries: &[RegisteredPlugin], output: &mut dyn InitialSourceOutput) -> bool {
    let mut discovery = PackageDiscovery {
        entries,
        output,
        directories: BTreeMap::new(),
        files: BTreeMap::new(),
    };
    let package_root = entries[0].path.clone();
    let Some(root_snapshot) = discovery.directory(&package_root) else {
        return false;
    };
    let AgentAssetSnapshot::DirectoryManifest {
        entries: root_entries,
        complete: true,
        ..
    } = root_snapshot
    else {
        return true;
    };
    if root_entries.iter().any(|entry| entry.is_symlink) {
        unobserved(discovery.output, "package.symbolicLink");
    }
    let manifest_path = package_root.join(".claude-plugin/plugin.json");
    let ControlFlow::Continue(Some(snapshot)) = discovery.output.snapshot_initial(source_spec(
        manifest_path.clone(),
        &package_root,
        AgentAssetSourceKind::File,
        entries
            .iter()
            .map(|entry| SourceBinding::new(entry, SourceRole::Manifest))
            .collect(),
    )) else {
        return false;
    };
    for entry in entries {
        let manifest = match &snapshot {
            AgentAssetSnapshot::Missing { .. } => decode_manifest(None, &entry.key),
            AgentAssetSnapshot::File { .. } => json(&snapshot, discovery.output)
                .as_ref()
                .and_then(|value| decode_manifest(Some(value), &entry.key)),
            _ => None,
        };
        let Some(manifest) = manifest else {
            malformed(discovery.output, "manifest");
            continue;
        };
        for field in &manifest.unsupported {
            unobserved(discovery.output, field);
        }
        for (index, root) in skill_roots(&entry.path, &root_entries, &manifest)
            .iter()
            .enumerate()
        {
            if !discover_skills(&mut discovery, entry, &manifest.namespace, index, root) {
                return false;
            }
        }
        let commands = manifest
            .commands
            .clone()
            .unwrap_or_else(|| vec![PathBuf::from("commands")]);
        for (index, relative) in commands.iter().enumerate() {
            let path = entry.path.join(relative);
            if path.extension().is_some_and(|extension| extension == "md") {
                discovery.file(
                    entry,
                    path,
                    SourceRole::Skill {
                        namespace: manifest.namespace.clone(),
                        root_index: index,
                        mode: "command".into(),
                    },
                );
            } else {
                let Some(snapshot) = discovery.directory(&path) else {
                    return false;
                };
                if let AgentAssetSnapshot::DirectoryManifest {
                    entries,
                    complete: true,
                    ..
                } = snapshot
                {
                    if entries.iter().any(|entry| entry.is_symlink) {
                        unobserved(discovery.output, "commands.symbolicLink");
                    }
                    for (file_index, file) in entries.iter().enumerate().filter(|(_, file)| {
                        !file.is_symlink
                            && file.source_kind == AgentAssetSourceKind::File
                            && Path::new(&file.name)
                                .extension()
                                .is_some_and(|extension| extension == "md")
                    }) {
                        discovery.file(
                            entry,
                            path.join(&file.name),
                            SourceRole::Skill {
                                namespace: manifest.namespace.clone(),
                                root_index: index,
                                mode: format!("command-{file_index}"),
                            },
                        );
                    }
                }
            }
        }
        discovery.file(
            entry,
            entry.path.join("hooks/hooks.json"),
            SourceRole::Hook {
                namespace: manifest.namespace.clone(),
                ordinal: 0,
            },
        );
        let mut hook_paths = std::collections::BTreeSet::from([physical_key(
            &entry.path.join("hooks/hooks.json"),
            &entry.path,
            AgentAssetSourceKind::File,
        )]);
        for (ordinal, component) in manifest.hooks.iter().enumerate() {
            if let hooks::HookComponent::File(relative) = component {
                let path = entry.path.join(relative);
                // Native deduplicates repeated physical hook files, including
                // the standard hooks/hooks.json path.
                if path != manifest_path
                    && hook_paths.insert(physical_key(
                        &path,
                        &entry.path,
                        AgentAssetSourceKind::File,
                    ))
                {
                    discovery.file(
                        entry,
                        path,
                        SourceRole::Hook {
                            namespace: manifest.namespace.clone(),
                            ordinal: ordinal + 1,
                        },
                    );
                }
            }
        }
        discovery.file(
            entry,
            entry.path.join(".mcp.json"),
            SourceRole::Mcp {
                namespace: manifest.namespace.clone(),
                ordinal: 0,
            },
        );
        for (index, component) in manifest.mcp.iter().enumerate() {
            if matches!(component, McpComponent::Invalid) {
                malformed(discovery.output, "mcpServers.component");
            }
            if let McpComponent::File(relative) = component {
                let path = entry.path.join(relative);
                // The admitted manifest source also parses explicit references
                // to itself. It cannot be offered again under another role.
                if physical_key(&path, &entry.path, AgentAssetSourceKind::File)
                    != physical_key(&manifest_path, &entry.path, AgentAssetSourceKind::File)
                {
                    discovery.file(
                        entry,
                        path,
                        SourceRole::Mcp {
                            namespace: manifest.namespace.clone(),
                            ordinal: index + 1,
                        },
                    );
                }
            }
        }
    }
    discovery.flush()
}

fn discover_skills(
    discovery: &mut PackageDiscovery<'_, '_>,
    entry: &RegisteredPlugin,
    namespace: &str,
    index: usize,
    root: &Path,
) -> bool {
    let Some(snapshot) = discovery.directory(root) else {
        return false;
    };
    let AgentAssetSnapshot::DirectoryManifest {
        entries,
        complete: true,
        ..
    } = snapshot
    else {
        return true;
    };
    if entries.iter().any(|entry| entry.is_symlink) {
        unobserved(discovery.output, "skills.symbolicLink");
    }
    if ordinary(&entries, "SKILL.md", AgentAssetSourceKind::File) {
        discovery.file(
            entry,
            root.join("SKILL.md"),
            SourceRole::Skill {
                namespace: namespace.to_owned(),
                root_index: index,
                mode: "root".into(),
            },
        );
        return true;
    }
    for (child_index, child) in entries.iter().enumerate().filter(|(_, child)| {
        child.source_kind == AgentAssetSourceKind::Directory && !child.is_symlink
    }) {
        discovery.file(
            entry,
            root.join(&child.name).join("SKILL.md"),
            SourceRole::Skill {
                namespace: namespace.to_owned(),
                root_index: index,
                mode: format!("child-{child_index}"),
            },
        );
    }
    true
}
