//! One physical snapshot can carry multiple native package bindings.
use super::*;
use crate::models::AgentAssetInstallationOrigin;
use crate::services::agent_cli::contracts::{AgentAssetSourcePathPolicy, AgentAssetSourceSpec};
use crate::services::agent_cli::environment::{
    lexical_identity, source_with_logical_origins, SourceInput,
};
use std::collections::BTreeSet;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::services::agent_cli::claude::environment) enum SourceRole {
    Root,
    Manifest,
    Directory,
    Skill {
        namespace: String,
        root_index: usize,
        mode: String,
    },
    Hook {
        namespace: String,
        ordinal: usize,
    },
    Mcp {
        namespace: String,
        ordinal: usize,
    },
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::services::agent_cli::claude::environment) struct SourceBinding {
    pub(in crate::services::agent_cli::claude::environment) key: PackageKey,
    scope: AgentAssetScope,
    precedence: u32,
    pub(in crate::services::agent_cli::claude::environment) role: SourceRole,
}
impl SourceBinding {
    pub(in crate::services::agent_cli::claude::environment) fn new(
        entry: &RegisteredPlugin,
        role: SourceRole,
    ) -> Self {
        Self {
            key: entry.key.clone(),
            scope: entry.origin.scope,
            precedence: entry.origin.precedence,
            role,
        }
    }

    pub(in crate::services::agent_cli::claude::environment) fn origin(
        &self,
    ) -> AgentAssetLogicalOrigin {
        AgentAssetLogicalOrigin {
            scope: self.scope,
            precedence: self.precedence,
        }
    }
}

pub(in crate::services::agent_cli::claude::environment) fn physical_key(
    path: &Path,
    root: &Path,
    kind: AgentAssetSourceKind,
) -> (String, String, u8) {
    (
        lexical_identity(path),
        lexical_identity(root),
        match kind {
            AgentAssetSourceKind::File => 0,
            AgentAssetSourceKind::Directory => 1,
        },
    )
}

pub(in crate::services::agent_cli::claude::environment) fn source_spec(
    path: PathBuf,
    allowed_root: &Path,
    kind: AgentAssetSourceKind,
    mut bindings: Vec<SourceBinding>,
) -> AgentAssetSourceSpec {
    bindings.sort();
    bindings.dedup();
    let origins = bindings
        .iter()
        .map(SourceBinding::origin)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let origin = origins
        .iter()
        .max_by_key(|origin| (origin.precedence, origin.scope))
        .expect("package sources have native bindings");
    let mut categories = BTreeSet::new();
    for binding in &bindings {
        match binding.role {
            SourceRole::Root => {
                categories.insert(AgentAssetCategory::Plugin);
            }
            SourceRole::Manifest => {
                categories.extend([
                    AgentAssetCategory::Plugin,
                    AgentAssetCategory::Mcp,
                    AgentAssetCategory::Hook,
                ]);
            }
            SourceRole::Skill { .. } => {
                categories.insert(AgentAssetCategory::Skill);
            }
            SourceRole::Hook { .. } => {
                categories.insert(AgentAssetCategory::Hook);
            }
            SourceRole::Mcp { .. } => {
                categories.insert(AgentAssetCategory::Mcp);
            }
            SourceRole::Directory => {}
        }
    }
    let marker = if bindings
        .iter()
        .all(|binding| binding.role == SourceRole::Root)
    {
        "root"
    } else if bindings
        .iter()
        .all(|binding| binding.role == SourceRole::Manifest)
    {
        "manifest"
    } else if kind == AgentAssetSourceKind::Directory {
        "directory"
    } else {
        "file"
    };
    let physical = physical_key(&path, allowed_root, kind);
    let key = format!(
        "{PREFIX}{marker}:{}:{}",
        URL_SAFE_NO_PAD.encode(stable_id(
            "package-source",
            &[&physical.0, &physical.1, &physical.2.to_string()],
        )),
        URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&bindings)
                .expect("package bindings contain only serializable fields")
        ),
    );
    let mut spec = source_with_logical_origins(
        SourceInput {
            native_source_key: &key,
            label: "Claude Code Plugin 本地包",
            path,
            allowed_root,
            scope: origin.scope,
            precedence: origin.precedence,
            origin: AgentAssetInstallationOrigin::NativePackage,
            sensitive: kind == AgentAssetSourceKind::File,
            source_kind: kind,
            categories: &categories.into_iter().collect::<Vec<_>>(),
        },
        &origins,
    );
    spec.writable = false;
    spec
}

pub(in crate::services::agent_cli::claude::environment) fn source_bindings(
    source: &AgentAssetSourceSpec,
) -> Option<Vec<SourceBinding>> {
    let encoded = source
        .native_source_key
        .strip_prefix(PREFIX)?
        .splitn(3, ':')
        .nth(2)?;
    let bindings: Vec<SourceBinding> =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).ok()?).ok()?;
    if bindings.is_empty()
        || bindings
            .iter()
            .any(|binding| binding.key.id.is_empty() || binding.key.occurrence.is_empty())
    {
        return None;
    }
    let expected = source_spec(
        source.path.clone(),
        &source.allowed_root,
        source.source_kind,
        bindings.clone(),
    );
    (source.native_source_key == expected.native_source_key
        && source.scope == expected.scope
        && source.precedence == expected.precedence
        && source.allowed_logical_origins == expected.allowed_logical_origins
        && source.categories == expected.categories
        && !source.writable
        && source.path_policy == AgentAssetSourcePathPolicy::NoFollow
        && source.origin == AgentAssetInstallationOrigin::NativePackage)
        .then_some(bindings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_binding_roundtrip_preserves_colons_and_scope_per_parent() {
        let entry = RegisteredPlugin {
            key: PackageKey {
                id: "plugin:qualified@market".into(),
                occurrence: stable_id("plugin-entry", &["native:installation"]),
            },
            path: PathBuf::from("/fixture/package"),
            origin: AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            },
            version: None,
        };
        let binding = SourceBinding::new(&entry, SourceRole::Root);
        let source = source_spec(
            entry.path.clone(),
            &entry.path,
            AgentAssetSourceKind::Directory,
            vec![binding.clone()],
        );
        assert!(source_bindings(&source).is_some_and(|values| values == vec![binding]));
        let mut forged = source;
        forged
            .allowed_logical_origins
            .push(AgentAssetLogicalOrigin {
                scope: AgentAssetScope::Managed,
                precedence: 40,
            });
        assert!(source_bindings(&forged).is_none());
    }
}
