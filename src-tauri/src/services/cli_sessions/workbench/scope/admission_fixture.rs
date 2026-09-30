use super::{hash, resolve_session_ref_with, state, ScopeState};
use crate::{
    models::{
        AgentCliKind, AgentSessionActivityState, AgentSessionParent, AgentSessionRole,
        AgentSessionRow, AgentSessionScope, AgentSessionSource, AgentSessionWorkspace,
        CliSessionSummary,
    },
    services::agent_cli::contracts::{
        EnvironmentPatch, SessionHistoryRecord, SessionHistorySource,
    },
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    time::Instant,
};

/// Synthetic scope data replaces only App reads. Grants and references use the
/// production workbench registry, and resolution uses the production checks.
pub(crate) struct ResumeAdmissionFixture {
    root: PathBuf,
    actor_prefix: String,
    scope: ScopeState,
    reference: state::ReferenceState,
    current_scopes: Mutex<HashMap<String, ScopeState>>,
}

impl ResumeAdmissionFixture {
    pub(crate) fn new(name: &str) -> Self {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        let actor_prefix = format!(
            "balancehub-resume-admission-{name}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(&actor_prefix);
        let workdir = root.join("workspace");
        let config_root = root.join("native-source");
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::create_dir_all(&config_root).unwrap();
        let locator = config_root.join("synthetic-session.jsonl");
        std::fs::write(&locator, "synthetic resume grant fixture\n").unwrap();
        let source_id = hash(&["native", "codex", &config_root.to_string_lossy()]);
        let workspace_id = hash(&["workspace", &workdir.to_string_lossy()]);
        let summary = CliSessionSummary {
            id: "synthetic-native-session".into(),
            title: "synthetic resume fixture".into(),
            preview: None,
            model: None,
            models: Vec::new(),
            cli_kind: AgentCliKind::Codex,
            created_at: None,
            updated_at: None,
            workdir: workdir.to_string_lossy().into_owned(),
            cli_version: None,
            archived: false,
            can_resume: true,
            metadata_source: "synthetic-fixture".into(),
        };
        let row = AgentSessionRow {
            session_ref: hash(&["resume-fixture", &actor_prefix]),
            source_id: source_id.clone(),
            workspace_id: workspace_id.clone(),
            session: summary.clone(),
            role: AgentSessionRole::Main,
            parent: AgentSessionParent::None,
            resume_reason: None,
            activity_state: AgentSessionActivityState::Unknown,
            runtime_ids: Vec::new(),
        };
        let scope = ScopeState {
            public: AgentSessionScope {
                revision: String::new(),
                workspaces: vec![AgentSessionWorkspace {
                    id: workspace_id,
                    path: summary.workdir.clone(),
                    exists: true,
                    is_home: false,
                }],
                sources: vec![AgentSessionSource {
                    id: source_id.clone(),
                    agent_kind: AgentCliKind::Codex,
                    config_root: config_root.to_string_lossy().into_owned(),
                    available: true,
                }],
            },
            explicit_workdir: Some(summary.workdir.clone()),
            sources: HashMap::from([(
                source_id,
                SessionHistorySource {
                    config_root,
                    launch_environment: EnvironmentPatch::default(),
                    resume_reason: None,
                },
            )]),
            created_at: Instant::now(),
        };
        Self {
            root,
            actor_prefix,
            scope,
            reference: state::ReferenceState {
                row,
                record: SessionHistoryRecord {
                    summary,
                    locator,
                    record_key: "synthetic-record".into(),
                    role: AgentSessionRole::Main,
                    parent_native_id: None,
                    resume_reason: None,
                    content_unavailable_reason: None,
                },
            },
            current_scopes: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn actor_key(&self, window: &str) -> String {
        format!("{}:{window}", self.actor_prefix)
    }

    pub(crate) fn session_ref(&self) -> &str {
        &self.reference.row.session_ref
    }

    pub(crate) fn grant_scope(&self, window: &str, revision: &str) {
        let mut scope = self.scope.clone();
        scope.public.revision = revision.into();
        self.current_scopes
            .lock()
            .unwrap()
            .insert(window.into(), scope.clone());
        state::registry()
            .scopes
            .insert((self.actor_key(window), revision.into()), scope);
    }

    pub(crate) fn grant(&self, window: &str, revision: &str) {
        self.grant_scope(window, revision);
        state::registry().references.insert(
            (
                self.actor_key(window),
                revision.into(),
                self.session_ref().into(),
            ),
            self.reference.clone(),
        );
    }

    pub(crate) fn advance_scope(&self, window: &str, revision: &str) {
        self.current_scopes
            .lock()
            .unwrap()
            .get_mut(window)
            .unwrap()
            .public
            .revision = revision.into();
    }

    pub(crate) fn resolve(
        &self,
        window: &str,
        revision: &str,
        session_ref: &str,
    ) -> Result<(), String> {
        resolve_session_ref_with(
            &self.actor_key(window),
            revision,
            session_ref,
            |explicit_workdir| {
                let scope = self.current_scopes.lock().unwrap()[window].clone();
                assert_eq!(explicit_workdir, scope.explicit_workdir.as_deref());
                Ok(scope)
            },
        )
        .map(|_| ())
    }
}

impl Drop for ResumeAdmissionFixture {
    fn drop(&mut self) {
        let prefix = format!("{}:", self.actor_prefix);
        let mut registry = state::registry();
        registry
            .scopes
            .retain(|(actor, _), _| !actor.starts_with(&prefix));
        registry
            .references
            .retain(|(actor, _, _), _| !actor.starts_with(&prefix));
        drop(registry);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
