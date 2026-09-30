//! Exact reads of immutable Plugin definitions. Public parent references select
//! a package, while fresh verified native inputs prove its role and membership.
use super::*;
use crate::services::agent_cli::{
    catalog::projection::definition_anchor,
    environment::{
        config_document::{self, ConfigDocumentFormat},
        mutation::GuardedDirectory,
        verified_path::reopen_verified_path,
    },
    grok::environment::plugin::{manifest, CONVENTIONS, MANIFESTS},
};
use std::path::PathBuf;

struct Reads<'a> {
    snapshot: &'a MutationInventory,
    ids: BTreeSet<String>,
    files: BTreeMap<String, Option<Vec<u8>>>,
}

impl Reads<'_> {
    fn touch(&mut self, source: &AgentAssetSource) -> Result<(), String> {
        if self.ids.contains(&source.id) {
            return Ok(());
        }
        if source.revision.is_symlink {
            return Err("Grok Plugin 来源不允许符号链接".to_owned());
        }
        if !source.revision.is_missing {
            let anchor = self
                .snapshot
                .source_anchors
                .get(&source.id)
                .ok_or("Grok Plugin 来源证据不完整")?;
            if anchor.revision().identity != source.revision.identity
                || anchor.display_path() != Path::new(&source.path)
                || anchor.allowed_root() != Path::new(&source.allowed_root)
                || anchor.source_kind() != source.source_kind
                || anchor.is_readonly_reference()
            {
                return Err("Grok Plugin 来源身份已变化".to_owned());
            }
        }
        if source.source_kind == AgentAssetSourceKind::File {
            let guard = GuardedFile::capture(source, &self.snapshot.source_anchors)
                .map_err(|_| "Grok Plugin 文件证据已变化")?;
            guard
                .revalidate()
                .map_err(|_| "Grok Plugin 文件证据已变化")?;
            self.files
                .insert(source.id.clone(), guard.bytes().map(<[u8]>::to_vec));
        } else if source.revision.is_missing {
            let guard = GuardedDirectory::capture_path(Path::new(&source.path))
                .map_err(|_| "Grok Plugin 目录缺失证据已变化")?;
            if !matches!(guard, GuardedDirectory::Missing { .. }) {
                return Err("Grok Plugin 目录已出现".to_owned());
            }
            guard
                .revalidate()
                .map_err(|_| "Grok Plugin 目录缺失证据已变化")?;
        } else {
            let anchor = self
                .snapshot
                .source_anchors
                .get(&source.id)
                .ok_or("Grok Plugin 目录证据不完整")?;
            reopen_verified_path(anchor)
                .and_then(|guard| guard.revalidate())
                .map_err(|_| "Grok Plugin 目录成员已变化")?;
        }
        self.ids.insert(source.id.clone());
        Ok(())
    }

    fn json(&mut self, source: &AgentAssetSource) -> Result<Option<Value>, String> {
        self.touch(source)?;
        self.files
            .get(&source.id)
            .ok_or("Grok Plugin 来源不是文件")?
            .as_deref()
            .map(|bytes| {
                config_document::parse(bytes, ConfigDocumentFormat::Json)
                    .ok_or_else(|| "Grok Plugin JSON 无效或有重复字段".to_owned())
            })
            .transpose()
    }
}

fn roots(context: &AgentConfigurationContext) -> Vec<(AgentAssetScope, PathBuf, PathBuf)> {
    let root = PathBuf::from(&context.config_root);
    let mut roots = vec![(AgentAssetScope::User, root.join("plugins"), root)];
    if let Some(workspace) = &context.workspace_id {
        let workspace = PathBuf::from(workspace);
        roots.push((
            AgentAssetScope::Workspace,
            workspace.join(".grok/plugins"),
            workspace,
        ));
    }
    roots
}

fn exact<'a>(
    snapshot: &'a MutationInventory,
    context: &AgentConfigurationContext,
    scope: AgentAssetScope,
    path: &Path,
    allowed_root: &Path,
    kind: AgentAssetSourceKind,
) -> Result<&'a AgentAssetSource, String> {
    let matches = snapshot
        .inventory
        .sources
        .iter()
        .filter(|source| {
            source.context_id == context.id
                && source.environment_id == context.environment_id
                && source.scope == scope
                && source.origin == AgentAssetInstallationOrigin::NativePackage
                && source.source_kind == kind
                && !source.writable
                && !source.revision.is_symlink
                && Path::new(&source.path) == path
                && Path::new(&source.allowed_root) == allowed_root
                && source.categories.contains(&AgentAssetCategory::Plugin)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [source] => Ok(*source),
        _ => Err("Grok Plugin 父级来源不完整或不唯一".to_owned()),
    }
}

fn selection_dependencies(
    reads: &mut Reads<'_>,
    context: &AgentConfigurationContext,
) -> Result<(), String> {
    for (scope, root, allowed) in roots(context) {
        reads.touch(exact(
            reads.snapshot,
            context,
            scope,
            &root,
            &allowed,
            AgentAssetSourceKind::Directory,
        )?)?;
        for source in reads.snapshot.inventory.sources.iter().filter(|source| {
            source.context_id == context.id
                && source.environment_id == context.environment_id
                && source.scope == scope
                && source.origin == AgentAssetInstallationOrigin::NativePackage
                && source.categories.contains(&AgentAssetCategory::Plugin)
        }) {
            let path = Path::new(&source.path);
            let package = Path::new(&source.allowed_root);
            let package_directory = source.source_kind == AgentAssetSourceKind::Directory
                && path.parent() == Some(root.as_path())
                && package == root;
            let package_evidence = package.parent() == Some(root.as_path())
                && path.strip_prefix(package).ok().is_some_and(|relative| {
                    MANIFESTS.iter().any(|name| relative == Path::new(name))
                        || CONVENTIONS.iter().any(|(name, kind)| {
                            relative == Path::new(name) && source.source_kind == *kind
                        })
                });
            if package_directory || package_evidence {
                reads.touch(source)?;
            }
        }
    }
    // These observed controls currently leave Plugin runtime state Unknown.
    // Retain them as dependencies so a policy plan never outlives its parent
    // inputs, without pretending to implement a Plugin switch.
    for source in reads.snapshot.inventory.sources.iter().filter(|source| {
        source.context_id == context.id
            && source.environment_id == context.environment_id
            && source.origin == AgentAssetInstallationOrigin::ConfigEntry
            && source.categories.contains(&AgentAssetCategory::Plugin)
            && ((source.scope == AgentAssetScope::User
                && Path::new(&source.path) == Path::new(&context.config_root).join("config.toml"))
                || (source.scope == AgentAssetScope::Workspace
                    && context.workspace_id.as_deref().is_some_and(|workspace| {
                        Path::new(&source.path) == Path::new(workspace).join(".grok/config.toml")
                    })))
    }) {
        reads.touch(source)?;
    }
    Ok(())
}

fn descriptor(
    reads: &mut Reads<'_>,
    context: &AgentConfigurationContext,
    scope: AgentAssetScope,
    package: &Path,
) -> Result<(manifest::Descriptor, String, Option<Value>), String> {
    for name in MANIFESTS {
        let source = exact(
            reads.snapshot,
            context,
            scope,
            &package.join(name),
            package,
            AgentAssetSourceKind::File,
        )?;
        if let Some(raw) = reads.json(source)? {
            let descriptor = manifest::decode(&raw).ok_or("Grok Plugin 清单已变化或无效")?;
            return Ok((descriptor, source.id.clone(), Some(raw)));
        }
    }
    let descriptor = manifest::convention(
        package
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Grok Plugin 包目录无效")?,
    )
    .ok_or("Grok Plugin 缺省命名无效")?;
    for (name, kind) in CONVENTIONS {
        let source = exact(
            reads.snapshot,
            context,
            scope,
            &package.join(name),
            package,
            kind,
        )?;
        reads.touch(source)?;
        if !source.revision.is_missing {
            let raw = if kind == AgentAssetSourceKind::File {
                reads.json(source)?
            } else {
                None
            };
            return Ok((descriptor, source.id.clone(), raw));
        }
    }
    Err("Grok Plugin 缺少原生包证据".to_owned())
}

pub(super) fn inspect(
    snapshot: &MutationInventory,
    source: &AgentAssetSource,
    context: &AgentConfigurationContext,
) -> Result<InspectedSource, String> {
    let package = Path::new(&source.allowed_root);
    let roots = roots(context);
    let (_, root, _) = roots
        .iter()
        .find(|(scope, root, _)| *scope == source.scope && package.parent() == Some(root.as_path()))
        .ok_or("来源不属于 Grok Plugin 原生包目录")?;
    if source.origin != AgentAssetInstallationOrigin::NativePackage
        || source.source_kind != AgentAssetSourceKind::File
        || source.writable
        || source.revision.is_symlink
        || source.context_id != context.id
        || source.environment_id != context.environment_id
        || !source.categories.contains(&AgentAssetCategory::Hook)
        || !Path::new(&source.path).starts_with(package)
    {
        return Err("来源不是已验证的 Grok Plugin Hook".to_owned());
    }
    let mut reads = Reads {
        snapshot,
        ids: BTreeSet::new(),
        files: BTreeMap::new(),
    };
    selection_dependencies(&mut reads, context)?;
    reads.touch(exact(
        snapshot,
        context,
        source.scope,
        package,
        root,
        AgentAssetSourceKind::Directory,
    )?)?;
    let (descriptor, parent_source_id, parent_raw) =
        descriptor(&mut reads, context, source.scope, package)?;
    let parents = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| {
            asset.context_id == context.id
                && asset.agent_kind == AGENT_KIND
                && asset.category == AgentAssetCategory::Plugin
                && asset.inspection_source_id == parent_source_id
                && asset.native_id == descriptor.namespace
                && matches!(
                    asset.resolution.relation,
                    AgentAssetResolutionRelation::Independent
                        | AgentAssetResolutionRelation::ReplaceWinner
                )
                && definition_anchor(snapshot, asset).is_some_and(|declaration| {
                    declaration.native_kind == AgentAssetCategory::Plugin
                        && declaration.declaration_key == "plugin"
                })
        })
        .collect::<Vec<_>>();
    let [parent] = parents.as_slice() else {
        return Err("Grok Hook 的父插件已变化、未被原生选中或存在歧义".to_owned());
    };
    let inline = descriptor.hooks_inline.is_some();
    let value = if let Some(value) = &descriptor.hooks_inline {
        if source.id != parent_source_id
            || parent_raw.as_ref().and_then(|raw| raw.get("hooks")) != Some(value)
        {
            return Err("Grok Plugin 内联 Hook 来源已变化".to_owned());
        }
        value.clone()
    } else {
        let relative = manifest::component_path(
            package,
            descriptor
                .hooks_path
                .as_deref()
                .ok_or("Grok Plugin 缺少 Hook 来源")?,
        )
        .ok_or("Grok Plugin Hook 路径越界")?;
        if Path::new(&source.path) != package.join(relative) {
            return Err("此文件不是 Grok Plugin 当前选择的 Hook 来源".to_owned());
        }
        reads.json(source)?.unwrap_or_else(|| serde_json::json!({}))
    };
    let namespace =
        manifest::hook_namespace(&descriptor.namespace, Path::new(&source.path), inline)
            .ok_or("Grok Plugin Hook 名称无法精确绑定")?;
    let rules = if value.is_object() {
        source_rules(snapshot, source, &value, &namespace, true)?
    } else {
        Vec::new()
    };
    let reference = AgentAssetNativeRef {
        category: AgentAssetCategory::Plugin,
        native_id: descriptor.namespace,
        qualifier: Some(format!("plugin:{}", parent.native_id)),
    };
    for rule in &rules {
        let asset = rule
            .native_asset_id
            .as_ref()
            .and_then(|id| {
                snapshot
                    .inventory
                    .assets
                    .iter()
                    .find(|asset| &asset.stable_id == id)
            })
            .ok_or("Grok Plugin Hook 的库存成员不完整")?;
        if asset.relationships.provided_by.as_deref() != Some(parent.stable_id.as_str())
            || asset.relationships.action_owner != asset.relationships.provided_by
            || definition_anchor(snapshot, asset).is_none_or(|declaration| {
                declaration.provided_by.as_ref() != Some(&reference)
                    || declaration.action_owner != declaration.provided_by
            })
        {
            return Err("Grok Plugin Hook 的已验证父级关系已变化".to_owned());
        }
    }
    Ok(InspectedSource {
        rules,
        namespace,
        read_source_ids: reads.ids,
    })
}
