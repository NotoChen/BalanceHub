use super::{
    contracts::ConfigurationValidationRequest,
    errors::{conflict, internal},
    operations::{OperationCell, PrivatePlan},
    receipts::ReceiptStore,
    selection::DocumentSelection,
    sources::{self, ConfigurationSourceAuthority},
    EDIT_TTL, MAX_DOCUMENTS, MAX_DOCUMENT_BYTES, MAX_PRIVATE_BYTES, PLAN_TTL,
};
use crate::{
    models::*,
    services::agent_cli::{
        self,
        environment::{
            access_registry::AgentAssetAccessRegistry,
            mutation::{token::opaque_id, GuardedFile},
        },
    },
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub(crate) struct ConfigurationService {
    pub(super) home: PathBuf,
    pub(super) app_data_root: PathBuf,
    pub(super) access: Arc<AgentAssetAccessRegistry>,
    pub(super) state: Arc<Mutex<ServiceState>>,
    pub(super) receipts: ReceiptStore,
}
pub(super) struct EditDocument {
    pub authority: Arc<ConfigurationSourceAuthority>,
    pub selection: DocumentSelection,
    pub read_only_reason: Option<String>,
}
pub(super) struct EditInput {
    pub source: Arc<ConfigurationSourceAuthority>,
    pub text: String,
    pub selection: DocumentSelection,
    pub read_only_reason: Option<String>,
}
pub(super) struct EditRecord {
    pub actor: String,
    pub public: AgentConfigurationEdit,
    pub documents: Vec<EditDocument>,
    pub dependencies: Vec<GuardedFile>,
    pub expires: Instant,
    pub retained_bytes: usize,
    latest_plan_generation: AtomicU64,
}
pub(super) struct PlanRecord {
    pub actor: String,
    pub plan: PrivatePlan,
    pub expires: Instant,
}
// Bind the submitted text and publication generation before scheduling work.
pub(crate) struct ConfigurationPlanTicket {
    actor: String,
    request: AgentConfigurationSaveRequest,
    publication: u64,
    deadline: Instant,
}
#[derive(Default)]
pub(super) struct ServiceState {
    pub edits: BTreeMap<String, Arc<EditRecord>>,
    pub plans: BTreeMap<String, PlanRecord>,
    pub consumed: BTreeMap<String, (String, String, String, Instant)>,
    pub operations: BTreeMap<String, Arc<OperationCell>>,
    pub recovered: BTreeMap<String, AgentConfigurationOperation>,
}
impl ServiceState {
    fn prune(&mut self) {
        let now = Instant::now();
        self.edits.retain(|_, edit| edit.expires > now);
        self.plans
            .retain(|_, plan| plan.expires > now && self.edits.contains_key(&plan.plan.edit_id));
        self.consumed.retain(|_, (_, _, _, expires)| *expires > now);
        while self.operations.len() > 128 {
            let oldest = self
                .operations
                .iter()
                .filter(|(_, cell)| cell.is_complete())
                .min_by_key(|(_, cell)| cell.created)
                .map(|(id, _)| id.clone());
            let Some(oldest) = oldest else { break };
            self.operations.remove(&oldest);
        }
    }
    pub fn retained_bytes(&self) -> usize {
        self.edits
            .values()
            .map(|edit| edit.retained_bytes)
            .sum::<usize>()
            + self
                .plans
                .values()
                .map(|record| record.plan.retained_bytes())
                .sum::<usize>()
            + self
                .operations
                .values()
                .map(|cell| cell.retained_bytes())
                .sum::<usize>()
    }
}
impl ConfigurationService {
    pub(crate) fn new(
        home: PathBuf,
        app_data_root: PathBuf,
        access: Arc<AgentAssetAccessRegistry>,
    ) -> Self {
        let state = Arc::new(Mutex::new(ServiceState::default()));
        let weak = Arc::downgrade(&state);
        // Fixed lifetimes are enforced even when a modal closes or a window
        // stops sending requests. This thread only prunes private memory.
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(30));
            let Some(state) = weak.upgrade() else { break };
            if let Ok(mut state) = state.lock() {
                state.prune();
            };
        });
        Self {
            home,
            receipts: ReceiptStore::new(app_data_root.join("agent-configuration-operations")),
            app_data_root,
            access,
            state,
        }
    }
    pub(crate) fn remove_actor(&self, actor: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.edits.retain(|_, edit| edit.actor != actor);
            state.plans.retain(|_, plan| plan.actor != actor);
        }
    }
    pub(crate) fn discard_edit(
        &self,
        actor: &str,
        edit_id: &str,
    ) -> Result<(), AgentConfigurationError> {
        let mut state = self.state.lock().map_err(|_| internal())?;
        if state
            .edits
            .get(edit_id)
            .is_some_and(|edit| edit.actor != actor)
        {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::ActorMismatch,
            ));
        }
        state.edits.remove(edit_id);
        state
            .plans
            .retain(|_, plan| plan.plan.edit_id != edit_id || plan.actor != actor);
        Ok(())
    }
    pub(crate) fn open_source(
        &self,
        actor: &str,
        request: AgentConfigurationOpenRequest,
        opener: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<(), AgentConfigurationError> {
        let source_request = AgentConfigurationSourceRequest {
            source_id: request.source_id,
            access_id: request.access_id,
            environment_id: request.environment_id,
            workspace: request.workspace,
            expected_revision: request.expected_revision,
        };
        let anchor = self.source(actor, &source_request)?;
        let kind = match request.target {
            AgentAssetOpenTarget::Asset => AgentConfigurationActionKind::Open,
            AgentAssetOpenTarget::Reveal => AgentConfigurationActionKind::Reveal,
        };
        sources::require_action(&anchor.authority.source, kind)?;
        let action = anchor
            .authority
            .source
            .actions
            .iter()
            .find(|action| action.action == kind)
            .ok_or_else(internal)?;
        let verified = anchor.authority.file.verified_anchor().ok_or_else(|| {
            AgentConfigurationError::new(AgentConfigurationErrorKind::SourceUnavailable)
        })?;
        crate::services::agent_cli::environment::acknowledge_risks(
            &action.risks,
            &request.accepted_risks,
        )?;
        crate::services::agent_cli::environment::open_verified_source(
            verified,
            MAX_DOCUMENT_BYTES,
            |_, path| opener(path),
        )?;
        Ok(())
    }
    pub(crate) fn read_source(
        &self,
        actor: &str,
        request: AgentConfigurationSourceRequest,
    ) -> Result<AgentConfigurationReadResult, AgentConfigurationError> {
        let anchor = self.source(actor, &request)?;
        sources::require_action(&anchor.authority.source, AgentConfigurationActionKind::Read)?;
        let source = &anchor.authority;
        let text = std::str::from_utf8(source.file.bytes().unwrap_or_default()).map_err(|_| {
            AgentConfigurationError::new(AgentConfigurationErrorKind::UnsupportedFormat)
        })?;
        let mut text = text.to_owned();
        let truncated = text.len() > 128 * 1024;
        if truncated {
            let mut end = 128 * 1024;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        Ok(AgentConfigurationReadResult {
            source_id: request.source_id,
            revision: request.expected_revision,
            text,
            truncated,
            diagnostics: Vec::new(),
        })
    }
    pub(crate) fn begin_edit(
        &self,
        actor: &str,
        request: AgentConfigurationSourceRequest,
    ) -> Result<AgentConfigurationEdit, AgentConfigurationError> {
        let anchor = self.source(actor, &request)?;
        let source = Arc::clone(&anchor.authority);
        require_edit(&source)?;
        let base = initial_text(&source)?;
        self.admit_edit(actor, source.source.agent_kind, vec![(source, base)])
    }
    pub(crate) fn begin_provider_edit(
        &self,
        actor: &str,
        provider: &Provider,
        kind: AgentCliKind,
        key: &str,
        settings: &AppSettings,
    ) -> Result<AgentConfigurationEdit, AgentConfigurationError> {
        let snapshot = self.list_sources(
            actor,
            AgentConfigurationListRequest {
                agent_kind: kind,
                workspace: None,
            },
            Some(settings),
        )?;
        let adapter = agent_cli::definition(kind)
            .default_config()
            .ok_or_else(|| {
                AgentConfigurationError::new(AgentConfigurationErrorKind::UnsupportedFormat)
            })?;
        let candidates = adapter.candidates(kind, provider, key).map_err(|_| {
            AgentConfigurationError::new(AgentConfigurationErrorKind::SourceUnavailable)
        })?;
        if candidates.is_empty() || candidates.len() > MAX_DOCUMENTS {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::InvalidRequest,
            ));
        }
        let mut documents = Vec::new();
        let mut seen = BTreeSet::new();
        for candidate in candidates {
            let source = snapshot
                .sources
                .iter()
                .find(|source| Path::new(&source.path) == candidate.path)
                .ok_or_else(|| {
                    AgentConfigurationError::new(AgentConfigurationErrorKind::SourceUnavailable)
                })?;
            if !seen.insert(source.source_id.clone()) {
                return Err(AgentConfigurationError::new(
                    AgentConfigurationErrorKind::InvalidRequest,
                ));
            }
            let AgentAssetAccess::Ready { access_id } = &source.access else {
                return Err(AgentConfigurationError::new(
                    AgentConfigurationErrorKind::SourceUnavailable,
                ));
            };
            let anchor = self.source(
                actor,
                &AgentConfigurationSourceRequest {
                    source_id: source.source_id.clone(),
                    access_id: access_id.clone(),
                    environment_id: source.environment_id.clone(),
                    workspace: source.workspace.clone(),
                    expected_revision: source.revision.identity.clone(),
                },
            )?;
            require_edit(&anchor.authority)?;
            if anchor.authority.file.bytes() != candidate.before.as_deref().map(str::as_bytes) {
                return Err(conflict());
            }
            documents.push((Arc::clone(&anchor.authority), candidate.after));
        }
        self.admit_edit(actor, kind, documents)
    }
    fn admit_edit(
        &self,
        actor: &str,
        kind: AgentCliKind,
        inputs: Vec<(Arc<ConfigurationSourceAuthority>, String)>,
    ) -> Result<AgentConfigurationEdit, AgentConfigurationError> {
        self.admit_inputs(
            actor,
            kind,
            inputs
                .into_iter()
                .map(|(source, text)| EditInput {
                    source,
                    text,
                    selection: DocumentSelection::Whole,
                    read_only_reason: None,
                })
                .collect(),
            None,
        )
    }

    pub(super) fn admit_inputs(
        &self,
        actor: &str,
        kind: AgentCliKind,
        inputs: Vec<EditInput>,
        resource: Option<AgentResourceContent>,
    ) -> Result<AgentConfigurationEdit, AgentConfigurationError> {
        if inputs.is_empty()
            || inputs.len() > MAX_DOCUMENTS
            || inputs
                .iter()
                .any(|input| input.text.len() > MAX_DOCUMENT_BYTES)
        {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::CapacityExceeded,
            ));
        }
        let edit_id = opaque_id().map_err(|_| internal())?;
        let revision = opaque_id().map_err(|_| internal())?;
        let companions = if resource.is_none() {
            self.access
                .configuration_companions(actor, &inputs[0].source)?
        } else {
            Vec::new()
        };
        let mut documents = Vec::new();
        let mut public_documents = Vec::new();
        let mut dependencies = Vec::new();
        let mut retained_bytes = 0usize;
        for EditInput {
            source,
            text,
            selection,
            read_only_reason,
        } in inputs
        {
            let before =
                std::str::from_utf8(source.file.bytes().unwrap_or_default()).map_err(|_| {
                    AgentConfigurationError::new(AgentConfigurationErrorKind::UnsupportedFormat)
                })?;
            retained_bytes = retained_bytes
                .saturating_add(before.len().saturating_mul(2))
                .saturating_add(text.len());
            public_documents.push(AgentConfigurationEditableDocument {
                source_id: source.source.source_id.clone(),
                label: source.source.label.clone(),
                path: source.source.path.clone(),
                format: source.spec.format,
                creating: source.file.bytes().is_none(),
                original_text: selection.text(before)?,
                text,
                read_only_reason: read_only_reason.clone(),
            });
            documents.push(EditDocument {
                authority: source,
                selection,
                read_only_reason,
            });
        }
        for source in companions {
            if !source.source.actions.iter().any(|action| {
                action.available
                    && matches!(
                        action.action,
                        AgentConfigurationActionKind::Edit | AgentConfigurationActionKind::Create
                    )
            }) {
                source.file.revalidate().map_err(|_| conflict())?;
                dependencies.push(source.file.clone());
            }
        }
        if self.app_data_root.is_dir() {
            dependencies.push(
                crate::services::agent_runtime::managed_hook::configuration_ownership_guard(
                    &self.app_data_root,
                    kind,
                )
                .map_err(|_| {
                    AgentConfigurationError::new(AgentConfigurationErrorKind::SourceUnavailable)
                })?,
            );
        }
        retained_bytes = retained_bytes.saturating_add(
            dependencies
                .iter()
                .map(|file| file.bytes().map_or(0, <[u8]>::len))
                .sum::<usize>(),
        );
        let public = AgentConfigurationEdit {
            edit_id: edit_id.clone(),
            revision,
            expires_at: at_after(EDIT_TTL),
            agent_kind: kind,
            documents: public_documents,
            diagnostics: Vec::new(),
            resource,
        };
        let record = EditRecord {
            actor: actor.to_owned(),
            public: public.clone(),
            documents,
            dependencies,
            expires: Instant::now() + EDIT_TTL,
            retained_bytes,
            latest_plan_generation: AtomicU64::new(0),
        };
        let mut state = self.state.lock().map_err(|_| internal())?;
        state.prune();
        if state.edits.len() >= 32
            || state
                .edits
                .values()
                .filter(|edit| edit.actor == actor)
                .count()
                >= 8
            || state.retained_bytes().saturating_add(retained_bytes) > MAX_PRIVATE_BYTES
        {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::CapacityExceeded,
            ));
        }
        state.edits.insert(edit_id, Arc::new(record));
        Ok(public)
    }
    pub(crate) fn begin_plan_save(
        &self,
        actor: &str,
        request: AgentConfigurationSaveRequest,
    ) -> Result<ConfigurationPlanTicket, AgentConfigurationError> {
        let mut state = self.state.lock().map_err(|_| internal())?;
        state.prune();
        let edit = state.edits.get(&request.edit_id).ok_or_else(|| {
            AgentConfigurationError::new(AgentConfigurationErrorKind::EditExpired)
        })?;
        if edit.actor != actor {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::ActorMismatch,
            ));
        }
        if edit.public.revision != request.expected_revision {
            return Err(conflict());
        }
        let publication = edit
            .latest_plan_generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |generation| {
                generation.checked_add(1)
            })
            .map_err(|_| {
                AgentConfigurationError::new(AgentConfigurationErrorKind::CapacityExceeded)
            })?
            + 1;
        Ok(ConfigurationPlanTicket {
            actor: actor.to_owned(),
            request,
            publication,
            deadline: Instant::now() + Duration::from_secs(10),
        })
    }
    pub(crate) fn plan_save_with_ticket(
        &self,
        ticket: ConfigurationPlanTicket,
    ) -> Result<AgentConfigurationPlan, AgentConfigurationError> {
        let ConfigurationPlanTicket {
            actor,
            request,
            publication,
            deadline,
        } = ticket;
        let actor = actor.as_str();
        if Instant::now() >= deadline {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::Timeout,
            ));
        }
        let edit = {
            let mut state = self.state.lock().map_err(|_| internal())?;
            state.prune();
            let edit = state.edits.get(&request.edit_id).ok_or_else(|| {
                AgentConfigurationError::new(AgentConfigurationErrorKind::EditExpired)
            })?;
            if edit.latest_plan_generation.load(Ordering::Acquire) != publication {
                return Err(AgentConfigurationError::new(
                    AgentConfigurationErrorKind::PlanExpired,
                ));
            }
            Arc::clone(edit)
        };
        let expected = edit
            .documents
            .iter()
            .map(|document| document.authority.source.source_id.as_str())
            .collect::<BTreeSet<_>>();
        if request.documents.len() != expected.len()
            || request
                .documents
                .iter()
                .map(|document| document.source_id.as_str())
                .collect::<BTreeSet<_>>()
                != expected
            || request
                .documents
                .iter()
                .any(|document| document.text.len() > MAX_DOCUMENT_BYTES)
        {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::InvalidRequest,
            ));
        }
        let mut files = Vec::new();
        let mut changes = Vec::new();
        let mut diagnostics = Vec::new();
        let mut reload_hints = Vec::new();
        for document in &edit.documents {
            if Instant::now() >= deadline {
                return Err(AgentConfigurationError::new(
                    AgentConfigurationErrorKind::Timeout,
                ));
            }
            let source = &document.authority;
            source.file.revalidate().map_err(|_| conflict())?;
            self.revalidate_context(source)?;
            let candidate = request
                .documents
                .iter()
                .find(|document| document.source_id == source.source.source_id)
                .ok_or_else(|| {
                    AgentConfigurationError::new(AgentConfigurationErrorKind::InvalidRequest)
                })?
                .text
                .clone();
            let before = source
                .file
                .bytes()
                .map(std::str::from_utf8)
                .transpose()
                .map_err(|_| {
                    AgentConfigurationError::new(AgentConfigurationErrorKind::UnsupportedFormat)
                })?;
            let original_text = document.selection.text(before.unwrap_or_default())?;
            if candidate == original_text && before.is_some() {
                continue;
            }
            if document.read_only_reason.is_some() {
                return Err(AgentConfigurationError::new(
                    AgentConfigurationErrorKind::ReadOnly,
                ));
            }
            require_edit(source)?;
            let candidate = document
                .selection
                .replace(before.unwrap_or_default(), &candidate)?;
            if candidate.len() > MAX_DOCUMENT_BYTES {
                return Err(AgentConfigurationError::new(
                    AgentConfigurationErrorKind::CapacityExceeded,
                ));
            }
            let validate = if edit.public.resource.is_some() {
                super::native_support::validate
            } else {
                agent_cli::definition(source.source.agent_kind)
                    .configuration()
                    .validate
            };
            let validation = validate(ConfigurationValidationRequest {
                source: &source.spec,
                after: &candidate,
            })?;
            if validation.diagnostics.iter().any(|diagnostic| {
                diagnostic.severity == AgentConfigurationDiagnosticSeverity::Error
            }) {
                return Err(AgentConfigurationError {
                    kind: AgentConfigurationErrorKind::UnsupportedScope,
                    message: "配置修改不适用于此来源，请使用对应资源入口".to_owned(),
                    diagnostics: validation.diagnostics,
                });
            }
            diagnostics.extend(validation.diagnostics);
            reload_hints.extend(validation.reload_hints);
            changes.push(AgentAssetPlanChange {
                label: source.source.label.clone(),
                path: Some(source.source.path.clone()),
                before: before.map(str::to_owned),
                after: Some(candidate.clone()),
            });
            files.push(super::operations::PlannedFile {
                source: Arc::clone(source),
                text: candidate,
            });
        }
        if files.is_empty() {
            return Err(AgentConfigurationError {
                kind: AgentConfigurationErrorKind::InvalidRequest,
                message: "内容没有变化，无需保存".to_owned(),
                diagnostics: Vec::new(),
            });
        }
        for dependency in &edit.dependencies {
            dependency.revalidate().map_err(|_| conflict())?;
        }
        if Instant::now() >= deadline {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::Timeout,
            ));
        }
        reload_hints.extend(
            edit.documents
                .iter()
                .map(|document| document.authority.spec.reload_hint.clone()),
        );
        reload_hints.sort();
        reload_hints.dedup();
        let token = opaque_id().map_err(|_| internal())?;
        let public = AgentConfigurationPlan {
            token: token.clone(),
            edit_id: request.edit_id.clone(),
            expires_at: at_after(PLAN_TTL),
            source_ids: files
                .iter()
                .map(|file| file.source.source.source_id.clone())
                .collect(),
            changes,
            reload_hints: reload_hints.clone(),
            diagnostics,
        };
        let plan = PrivatePlan {
            edit_id: request.edit_id,
            kind: edit.public.agent_kind,
            files,
            dependencies: edit.dependencies.clone(),
            reload_hints,
        };
        let mut state = self.state.lock().map_err(|_| internal())?;
        state.prune();
        if !state.edits.contains_key(&plan.edit_id) {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::EditExpired,
            ));
        }
        if edit.latest_plan_generation.load(Ordering::Acquire) != publication {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::PlanExpired,
            ));
        }
        let replaced_bytes = state
            .plans
            .values()
            .filter(|record| record.actor == actor && record.plan.edit_id == plan.edit_id)
            .map(|record| record.plan.retained_bytes())
            .sum::<usize>();
        if state
            .retained_bytes()
            .saturating_sub(replaced_bytes)
            .saturating_add(plan.retained_bytes())
            > MAX_PRIVATE_BYTES
        {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::CapacityExceeded,
            ));
        }
        // A failed or superseded request cannot revoke the usable plan. Replace
        // it only after this latest-started publication passes every check.
        state
            .plans
            .retain(|_, record| record.actor != actor || record.plan.edit_id != plan.edit_id);
        state.plans.insert(
            token,
            PlanRecord {
                actor: actor.to_owned(),
                plan,
                expires: Instant::now() + PLAN_TTL,
            },
        );
        Ok(public)
    }
}
fn require_edit(source: &ConfigurationSourceAuthority) -> Result<(), AgentConfigurationError> {
    let action = if source.file.bytes().is_none() {
        AgentConfigurationActionKind::Create
    } else {
        AgentConfigurationActionKind::Edit
    };
    sources::require_action(&source.source, action)
}
fn initial_text(source: &ConfigurationSourceAuthority) -> Result<String, AgentConfigurationError> {
    if let Some(bytes) = source.file.bytes() {
        return std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| {
            AgentConfigurationError::new(AgentConfigurationErrorKind::UnsupportedFormat)
        });
    }
    Ok(source.spec.initial_text.clone().unwrap_or_else(|| {
        if matches!(
            source.spec.format,
            AgentConfigurationFormat::Json | AgentConfigurationFormat::Jsonc
        ) {
            "{}\n".to_owned()
        } else {
            String::new()
        }
    }))
}
pub(super) fn at_after(duration: Duration) -> String {
    (chrono::Utc::now() + chrono::Duration::from_std(duration).unwrap_or_default()).to_rfc3339()
}
pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
