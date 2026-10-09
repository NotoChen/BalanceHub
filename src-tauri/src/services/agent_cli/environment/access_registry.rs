//! Current published inventory generations, with opaque target-bound access IDs.

use super::{
    identity::{lexical_absolute, stable_id},
    preview::AgentSourcePreviewPolicy,
    verified_path::VerifiedPathAnchor,
};
use crate::models::{
    AgentAssetAccess, AgentAssetAccessError, AgentAssetAccessErrorKind, AgentAssetAccessRisk,
    AgentAssetAccessUnavailableReason, AgentAssetAction, AgentAssetActionKind,
    AgentAssetActionUnavailableReason, AgentAssetDiagnostic, AgentAssetLimitKind, AgentAssetLimits,
    AgentAssetSource, AgentCliKind, AgentConfigurationContext, AgentEnvironmentInventory,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{btree_map::Entry, BTreeMap},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

const MAX_GENERATIONS: usize = 8;
const MAX_REGISTERED_TARGETS: usize = 16_384;

#[derive(Debug, Clone)]
pub(crate) struct AgentSourceAccessEvidence {
    pub(crate) source_id: String,
    pub(crate) anchor: VerifiedPathAnchor,
    pub(crate) policy: AgentSourcePreviewPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum AgentAssetAccessTargetKind {
    Source,
    Asset,
    ConfigurationSource,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AgentAssetAccessRequest<'a> {
    pub(crate) actor: &'a str,
    pub(crate) environment_id: &'a str,
    pub(crate) workspace: Option<&'a Path>,
    pub(crate) target_id: &'a str,
    pub(crate) access_id: &'a str,
    pub(crate) target_kind: AgentAssetAccessTargetKind,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum PublicationNamespace {
    Assets,
    Configuration(AgentCliKind),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct GenerationKey {
    namespace: PublicationNamespace,
    actor: String,
    environment_id: String,
    workspace: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct AccessBinding {
    actor: String,
    generation: u64,
    environment_id: String,
    workspace: Option<PathBuf>,
    workspace_id: Option<String>,
    context_id: String,
    agent_kind: AgentCliKind,
    target_id: String,
    target_kind: AgentAssetAccessTargetKind,
    inspection_source_id: String,
    projection_identity: String,
    schema_identity: String,
    policy_identity: String,
    source_revision: String,
    invalid_document: bool,
}

#[derive(Debug)]
pub(crate) struct AgentAssetAccessAnchor {
    pub(crate) access_id: String,
    pub(crate) verified: Arc<VerifiedPathAnchor>,
    pub(crate) policy: AgentSourcePreviewPolicy,
    pub(crate) sensitive: bool,
    pub(crate) max_read_bytes: usize,
    pub(crate) invalid_document: bool,
    binding: AccessBinding,
    actions: Vec<AgentAssetAction>,
}

impl AgentAssetAccessAnchor {
    pub(crate) fn require_action(
        &self,
        action: AgentAssetActionKind,
    ) -> Result<&AgentAssetAction, AgentAssetAccessError> {
        self.actions
            .iter()
            .find(|item| item.action == action && item.available)
            .ok_or_else(|| AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessUnavailable))
    }
}

#[derive(Debug)]
struct PublishedGeneration {
    sequence: u64,
    entries: BTreeMap<String, Arc<AgentAssetAccessAnchor>>,
    configurations: BTreeMap<String, Arc<AgentConfigurationAccessAnchor>>,
}

#[derive(Debug, Default)]
struct RegistryState {
    generations: BTreeMap<GenerationKey, PublishedGeneration>,
    requested: BTreeMap<GenerationKey, u64>,
}

#[derive(Debug, Default)]
pub(crate) struct AgentAssetAccessRegistry {
    sequence: AtomicU64,
    state: Mutex<RegistryState>,
}

#[derive(Debug)]
pub(crate) struct AgentAssetPublishTicket {
    key: GenerationKey,
    sequence: u64,
}

impl AgentAssetPublishTicket {
    pub(crate) fn sequence(&self) -> u64 {
        self.sequence
    }
}

struct PreparedSource {
    source_id: String,
    context_id: String,
    workspace_id: Option<String>,
    agent_kind: AgentCliKind,
    schema_identity: String,
    verified: Arc<VerifiedPathAnchor>,
    policy: AgentSourcePreviewPolicy,
    sensitive: bool,
    max_read_bytes: usize,
    invalid_document: bool,
}

struct AnchorInput<'a> {
    generation: u64,
    key: &'a GenerationKey,
    source: &'a PreparedSource,
    target_id: &'a str,
    target_kind: AgentAssetAccessTargetKind,
    projection_identity: String,
    actions: Vec<AgentAssetAction>,
    sensitive: bool,
}

impl AgentAssetAccessRegistry {
    /// Reserve before the asynchronous build starts. A later request wins even
    /// when an older scan takes longer to finish.
    pub(crate) fn begin_publish(
        &self,
        actor: &str,
        workspace: Option<&Path>,
    ) -> Result<AgentAssetPublishTicket, AgentAssetAccessError> {
        self.begin_publish_namespace(actor, workspace, PublicationNamespace::Assets)
    }

    pub(crate) fn begin_configuration_publish(
        &self,
        actor: &str,
        kind: AgentCliKind,
        workspace: Option<&Path>,
    ) -> Result<AgentAssetPublishTicket, AgentAssetAccessError> {
        self.begin_publish_namespace(actor, workspace, PublicationNamespace::Configuration(kind))
    }

    fn begin_publish_namespace(
        &self,
        actor: &str,
        workspace: Option<&Path>,
        namespace: PublicationNamespace,
    ) -> Result<AgentAssetPublishTicket, AgentAssetAccessError> {
        if actor.is_empty() {
            return Err(AgentAssetAccessError::new(
                AgentAssetAccessErrorKind::ActorMismatch,
            ));
        }
        let key = GenerationKey {
            namespace,
            actor: actor.to_string(),
            environment_id: super::inventory::native_environment().id,
            workspace: workspace_key(workspace)?,
        };
        let sequence = self
            .sequence
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessExpired))?
            + 1;
        let mut state = self.state.lock().map_err(|_| {
            AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessUnavailable)
        })?;
        if state
            .generations
            .get(&key)
            .is_some_and(|generation| generation.sequence > sequence)
        {
            return Err(AgentAssetAccessError::new(
                AgentAssetAccessErrorKind::AccessExpired,
            ));
        }
        state
            .requested
            .entry(key.clone())
            .and_modify(|current| *current = (*current).max(sequence))
            .or_insert(sequence);
        while state.requested.len() > MAX_GENERATIONS {
            let oldest = state
                .requested
                .iter()
                .min_by_key(|(_, sequence)| **sequence)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                state.requested.remove(&oldest);
            }
        }
        Ok(AgentAssetPublishTicket { key, sequence })
    }

    /// Only the inventory command calls this after the complete build succeeds.
    /// Building previews, version probes and mutation plans never publish.
    pub(crate) fn publish_ticket(
        &self,
        ticket: AgentAssetPublishTicket,
        inventory: &mut AgentEnvironmentInventory,
        evidence: Vec<AgentSourceAccessEvidence>,
    ) -> Result<(), AgentAssetAccessError> {
        let workspace = workspace_key(inventory.workspace.as_deref().map(Path::new))?;
        if ticket.key.environment_id != inventory.environment.id {
            return Err(AgentAssetAccessError::new(
                AgentAssetAccessErrorKind::EnvironmentMismatch,
            ));
        }
        if ticket.key.workspace != workspace {
            return Err(AgentAssetAccessError::new(
                AgentAssetAccessErrorKind::WorkspaceMismatch,
            ));
        }
        let key = ticket.key;
        let generation = ticket.sequence;
        let mut evidence_by_source = BTreeMap::<String, Option<AgentSourceAccessEvidence>>::new();
        for evidence in evidence {
            match evidence_by_source.entry(evidence.source_id.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(Some(evidence));
                }
                Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }
        let contexts = inventory
            .contexts
            .iter()
            .map(|context| (context.id.as_str(), context))
            .collect::<BTreeMap<_, _>>();
        let mut entries = BTreeMap::new();
        let mut prepared_sources = BTreeMap::new();
        for source in &mut inventory.sources {
            let context = contexts.get(source.context_id.as_str()).copied();
            let evidence = evidence_by_source.remove(&source.id).flatten();
            let Some((context, evidence)) = context.zip(evidence).filter(|(context, evidence)| {
                source_matches_evidence(source, context, evidence, &key)
            }) else {
                source.access = unavailable(if source_is_blocked(source) {
                    AgentAssetAccessUnavailableReason::SourceUnavailable
                } else {
                    AgentAssetAccessUnavailableReason::SnapshotUnavailable
                });
                prepare_actions(&mut source.actions, false, source.sensitive);
                continue;
            };
            if entries.len() >= MAX_REGISTERED_TARGETS {
                source.access = unavailable(AgentAssetAccessUnavailableReason::SnapshotUnavailable);
                prepare_actions(&mut source.actions, false, source.sensitive);
                continue;
            }
            prepare_actions(&mut source.actions, true, source.sensitive);
            let prepared = PreparedSource {
                source_id: source.id.clone(),
                context_id: context.id.clone(),
                workspace_id: source.workspace_id.clone(),
                agent_kind: context.agent_kind,
                schema_identity: fingerprint(&(context.parser_version, &context.schema_facts)),
                verified: Arc::new(evidence.anchor),
                policy: evidence.policy,
                sensitive: source.sensitive,
                max_read_bytes: inventory
                    .limits
                    .bytes_per_source
                    .min(inventory.limits.bytes_per_refresh)
                    .min(AgentAssetLimits::HARD_CAP.bytes_per_source),
                invalid_document: source
                    .diagnostics
                    .iter()
                    .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Malformed { .. })),
            };
            let anchor = create_anchor(AnchorInput {
                generation,
                key: &key,
                source: &prepared,
                target_id: &source.id,
                target_kind: AgentAssetAccessTargetKind::Source,
                projection_identity: fingerprint(&(
                    &source.id,
                    &source.context_id,
                    &source.revision.identity,
                    &source.categories,
                    source.scope,
                )),
                actions: source.actions.clone(),
                sensitive: source.sensitive,
            });
            source.access = AgentAssetAccess::Ready {
                access_id: anchor.access_id.clone(),
            };
            if entries
                .insert(anchor.access_id.clone(), Arc::new(anchor))
                .is_some()
            {
                return Err(AgentAssetAccessError::new(
                    AgentAssetAccessErrorKind::TargetMismatch,
                ));
            }
            prepared_sources.insert(source.id.clone(), prepared);
        }
        for asset in &mut inventory.assets {
            let prepared = prepared_sources
                .get(&asset.inspection_source_id)
                .filter(|source| {
                    source.context_id == asset.context_id
                        && source.workspace_id == asset.workspace_id
                        && source.agent_kind == asset.agent_kind
                        && asset.environment_id == key.environment_id
                        && asset.source_ids.contains(&asset.inspection_source_id)
                });
            let Some(prepared) = prepared.filter(|_| entries.len() < MAX_REGISTERED_TARGETS) else {
                asset.access = unavailable(AgentAssetAccessUnavailableReason::SnapshotUnavailable);
                prepare_actions(&mut asset.actions, false, asset.sensitive);
                continue;
            };
            let sensitive = asset.sensitive || prepared.sensitive;
            prepare_actions(&mut asset.actions, true, sensitive);
            let anchor = create_anchor(AnchorInput {
                generation,
                key: &key,
                source: prepared,
                target_id: &asset.stable_id,
                target_kind: AgentAssetAccessTargetKind::Asset,
                projection_identity: fingerprint(&(
                    &asset.represented_declaration_ids,
                    &asset.source_ids,
                    &asset.inspection_source_id,
                    &asset.resolution,
                    &asset.relationships,
                    &asset.details,
                    asset.declared_state,
                    asset.effective_state,
                    asset.trust_state,
                )),
                actions: asset.actions.clone(),
                sensitive,
            });
            asset.access = AgentAssetAccess::Ready {
                access_id: anchor.access_id.clone(),
            };
            if entries
                .insert(anchor.access_id.clone(), Arc::new(anchor))
                .is_some()
            {
                return Err(AgentAssetAccessError::new(
                    AgentAssetAccessErrorKind::TargetMismatch,
                ));
            }
        }
        let mut state = self.state.lock().map_err(|_| {
            AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessUnavailable)
        })?;
        // An older refresh completing late must not replace an already-published
        // newer generation for the same UI context.
        if state.requested.get(&key) != Some(&generation) {
            return Err(AgentAssetAccessError::new(
                AgentAssetAccessErrorKind::AccessExpired,
            ));
        }
        state.requested.remove(&key);
        state.generations.insert(
            key.clone(),
            PublishedGeneration {
                sequence: generation,
                entries,
                configurations: BTreeMap::new(),
            },
        );
        while state.generations.len() > MAX_GENERATIONS
            || state
                .generations
                .values()
                .map(|generation| generation.entries.len() + generation.configurations.len())
                .sum::<usize>()
                > MAX_REGISTERED_TARGETS
        {
            let oldest = state
                .generations
                .iter()
                .filter(|(candidate, _)| **candidate != key)
                .min_by_key(|(_, generation)| generation.sequence)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else { break };
            state.generations.remove(&oldest);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn publish(
        &self,
        actor: &str,
        inventory: &mut AgentEnvironmentInventory,
        evidence: Vec<AgentSourceAccessEvidence>,
    ) -> Result<(), AgentAssetAccessError> {
        let ticket = self.begin_publish(actor, inventory.workspace.as_deref().map(Path::new))?;
        self.publish_ticket(ticket, inventory, evidence)
    }

    pub(crate) fn resolve(
        &self,
        request: AgentAssetAccessRequest<'_>,
    ) -> Result<Arc<AgentAssetAccessAnchor>, AgentAssetAccessError> {
        let state = self.state.lock().map_err(|_| {
            AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessUnavailable)
        })?;
        let anchor = state
            .generations
            .values()
            .find_map(|generation| generation.entries.get(request.access_id))
            .ok_or_else(|| AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessExpired))?;
        let binding = &anchor.binding;
        let mismatch = if binding.actor != request.actor {
            Some(AgentAssetAccessErrorKind::ActorMismatch)
        } else if binding.target_kind != request.target_kind
            || binding.target_id != request.target_id
        {
            Some(AgentAssetAccessErrorKind::TargetMismatch)
        } else if binding.environment_id != request.environment_id {
            Some(AgentAssetAccessErrorKind::EnvironmentMismatch)
        } else if binding.workspace != workspace_key(request.workspace)? {
            Some(AgentAssetAccessErrorKind::WorkspaceMismatch)
        } else if binding.policy_identity != anchor.policy.identity() {
            Some(AgentAssetAccessErrorKind::SchemaChanged)
        } else {
            None
        };
        if let Some(kind) = mismatch {
            return Err(AgentAssetAccessError::new(kind));
        }
        Ok(Arc::clone(anchor))
    }

    pub(crate) fn remove_actor(&self, actor: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.generations.retain(|key, _| key.actor != actor);
            state.requested.retain(|key, _| key.actor != actor);
        }
    }
}

/// The shared registry owns ordinary and missing-file configuration authority.
/// A missing anchor never acquires preview/open permission through asset APIs.
pub(crate) struct AgentConfigurationAccessAnchor {
    pub(crate) authority:
        Arc<crate::services::agent_cli::configuration::sources::ConfigurationSourceAuthority>,
    binding: AccessBinding,
}
impl std::fmt::Debug for AgentConfigurationAccessAnchor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfigurationAccessAnchor")
            .field("source_id", &self.binding.target_id)
            .finish()
    }
}

impl AgentAssetAccessRegistry {
    pub(crate) fn publish_configuration(
        &self,
        ticket: AgentAssetPublishTicket,
        snapshot: &mut crate::models::AgentConfigurationSnapshot,
        authorities: Vec<
            Arc<crate::services::agent_cli::configuration::sources::ConfigurationSourceAuthority>,
        >,
    ) -> Result<(), AgentAssetAccessError> {
        if ticket.key.namespace != PublicationNamespace::Configuration(snapshot.agent_kind)
            || ticket.key.environment_id != snapshot.environment_id
            || ticket.key.workspace != workspace_key(snapshot.workspace.as_deref().map(Path::new))?
        {
            return Err(AgentAssetAccessError::new(
                AgentAssetAccessErrorKind::TargetMismatch,
            ));
        }
        let mut configurations = BTreeMap::new();
        for authority in authorities {
            let Some(source) = snapshot
                .sources
                .iter_mut()
                .find(|source| source.source_id == authority.source.source_id)
            else {
                continue;
            };
            let policy_identity = format!("configuration-v1:{:?}", authority.spec.format);
            let binding = AccessBinding {
                actor: ticket.key.actor.clone(),
                generation: ticket.sequence,
                environment_id: snapshot.environment_id.clone(),
                workspace: ticket.key.workspace.clone(),
                workspace_id: authority.context.workspace_id.clone(),
                context_id: source.context_id.clone(),
                agent_kind: source.agent_kind,
                target_id: source.source_id.clone(),
                target_kind: AgentAssetAccessTargetKind::ConfigurationSource,
                inspection_source_id: source.source_id.clone(),
                projection_identity: authority.file.signature(),
                schema_identity: fingerprint(&(
                    authority.context.parser_version,
                    &authority.context.schema_facts,
                )),
                policy_identity,
                source_revision: source.revision.identity.clone(),
                invalid_document: false,
            };
            let access_id = stable_id(
                "access",
                &[&fingerprint(&binding), &fingerprint(&source.actions)],
            );
            source.access = AgentAssetAccess::Ready {
                access_id: access_id.clone(),
            };
            configurations.insert(
                access_id,
                Arc::new(AgentConfigurationAccessAnchor { authority, binding }),
            );
        }
        let mut state = self.state.lock().map_err(|_| {
            AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessUnavailable)
        })?;
        if state.requested.get(&ticket.key) != Some(&ticket.sequence) {
            return Err(AgentAssetAccessError::new(
                AgentAssetAccessErrorKind::AccessExpired,
            ));
        }
        state.requested.remove(&ticket.key);
        state.generations.insert(
            ticket.key.clone(),
            PublishedGeneration {
                sequence: ticket.sequence,
                entries: BTreeMap::new(),
                configurations,
            },
        );
        while state.generations.len() > MAX_GENERATIONS {
            let oldest = state
                .generations
                .iter()
                .filter(|(key, _)| **key != ticket.key)
                .min_by_key(|(_, value)| value.sequence)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else {
                break;
            };
            state.generations.remove(&oldest);
        }
        Ok(())
    }

    pub(crate) fn resolve_configuration(
        &self,
        actor: &str,
        request: &crate::models::AgentConfigurationSourceRequest,
    ) -> Result<Arc<AgentConfigurationAccessAnchor>, AgentAssetAccessError> {
        let state = self.state.lock().map_err(|_| {
            AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessUnavailable)
        })?;
        let anchor = state
            .generations
            .values()
            .find_map(|generation| generation.configurations.get(&request.access_id))
            .ok_or_else(|| AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessExpired))?;
        let binding = &anchor.binding;
        let kind = if binding.actor != actor {
            Some(AgentAssetAccessErrorKind::ActorMismatch)
        } else if binding.target_kind != AgentAssetAccessTargetKind::ConfigurationSource
            || binding.target_id != request.source_id
        {
            Some(AgentAssetAccessErrorKind::TargetMismatch)
        } else if binding.environment_id != request.environment_id {
            Some(AgentAssetAccessErrorKind::EnvironmentMismatch)
        } else if binding.workspace != workspace_key(request.workspace.as_deref().map(Path::new))? {
            Some(AgentAssetAccessErrorKind::WorkspaceMismatch)
        } else if binding.source_revision != request.expected_revision {
            Some(AgentAssetAccessErrorKind::SourceChanged)
        } else {
            None
        };
        if let Some(kind) = kind {
            return Err(AgentAssetAccessError::new(kind));
        }
        Ok(Arc::clone(anchor))
    }

    /// Only private peers from the exact published generation can supply policy
    /// read guards or known-secret redaction. A later scan is never relabeled
    /// as the original edit's evidence.
    pub(crate) fn configuration_companions(
        &self,
        actor: &str,
        authority: &Arc<
            crate::services::agent_cli::configuration::sources::ConfigurationSourceAuthority,
        >,
    ) -> Result<
        Vec<Arc<crate::services::agent_cli::configuration::sources::ConfigurationSourceAuthority>>,
        AgentAssetAccessError,
    > {
        let state = self.state.lock().map_err(|_| {
            AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessUnavailable)
        })?;
        let generation = state
            .generations
            .values()
            .find(|generation| {
                generation.configurations.values().any(|anchor| {
                    anchor.binding.actor == actor && Arc::ptr_eq(&anchor.authority, authority)
                })
            })
            .ok_or_else(|| AgentAssetAccessError::new(AgentAssetAccessErrorKind::AccessExpired))?;
        Ok(generation
            .configurations
            .values()
            .map(|anchor| Arc::clone(&anchor.authority))
            .collect())
    }
}

fn create_anchor(input: AnchorInput<'_>) -> AgentAssetAccessAnchor {
    let binding = AccessBinding {
        actor: input.key.actor.clone(),
        generation: input.generation,
        environment_id: input.key.environment_id.clone(),
        workspace: input.key.workspace.clone(),
        workspace_id: input.source.workspace_id.clone(),
        context_id: input.source.context_id.clone(),
        agent_kind: input.source.agent_kind,
        target_id: input.target_id.to_string(),
        target_kind: input.target_kind,
        inspection_source_id: input.source.source_id.clone(),
        projection_identity: input.projection_identity,
        schema_identity: input.source.schema_identity.clone(),
        policy_identity: input.source.policy.identity(),
        source_revision: input.source.verified.revision().identity.clone(),
        invalid_document: input.source.invalid_document,
    };
    let access_id = stable_id(
        "access",
        &[&fingerprint(&binding), &fingerprint(&input.actions)],
    );
    AgentAssetAccessAnchor {
        access_id,
        verified: Arc::clone(&input.source.verified),
        policy: input.source.policy,
        sensitive: input.sensitive,
        max_read_bytes: input.source.max_read_bytes,
        invalid_document: input.source.invalid_document,
        binding,
        actions: input.actions,
    }
}

fn workspace_key(workspace: Option<&Path>) -> Result<Option<PathBuf>, AgentAssetAccessError> {
    workspace
        .map(|path| {
            lexical_absolute(path).ok_or_else(|| {
                AgentAssetAccessError::new(AgentAssetAccessErrorKind::WorkspaceMismatch)
            })
        })
        .transpose()
}

fn source_matches_evidence(
    source: &AgentAssetSource,
    context: &AgentConfigurationContext,
    evidence: &AgentSourceAccessEvidence,
    key: &GenerationKey,
) -> bool {
    !source_is_blocked(source)
        && source.environment_id == key.environment_id
        && context.environment_id == source.environment_id
        && context.workspace_id == source.workspace_id
        && source.id == evidence.source_id
        && lexical_absolute(Path::new(&source.path)).as_deref()
            == Some(evidence.anchor.display_path())
        && lexical_absolute(Path::new(&source.allowed_root)).as_deref()
            == Some(evidence.anchor.allowed_root())
        && source.source_kind == evidence.anchor.source_kind()
        && source.revision.identity == evidence.anchor.revision().identity
}

fn source_is_blocked(source: &AgentAssetSource) -> bool {
    source.revision.is_missing
        || source.revision.is_symlink
        || source.revision.is_directory
            != (source.source_kind == crate::models::AgentAssetSourceKind::Directory)
        || source.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic,
                AgentAssetDiagnostic::SymlinkRejected { .. }
                    | AgentAssetDiagnostic::ReadFailed { .. }
                    | AgentAssetDiagnostic::SourceOutsideAllowedRoot { .. }
                    | AgentAssetDiagnostic::SourceTypeMismatch { .. }
                    | AgentAssetDiagnostic::BudgetExceeded { .. }
                    | AgentAssetDiagnostic::Truncated {
                        limit: AgentAssetLimitKind::BytesPerSource
                            | AgentAssetLimitKind::BytesPerRefresh,
                        ..
                    }
            )
        })
}

fn prepare_actions(actions: &mut [AgentAssetAction], available: bool, sensitive: bool) {
    for action in actions {
        if matches!(
            action.action,
            AgentAssetActionKind::Preview
                | AgentAssetActionKind::Open
                | AgentAssetActionKind::Reveal
        ) {
            if !available {
                action.available = false;
                action.reason = Some(AgentAssetActionUnavailableReason::SourceUnavailable);
            }
            if matches!(
                action.action,
                AgentAssetActionKind::Open | AgentAssetActionKind::Reveal
            ) {
                action.confirmation_required = true;
                action.risks = vec![AgentAssetAccessRisk::ExternalPathnameRace];
                if sensitive {
                    action.risks.push(AgentAssetAccessRisk::RawSensitiveContent);
                }
            } else {
                action.confirmation_required = false;
                action.risks.clear();
            }
        }
    }
}

fn unavailable(reason: AgentAssetAccessUnavailableReason) -> AgentAssetAccess {
    AgentAssetAccess::Unavailable { reason }
}

fn fingerprint(value: &impl Serialize) -> String {
    let encoded = serde_json::to_vec(value).expect("private access evidence is JSON serializable");
    format!("{:x}", Sha256::digest(encoded))
}

#[cfg(test)]
pub(super) mod test_support;
#[cfg(test)]
mod tests;
