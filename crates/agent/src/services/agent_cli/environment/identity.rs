//! Stable identities for Agent environments, configuration contexts and sources.
//!
//! Identity functions intentionally use only canonical, user-controlled location
//! facts. Mutable observations such as versions, schema revisions and labels are
//! kept out of these hashes so a refresh does not silently create a new logical
//! object.

use crate::models::{
    AgentCliKind, AgentConfigurationContext, AgentEnvironmentDescriptor, AgentTrustState,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// Return the canonical path when it can be resolved, while retaining a stable
/// lexical path for a not-yet-created source.
pub(crate) fn canonical_identity(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Normalize an absolute path without consulting the filesystem.
///
/// Parent traversal is rejected instead of collapsed. Source specifications
/// are adapter input, so accepting `root/../outside` would let an adapter bug
/// bypass the common containment boundary before the snapshot layer runs.
pub(crate) fn lexical_absolute(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => return None,
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    Some(normalized)
}

pub(crate) fn lexical_identity(path: &Path) -> String {
    lexical_absolute(path)
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Hash a typed identity tuple with unambiguous component separators.
pub(crate) fn stable_id(prefix: &str, components: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for component in components {
        hasher.update(component.as_bytes());
        hasher.update([0]);
    }
    format!("{prefix}:{:x}", hasher.finalize())
}

pub(crate) fn context_stable_id(
    environment: &AgentEnvironmentDescriptor,
    kind: AgentCliKind,
    context: &AgentConfigurationContext,
) -> String {
    let workspace = context.workspace_id.as_deref().unwrap_or_default();
    let agent_key = kind.key();
    let config_root = canonical_identity(Path::new(&context.config_root));
    stable_id(
        "context",
        &[
            &environment.id,
            agent_key,
            config_root.as_str(),
            context.profile.as_str(),
            workspace,
            trust_key(context.trust_context),
        ],
    )
}

pub(crate) fn source_stable_id(context: &AgentConfigurationContext, path: &Path) -> String {
    // Source IDs are computed before a source is trusted. Keep this operation
    // purely lexical so an invalid/out-of-root path cannot trigger filesystem
    // traversal merely by being assigned an opaque ID.
    let lexical_path = lexical_identity(path);
    stable_id("source", &[context.id.as_str(), lexical_path.as_str()])
}

fn trust_key(state: AgentTrustState) -> &'static str {
    match state {
        AgentTrustState::Trusted => "trusted",
        AgentTrustState::Untrusted => "untrusted",
        AgentTrustState::Required => "required",
        AgentTrustState::Unknown => "unknown",
    }
}
