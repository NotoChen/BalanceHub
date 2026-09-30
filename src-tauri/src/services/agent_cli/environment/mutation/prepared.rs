use super::super::{
    mechanism::{evaluate_mechanism, AgentMechanismEvaluation},
    verified_path::VerifiedPathAnchor,
};
use super::{
    execution::{ExactCliCommand, ExecutableStamp},
    files::{conflict, GuardedDirectory, GuardedFile},
};
use crate::{models::*, services::agent_cli::definition};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{atomic::AtomicBool, Arc},
};

pub(crate) type AgentAssetMechanismCatalog = fn(AgentCliKind) -> Vec<AgentAssetMechanismRecord>;
pub(crate) type AgentAssetMutationPreparer =
    for<'a> fn(MutationPreparation<'a>) -> Result<PreparedMutation, AgentAssetMutationError>;
pub(crate) type AgentAssetNativeUnavailable =
    fn(AgentAssetCategory, AgentAssetActionKind) -> AgentAssetActionUnavailableReason;

pub(crate) struct MutationInventory {
    pub(crate) inventory: AgentEnvironmentInventory,
    pub(crate) source_anchors: BTreeMap<String, VerifiedPathAnchor>,
}

pub(crate) trait MutationInspector: Send + Sync {
    fn inspect(&self) -> Result<MutationInventory, AgentAssetMutationError>;
    fn inspect_for_read(
        &self,
        _canceled: Arc<AtomicBool>,
    ) -> Result<MutationInventory, AgentAssetMutationError> {
        self.inspect()
    }
    fn home(&self) -> &Path;
    fn workspace(&self) -> Option<&Path>;
    fn settings(&self) -> &AppSettings;

    fn prepare(
        &self,
        request: MutationPreparation<'_>,
    ) -> Result<PreparedMutation, AgentAssetMutationError> {
        definition(request.asset.agent_kind)
            .environment
            .prepare_mutation(request)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct MutationPreparation<'a> {
    pub(crate) inventory: &'a AgentEnvironmentInventory,
    pub(crate) source_anchors: &'a BTreeMap<String, VerifiedPathAnchor>,
    pub(crate) context: &'a AgentConfigurationContext,
    pub(crate) asset: &'a AgentAssetRecord,
    pub(crate) installation: &'a AgentInstallation,
    pub(crate) mechanism: &'a AgentAssetMechanismRecord,
    pub(crate) action: AgentAssetActionKind,
    pub(crate) home: &'a Path,
    pub(crate) workspace: Option<&'a Path>,
}

impl MutationPreparation<'_> {
    pub(crate) fn source(
        &self,
        source_id: &str,
    ) -> Result<&AgentAssetSource, AgentAssetMutationError> {
        self.inventory
            .sources
            .iter()
            .find(|source| source.context_id == self.context.id && source.id == source_id)
            .ok_or_else(conflict)
    }

    pub(crate) fn source_at(
        &self,
        path: &Path,
    ) -> Result<&AgentAssetSource, AgentAssetMutationError> {
        self.inventory
            .sources
            .iter()
            .find(|source| source.context_id == self.context.id && Path::new(&source.path) == path)
            .ok_or_else(conflict)
    }

    pub(crate) fn file(
        &self,
        source: &AgentAssetSource,
    ) -> Result<GuardedFile, AgentAssetMutationError> {
        if source.context_id != self.context.id {
            return Err(conflict());
        }
        GuardedFile::capture(source, self.source_anchors)
    }

    pub(crate) fn context_files(&self) -> Result<Vec<GuardedFile>, AgentAssetMutationError> {
        let mut categories = BTreeSet::from([self.asset.category]);
        for affected in &self.asset.relationships.affected_asset_ids {
            if let Some(asset) = self
                .inventory
                .assets
                .iter()
                .find(|asset| &asset.stable_id == affected)
            {
                categories.insert(asset.category);
            }
        }
        self.inventory
            .sources
            .iter()
            .filter(|source| {
                source.context_id == self.context.id
                    && source.source_kind == AgentAssetSourceKind::File
                    && source
                        .categories
                        .iter()
                        .any(|category| categories.contains(category))
            })
            .map(|source| self.file(source))
            .collect()
    }

    pub(crate) fn desired_enabled(&self) -> Result<bool, AgentAssetMutationError> {
        match self.action {
            AgentAssetActionKind::Enable => Ok(true),
            AgentAssetActionKind::Disable => Ok(false),
            _ => Err(AgentAssetMutationError::new(
                AgentAssetMutationErrorKind::InvalidRequest,
            )),
        }
    }

    pub(crate) fn cwd(&self) -> &Path {
        self.workspace.unwrap_or(self.home)
    }

    pub(crate) fn boolean_change(&self, path: &Path, label: &str) -> AgentAssetPlanChange {
        AgentAssetPlanChange {
            label: label.to_owned(),
            path: Some(path.to_string_lossy().into_owned()),
            before: Some(
                match self.asset.declared_state {
                    AgentAssetState::Enabled => "已启用",
                    AgentAssetState::Disabled => "已禁用",
                    _ => "由原生配置决定",
                }
                .to_owned(),
            ),
            after: Some(
                if self.action == AgentAssetActionKind::Enable {
                    "启用"
                } else {
                    "禁用"
                }
                .to_owned(),
            ),
        }
    }
}

pub(crate) enum MutationExecution {
    AtomicFile {
        source_id: String,
        replacement: Vec<u8>,
    },
    ExactCli(Box<ExactCliCommand>),
}

pub(crate) struct PreparedMutation {
    pub(crate) execution: MutationExecution,
    pub(crate) files: Vec<GuardedFile>,
    pub(crate) write_source_ids: Vec<String>,
    pub(crate) domains: Vec<String>,
    pub(crate) changes: Vec<AgentAssetPlanChange>,
    pub(crate) affected_asset_ids: Vec<String>,
    pub(crate) verify: for<'a> fn(MutationVerification<'a>) -> bool,
    executable: ExecutableStamp,
    directories: Vec<GuardedDirectory>,
    pub(crate) signature: String,
}

pub(crate) struct MutationVerification<'a> {
    pub(crate) inventory: &'a AgentEnvironmentInventory,
    pub(crate) asset_id: &'a str,
    pub(crate) action: AgentAssetActionKind,
    pub(crate) affected_asset_ids: &'a [String],
}

impl MutationVerification<'_> {
    pub(crate) fn effective_state_matches(&self) -> bool {
        let desired = if self.action == AgentAssetActionKind::Enable {
            AgentAssetState::Enabled
        } else {
            AgentAssetState::Disabled
        };
        self.inventory
            .assets
            .iter()
            .find(|asset| asset.stable_id == self.asset_id)
            .is_some_and(|asset| {
                asset.effective_state == desired
                    && asset.resolution.terminal.is_none()
                    && asset.resolution.relation != AgentAssetResolutionRelation::Replaced
            })
            && self.affected_asset_ids.iter().any(|id| id == self.asset_id)
            && self.affected_asset_ids.iter().all(|id| {
                self.inventory
                    .assets
                    .iter()
                    .find(|asset| &asset.stable_id == id)
                    .is_some_and(|asset| {
                        !matches!(
                            asset.effective_state,
                            AgentAssetState::Unknown | AgentAssetState::Invalid
                        ) && asset.resolution.terminal
                            != Some(AgentAssetResolutionTerminal::Unknown)
                    })
            })
    }

    pub(crate) fn parent_state_matches(&self) -> bool {
        if !self.effective_state_matches() {
            return false;
        }
        self.affected_asset_ids
            .iter()
            .filter(|id| id.as_str() != self.asset_id)
            .all(|id| {
                let Some(child) = self
                    .inventory
                    .assets
                    .iter()
                    .find(|asset| &asset.stable_id == id)
                else {
                    return false;
                };
                if !self.is_provided_child(child) {
                    return false;
                }
                // Enabling the parent never overrides a child's own disabled,
                // shadowing, trust or policy. Disabling may not leave children live.
                self.action == AgentAssetActionKind::Enable
                    || matches!(
                        child.effective_state,
                        AgentAssetState::Disabled
                            | AgentAssetState::Blocked
                            | AgentAssetState::Shadowed
                    )
            })
    }

    pub(crate) fn asset_removed(&self) -> bool {
        !self
            .inventory
            .assets
            .iter()
            .any(|asset| asset.stable_id == self.asset_id)
    }

    pub(crate) fn is_provided_child(&self, child: &AgentAssetRecord) -> bool {
        let mut cursor = child;
        let mut seen = BTreeSet::new();
        loop {
            let Some(parent_id) = cursor.relationships.provided_by.as_ref() else {
                return false;
            };
            if parent_id == self.asset_id {
                return true;
            }
            if !seen.insert(parent_id) {
                return false;
            }
            let Some(parent) = self
                .inventory
                .assets
                .iter()
                .find(|asset| &asset.stable_id == parent_id)
            else {
                return false;
            };
            cursor = parent;
        }
    }
}

impl PreparedMutation {
    pub(crate) fn new(
        request: MutationPreparation<'_>,
        execution: MutationExecution,
        files: Vec<GuardedFile>,
        write_source_ids: Vec<String>,
        changes: Vec<AgentAssetPlanChange>,
        affected_asset_ids: Vec<String>,
        verify: for<'a> fn(MutationVerification<'a>) -> bool,
    ) -> Result<Self, AgentAssetMutationError> {
        let all_sources: BTreeSet<_> = files.iter().map(|file| file.source_id.as_str()).collect();
        if write_source_ids.is_empty()
            || write_source_ids
                .iter()
                .any(|id| !all_sources.contains(id.as_str()))
        {
            return Err(AgentAssetMutationError::new(
                AgentAssetMutationErrorKind::PreparationFailed,
            ));
        }
        let mut domains: Vec<_> = files
            .iter()
            .filter(|file| write_source_ids.contains(&file.source_id))
            .flat_map(GuardedFile::lock_domains)
            .collect();
        // A single configuration domain also covers a CLI's secondary files and
        // future sibling rows, across every selected installation and workspace.
        domains.push(format!("config:{}", request.context.config_root));
        domains.push(format!("installation:{}", request.installation.id));
        domains.sort();
        domains.dedup();
        let mut affected_asset_ids = affected_asset_ids;
        affected_asset_ids.push(request.asset.stable_id.clone());
        affected_asset_ids.sort();
        affected_asset_ids.dedup();
        if affected_asset_ids.iter().any(|id| {
            !request
                .inventory
                .assets
                .iter()
                .any(|asset| &asset.stable_id == id)
        }) {
            return Err(conflict());
        }
        let categories: BTreeSet<_> = request
            .inventory
            .assets
            .iter()
            .filter(|asset| affected_asset_ids.contains(&asset.stable_id))
            .map(|asset| asset.category)
            .collect();
        let directories = request
            .inventory
            .sources
            .iter()
            .filter(|source| {
                source.context_id == request.context.id
                    && source.source_kind == AgentAssetSourceKind::Directory
                    && source
                        .categories
                        .iter()
                        .any(|category| categories.contains(category))
            })
            .map(|source| GuardedDirectory::capture(source, request.source_anchors))
            .collect::<Result<Vec<_>, _>>()?;
        let executable = ExecutableStamp::capture(request.installation)?;
        let mut digest = Sha256::new();
        let affected_assets: Vec<_> = request.inventory.assets.iter().filter(|asset| affected_asset_ids.contains(&asset.stable_id))
            .map(|asset| serde_json::json!({ "id": asset.stable_id, "revision": asset.revision.identity,
                "declared": asset.declared_state, "effective": asset.effective_state, "trust": asset.trust_state,
                "resolution": asset.resolution, "relationships": asset.relationships, "details": asset.details }))
            .collect();
        let semantic = serde_json::json!({
            "context": { "id": request.context.id, "root": request.context.config_root, "profile": request.context.profile,
                "workspace": request.context.workspace_id, "trust": request.context.trust_context, "schema": request.context.parser_version },
            "asset": { "id": request.asset.stable_id, "nativeId": request.asset.native_id, "category": request.asset.category,
                "scope": request.asset.scope, "sources": request.asset.source_ids, "declared": request.asset.declared_state,
                "effective": request.asset.effective_state, "trust": request.asset.trust_state,
                "resolution": request.asset.resolution, "relationships": request.asset.relationships, "details": request.asset.details },
            "installation": { "id": request.installation.id, "identity": request.installation.executable_identity,
                "revision": request.installation.executable_revision, "version": request.installation.installed_version,
                "distribution": request.installation.distribution },
            "mechanism": request.mechanism, "action": request.action, "domains": domains,
            "changes": changes, "affected": affected_assets,
        });
        digest.update(serde_json::to_vec(&semantic).map_err(|_| conflict())?);
        let mut file_signatures: Vec<_> = files.iter().map(GuardedFile::signature).collect();
        file_signatures.sort();
        for signature in file_signatures {
            digest.update(signature.as_bytes());
        }
        let mut directory_signatures: Vec<_> = directories
            .iter()
            .map(GuardedDirectory::signature)
            .collect();
        directory_signatures.sort();
        for signature in directory_signatures {
            digest.update(signature.as_bytes());
        }
        match &execution {
            MutationExecution::AtomicFile {
                source_id,
                replacement,
            } => {
                digest.update(source_id.as_bytes());
                digest.update(replacement);
            }
            MutationExecution::ExactCli(command) => {
                digest.update(command.signature().as_bytes());
            }
        }
        Ok(Self {
            execution,
            files,
            write_source_ids,
            domains,
            changes,
            affected_asset_ids,
            verify,
            executable,
            directories,
            signature: format!("{:x}", digest.finalize()),
        })
    }

    pub(crate) fn revalidate(&self) -> Result<(), AgentAssetMutationError> {
        for file in &self.files {
            file.revalidate()?;
        }
        for directory in &self.directories {
            directory.revalidate()?;
        }
        self.executable.revalidate()
    }
}

pub(crate) fn prepare_request(
    inspector: &dyn MutationInspector,
    snapshot: &MutationInventory,
    request: &AgentAssetPlanRequest,
) -> Result<(PreparedMutation, AgentAssetPlan), AgentAssetMutationError> {
    if !matches!(
        request.action,
        AgentAssetActionKind::Enable | AgentAssetActionKind::Disable | AgentAssetActionKind::Remove
    ) {
        return Err(AgentAssetMutationError::new(
            AgentAssetMutationErrorKind::InvalidRequest,
        ));
    }
    let inventory = &snapshot.inventory;
    let asset = inventory
        .assets
        .iter()
        .find(|asset| asset.stable_id == request.asset_id)
        .ok_or_else(conflict)?;
    if asset.revision.identity != request.expected_revision {
        return Err(conflict());
    }
    let action = asset
        .actions
        .iter()
        .find(|action| action.action == request.action)
        .ok_or_else(conflict)?;
    if !action.available {
        return Err(AgentAssetMutationError::unavailable(
            action
                .reason
                .unwrap_or(AgentAssetActionUnavailableReason::NoOfficialMechanism),
        ));
    }
    let context = inventory
        .contexts
        .iter()
        .find(|context| context.id == asset.context_id)
        .ok_or_else(conflict)?;
    let installation_id = request
        .installation_id
        .as_ref()
        .or(action.selected_installation_id.as_ref())
        .ok_or_else(conflict)?;
    let installation = inventory
        .installations
        .iter()
        .find(|installation| &installation.id == installation_id)
        .ok_or_else(conflict)?;
    let target = AgentNativeTarget {
        platform: inventory.environment.host_platform,
        architecture: inventory.environment.host_architecture,
    };
    let mechanism = match evaluate_mechanism(
        &inventory.mechanisms,
        installation,
        context,
        asset,
        request.action,
        target,
    ) {
        AgentMechanismEvaluation::Matched(mechanism) => mechanism,
        AgentMechanismEvaluation::Unavailable { reason, .. } => {
            return Err(AgentAssetMutationError::unavailable(reason))
        }
    };
    let preparation = MutationPreparation {
        inventory,
        source_anchors: &snapshot.source_anchors,
        context,
        asset,
        installation,
        mechanism,
        action: request.action,
        home: inspector.home(),
        workspace: inspector.workspace(),
    };
    let prepared = inspector.prepare(preparation)?;
    prepared.revalidate()?;
    let plan = AgentAssetPlan {
        token: String::new(),
        asset_id: asset.stable_id.clone(),
        action: request.action,
        title: match request.action {
            AgentAssetActionKind::Enable => "确认启用资产",
            AgentAssetActionKind::Disable => "确认禁用资产",
            AgentAssetActionKind::Remove => "确认卸载资产",
            _ => "确认修改资产",
        }
        .into(),
        mechanism_id: mechanism.id.0.clone(),
        selected_installation_id: Some(installation.id.clone()),
        expires_at: String::new(),
        changes: prepared.changes.clone(),
        affected_asset_ids: prepared.affected_asset_ids.clone(),
        affected_installation_ids: context.compatible_installation_ids.clone(),
        source_ids: prepared.write_source_ids.clone(),
        reload_effect: mechanism.reload_effect.clone(),
        trust_effect: Some("保留当前信任与管理策略".into()),
    };
    Ok((prepared, plan))
}
