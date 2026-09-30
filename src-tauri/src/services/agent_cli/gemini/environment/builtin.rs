//! Installation-relative resources verified against Gemini's npm package.
use crate::{
    models::{
        AgentAssetCategory, AgentAssetDiagnostic, AgentAssetDiscoveryIncompleteReason,
        AgentAssetInstallationOrigin, AgentAssetProviderOrigin, AgentAssetScope,
        AgentAssetSourceKind, AgentCliDistribution, AgentInstallation,
        AgentInstallationAvailability,
    },
    services::agent_cli::{
        contracts::{AgentSourceDiscoveryRequest, InitialSourceOutput},
        discovery::npm_package_owner,
        environment::{source, SourceInput},
        native_agent_kinds::gemini::AGENT_KIND,
    },
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

pub(super) const SOURCE_PREFIX: &str = "gemini-builtin-skills:";
const PACKAGE_NAME: &str = "@google/gemini-cli";

fn builtin_root(
    installation: &AgentInstallation,
) -> Result<Option<PathBuf>, AgentAssetDiscoveryIncompleteReason> {
    use AgentAssetDiscoveryIncompleteReason::{
        InstallationUnverified, SourceUnavailable, UnsupportedEntryPoint,
    };
    if installation.agent_kind != AGENT_KIND
        || installation.availability != AgentInstallationAvailability::Available
    {
        return Ok(None);
    }
    let version = installation
        .installed_version
        .as_deref()
        .ok_or(InstallationUnverified)?;
    match installation.distribution {
        AgentCliDistribution::Npm => {}
        AgentCliDistribution::Unknown => return Err(InstallationUnverified),
        _ => return Err(UnsupportedEntryPoint),
    }
    let identity = installation
        .executable_identity
        .as_ref()
        .ok_or(InstallationUnverified)?;
    let executable = Path::new(&identity.canonical_path);
    if !executable.is_absolute() {
        return Err(InstallationUnverified);
    }
    let owner =
        npm_package_owner(executable, PACKAGE_NAME, version).ok_or(InstallationUnverified)?;
    let bundle = owner.root.join("bundle");
    if owner.bin_path != bundle.join("gemini.js") {
        return Err(UnsupportedEntryPoint);
    }
    // Gemini's supported npm layout loads dirname(import.meta.url)/builtin,
    // including releases that split gemini.js into sibling chunks. Verify
    // package ownership and layout rather than pinning one release number. Do not
    // canonicalize this resource path: a replaced directory must not grant
    // access to a symlink target outside the verified package layout.
    let builtin = bundle.join("builtin");
    for directory in [&bundle, &builtin] {
        let metadata = fs::symlink_metadata(directory).map_err(|_| SourceUnavailable)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(SourceUnavailable);
        }
    }
    Ok(Some(builtin))
}

pub(in crate::services::agent_cli::gemini) fn readonly_external_roots(
    installations: &[AgentInstallation],
) -> Vec<PathBuf> {
    installations
        .iter()
        .filter_map(|installation| builtin_root(installation).ok().flatten())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn discover_sources(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let mut installations = request
        .installations
        .iter()
        .filter(|installation| {
            request
                .context
                .compatible_installation_ids
                .contains(&installation.id)
        })
        .collect::<Vec<_>>();
    installations.sort_by(|left, right| left.id.cmp(&right.id));
    let mut seen = BTreeSet::new();
    for installation in installations {
        // Reverify after installation/root discovery. A package replaced in
        // between must not inherit the earlier external-root admission.
        let root = match builtin_root(installation) {
            Ok(Some(root)) => root,
            Ok(None) => continue,
            Err(reason) => {
                output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
                    agent_kind: AGENT_KIND,
                    category: AgentAssetCategory::Skill,
                    reason,
                });
                continue;
            }
        };
        if !seen.insert(root.clone()) {
            continue;
        }
        let key = format!("{SOURCE_PREFIX}{}", installation.id);
        let mut spec = source(SourceInput {
            native_source_key: &key,
            label: "Gemini CLI 内置 Skills",
            path: root.clone(),
            allowed_root: &root,
            scope: AgentAssetScope::System,
            origin: AgentAssetInstallationOrigin::Bundled,
            precedence: 0,
            sensitive: false,
            source_kind: AgentAssetSourceKind::Directory,
            categories: &[AgentAssetCategory::Skill],
        });
        // This fact comes from the verified native package owner, independently
        // of the resource's System scope or Bundled installation origin.
        spec.provider = AgentAssetProviderOrigin::AgentVendor;
        if output.emit_initial(spec).is_break() {
            return;
        }
    }
}
