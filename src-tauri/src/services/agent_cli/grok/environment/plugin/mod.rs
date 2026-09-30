//! Grok package evidence stays private; discovery and assessment share decoders.
mod discovery;
pub(in crate::services::agent_cli::grok) mod manifest;
mod parse;
mod proof;

pub(super) use discovery::discover_sources;
pub(super) use parse::parse_source;
pub(in crate::services::agent_cli::grok) use proof::definition_suppressions;
pub(super) use proof::{is_plugin_payload, PluginIndex};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::models::{
    AgentAssetCategory, AgentAssetDiagnostic, AgentAssetDiscoveryIncompleteReason,
    AgentAssetNativeRef, AgentAssetScope, AgentAssetSourceKind,
};
use crate::services::agent_cli::contracts::{AgentAssetSnapshot, AgentDiagnosticOutput};

const SOURCE_PREFIX: &str = "grok-package:";
pub(in crate::services::agent_cli::grok) const MANIFESTS: [&str; 3] = [
    "plugin.json",
    ".grok-plugin/plugin.json",
    ".claude-plugin/plugin.json",
];
pub(in crate::services::agent_cli::grok) const CONVENTIONS: [(&str, AgentAssetSourceKind); 6] = [
    ("skills", AgentAssetSourceKind::Directory),
    ("commands", AgentAssetSourceKind::Directory),
    ("agents", AgentAssetSourceKind::Directory),
    (".mcp.json", AgentAssetSourceKind::File),
    (".lsp.json", AgentAssetSourceKind::File),
    ("hooks/hooks.json", AgentAssetSourceKind::File),
];
const PROBE_ID: &str = "__grok_package_evidence__";

/// Raw content is never serialized, displayed in facts, or exposed by Debug.
#[derive(Clone)]
pub(crate) struct GrokPluginPayload {
    revision: String,
    evidence: Evidence,
}

#[derive(Clone)]
enum Evidence {
    PackageDirectory,
    MissingManifest,
    Manifest(Value),
    Convention(Option<Arc<Value>>),
    Skill(String),
    Mcp(Arc<Value>),
    Hook {
        raw: Arc<Value>,
        event: String,
        group: usize,
        handler: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PackageRoot {
    source: String,
    directory: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum ParentEvidence {
    Manifest(u8),
    Convention(u8),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Parent {
    package: PackageRoot,
    package_revision: String,
    evidence: ParentEvidence,
    revision: String,
    namespace: String,
}

impl Parent {
    fn source_key(&self) -> SourceKey {
        SourceKey {
            package: self.package.clone(),
            kind: match self.evidence {
                ParentEvidence::Manifest(rank) => SourceKind::Manifest {
                    rank,
                    package_revision: self.package_revision.clone(),
                },
                ParentEvidence::Convention(component) => SourceKind::Convention {
                    component,
                    package_revision: self.package_revision.clone(),
                },
            },
        }
    }

    fn reference(&self) -> AgentAssetNativeRef {
        AgentAssetNativeRef {
            category: AgentAssetCategory::Plugin,
            native_id: self.namespace.clone(),
            qualifier: Some(format!("plugin:{}", self.namespace)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SourceKey {
    package: PackageRoot,
    kind: SourceKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SkillRole {
    command: bool,
    root_index: usize,
}

impl SkillRole {
    fn declaration_key(&self) -> String {
        format!(
            "{}:{}",
            if self.command { "command" } else { "SKILL.md" },
            self.root_index
        )
    }

    fn basename<'a>(&self, path: &'a Path, package_directory: &'a str) -> &'a str {
        let name = if self.command {
            path.file_stem()
        } else {
            path.parent().and_then(Path::file_name)
        };
        name.and_then(|name| name.to_str())
            .unwrap_or(package_directory)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FileRoles {
    skills: Vec<SkillRole>,
    mcp: bool,
    hooks: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum SourceKind {
    Package,
    Manifest {
        rank: u8,
        package_revision: String,
    },
    Convention {
        component: u8,
        package_revision: String,
    },
    SkillDirectory {
        parent: Parent,
        path: PathBuf,
    },
    ComponentFile {
        parent: Parent,
        path: PathBuf,
        roles: FileRoles,
    },
}

impl SourceKey {
    fn encode(&self) -> String {
        format!(
            "{SOURCE_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).expect("serializable source pointer"))
        )
    }

    fn decode(key: &str) -> Option<Self> {
        let bytes = URL_SAFE_NO_PAD
            .decode(key.strip_prefix(SOURCE_PREFIX)?)
            .ok()?;
        let value: Self = serde_json::from_slice(&bytes).ok()?;
        (value.encode() == key
            && matches!(
                value.package.source.as_str(),
                "plugins" | "workspace-plugins"
            )
            && single_component(&value.package.directory))
        .then_some(value)
    }

    fn relative_path(&self) -> Option<PathBuf> {
        match &self.kind {
            SourceKind::Package => Some(PathBuf::new()),
            SourceKind::Manifest { rank, .. } => {
                MANIFESTS.get(usize::from(*rank)).map(PathBuf::from)
            }
            SourceKind::Convention { component, .. } => CONVENTIONS
                .get(usize::from(*component))
                .map(|(path, _)| PathBuf::from(path)),
            SourceKind::SkillDirectory { path, .. } | SourceKind::ComponentFile { path, .. } => {
                clean_relative(path)
            }
        }
    }

    fn category(&self) -> AgentAssetCategory {
        match &self.kind {
            SourceKind::SkillDirectory { .. } => AgentAssetCategory::Skill,
            SourceKind::ComponentFile { roles, .. } => {
                if !roles.skills.is_empty() {
                    AgentAssetCategory::Skill
                } else if roles.hooks {
                    AgentAssetCategory::Hook
                } else {
                    AgentAssetCategory::Mcp
                }
            }
            _ => AgentAssetCategory::Plugin,
        }
    }

    fn source_kind(&self) -> Option<AgentAssetSourceKind> {
        match &self.kind {
            SourceKind::Package | SourceKind::SkillDirectory { .. } => {
                Some(AgentAssetSourceKind::Directory)
            }
            SourceKind::Convention { component, .. } => CONVENTIONS
                .get(usize::from(*component))
                .map(|(_, kind)| *kind),
            _ => Some(AgentAssetSourceKind::File),
        }
    }

    fn categories(&self) -> &'static [AgentAssetCategory] {
        match &self.kind {
            SourceKind::Manifest { .. } => &[
                AgentAssetCategory::Plugin,
                AgentAssetCategory::Mcp,
                AgentAssetCategory::Hook,
            ],
            SourceKind::Convention { component: 3, .. } => {
                &[AgentAssetCategory::Plugin, AgentAssetCategory::Mcp]
            }
            SourceKind::Convention { component: 5, .. } => {
                &[AgentAssetCategory::Plugin, AgentAssetCategory::Hook]
            }
            SourceKind::Convention {
                component: 0 | 1, ..
            } => &[AgentAssetCategory::Plugin, AgentAssetCategory::Skill],
            SourceKind::SkillDirectory { .. } => &[AgentAssetCategory::Skill],
            SourceKind::ComponentFile { roles, .. } => {
                match (roles.skills.is_empty(), roles.mcp, roles.hooks) {
                    (false, true, true) => &[
                        AgentAssetCategory::Skill,
                        AgentAssetCategory::Mcp,
                        AgentAssetCategory::Hook,
                    ],
                    (false, true, false) => &[AgentAssetCategory::Skill, AgentAssetCategory::Mcp],
                    (false, false, true) => &[AgentAssetCategory::Skill, AgentAssetCategory::Hook],
                    (false, false, false) => &[AgentAssetCategory::Skill],
                    (true, true, true) => &[AgentAssetCategory::Mcp, AgentAssetCategory::Hook],
                    (true, true, false) => &[AgentAssetCategory::Mcp],
                    (true, false, true) => &[AgentAssetCategory::Hook],
                    (true, false, false) => &[],
                }
            }
            SourceKind::Package | SourceKind::Convention { .. } => &[AgentAssetCategory::Plugin],
        }
    }

    fn scope(&self) -> AgentAssetScope {
        if self.package.source == "workspace-plugins" {
            AgentAssetScope::Workspace
        } else {
            AgentAssetScope::User
        }
    }

    fn precedence(&self) -> u32 {
        if self.scope() == AgentAssetScope::Workspace {
            20
        } else {
            10
        }
    }
}

fn single_component(value: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

/// Keep the lexical spelling for the shared no-follow gate. Collapsing `..`
/// could hide an intermediate symlink, so such paths remain unavailable.
fn clean_relative(path: &Path) -> Option<PathBuf> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => result.push(value),
            _ => return None,
        }
    }
    Some(result)
}

fn revision(snapshot: &AgentAssetSnapshot) -> &str {
    match snapshot {
        AgentAssetSnapshot::Missing { revision }
        | AgentAssetSnapshot::File { revision, .. }
        | AgentAssetSnapshot::DirectoryManifest { revision, .. }
        | AgentAssetSnapshot::Blocked { revision, .. } => &revision.identity,
    }
}

fn incomplete(
    output: &mut dyn AgentDiagnosticOutput,
    category: AgentAssetCategory,
    reason: AgentAssetDiscoveryIncompleteReason,
) {
    output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
        agent_kind: super::super::AGENT_KIND,
        category,
        reason,
    });
}
