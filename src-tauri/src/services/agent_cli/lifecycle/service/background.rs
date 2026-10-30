use super::super::automation::{AutomaticUpdates, CATALOG_EVENT, OPERATION_EVENT};
use super::*;
use std::sync::{atomic::AtomicU64, OnceLock};
use tauri::{Emitter, Manager};

pub(super) struct BackgroundState {
    pub(super) automatic: Arc<AutomaticUpdates>,
    pub(super) wake: tokio::sync::Notify,
    catalog: Mutex<Option<AgentLifecycleCatalog>>,
    catalog_gate: tokio::sync::Mutex<()>,
    revision: AtomicU64,
    pub(super) installation_dirty: AtomicBool,
    app: OnceLock<tauri::AppHandle>,
}

impl BackgroundState {
    pub(super) fn new(journal: &Journal) -> Self {
        Self {
            automatic: Arc::new(AutomaticUpdates::new(journal.automatic_path())),
            wake: tokio::sync::Notify::new(),
            catalog: Mutex::default(),
            catalog_gate: tokio::sync::Mutex::new(()),
            revision: AtomicU64::new(0),
            app: OnceLock::new(),
            installation_dirty: AtomicBool::new(false),
        }
    }
}

impl LifecycleService {
    pub(super) async fn catalog_live(
        &self,
        settings: AppSettings,
        home: PathBuf,
        request: AgentLifecycleCatalogRequest,
    ) -> Result<AgentLifecycleCatalog, AgentLifecycleError> {
        let _guard = self.background.catalog_gate.lock().await;
        if let Err(error) = self.operations(AUTOMATIC_ACTOR) {
            self.automatic().block(format!(
                "升级记录不可用：{}；修复后重启应用可恢复自动更新",
                error.message
            ));
        }
        let kind = request.agent_kind;
        let catalog = self
            .planner
            .catalog(
                self.context(settings, home),
                AgentLifecycleCatalogRequest {
                    agent_kind: None,
                    ..request
                },
            )
            .await?;
        let mut catalog = self.publish_catalog(catalog)?;
        if let Some(kind) = kind {
            catalog.targets.retain(|target| target.agent_kind == kind);
            catalog
                .automatic_updates
                .retain(|entry| entry.agent_kind == kind);
        }
        Ok(catalog)
    }

    fn publish_catalog(
        &self,
        mut catalog: AgentLifecycleCatalog,
    ) -> Result<AgentLifecycleCatalog, AgentLifecycleError> {
        // Assign revisions while holding the publication lock, including policy-only changes.
        let mut latest = self.background.catalog.lock().map_err(|_| internal())?;
        catalog.automatic_updates = self.background.automatic.snapshot(&catalog.targets);
        catalog.revision = self
            .background
            .revision
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        *latest = Some(catalog.clone());
        drop(latest);
        if let Some(app) = self.background.app.get() {
            let _ = app.emit(CATALOG_EVENT, &catalog);
        }
        Ok(catalog)
    }

    pub(in super::super) fn cached_catalog(&self) -> Option<AgentLifecycleCatalog> {
        self.background
            .catalog
            .lock()
            .ok()
            .and_then(|catalog| catalog.clone())
    }

    pub(in super::super) fn publish_automatic(&self) {
        let mut latest = self
            .background
            .catalog
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let Some(catalog) = latest.as_mut() else {
            return;
        };
        let statuses = self.background.automatic.snapshot(&catalog.targets);
        if statuses == catalog.automatic_updates {
            return;
        }
        catalog.automatic_updates = statuses;
        catalog.revision = self
            .background
            .revision
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        let snapshot = catalog.clone();
        drop(latest);
        if let Some(app) = self.background.app.get() {
            let _ = app.emit(CATALOG_EVENT, &snapshot);
        }
    }

    pub(in super::super) fn automatic(&self) -> Arc<AutomaticUpdates> {
        Arc::clone(&self.background.automatic)
    }

    pub(in super::super) fn invalidate_installations(&self) {
        self.background
            .installation_dirty
            .store(true, Ordering::Release);
    }
    pub(in super::super) fn take_installation_change(&self) -> bool {
        self.background
            .installation_dirty
            .swap(false, Ordering::AcqRel)
    }

    pub(in super::super) fn attach_worker(&self, app: tauri::AppHandle) -> bool {
        self.background.app.set(app).is_ok()
    }

    pub(in super::super) async fn wait_for_work(&self, duration: Duration) {
        let _ = tokio::time::timeout(duration, self.background.wake.notified()).await;
    }

    pub(in super::super) fn has_active_update(&self, key: &str) -> bool {
        self.operations.lock().map_or(true, |operations| {
            operations.values().any(|cell| {
                let operation = cell.snapshot();
                operation.outcome.is_none() && operation.update_key.as_deref() == Some(key)
            })
        })
    }

    pub(super) fn emit_operation(&self, operation: &AgentLifecycleOperation) {
        if let Some(app) = self.background.app.get() {
            let _ = app.emit(OPERATION_EVENT, operation);
        }
    }

    pub(super) fn annotate_completed(
        &self,
        actor: &str,
        mut operation: AgentLifecycleOperation,
    ) -> AgentLifecycleOperation {
        if let Some(app) = self.background.app.get() {
            let settings = app
                .state::<crate::state::AppState>()
                .data
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .settings
                .clone();
            super::super::launch::annotate(&mut operation, &settings);
            if let Ok(published) = self.publish_launch(actor, operation.clone()) {
                return published;
            }
        }
        operation
    }

    pub(crate) async fn set_automatic(
        self: &Arc<Self>,
        settings: AppSettings,
        home: PathBuf,
        request: SetAgentAutomaticUpdateRequest,
    ) -> Result<AgentLifecycleCatalog, AgentLifecycleError> {
        if request.key.is_empty() || request.key.len() > 256 {
            return Err(AgentLifecycleError::new(
                AgentLifecycleErrorKind::InvalidRequest,
            ));
        }
        let target = if request.enabled {
            self.operations(AUTOMATIC_ACTOR)?;
            let catalog = self
                .catalog(
                    settings.clone(),
                    home.clone(),
                    AgentLifecycleCatalogRequest::default(),
                )
                .await?;
            let target = catalog
                .targets
                .into_iter()
                .find(|target| {
                    Some(&target.id) == request.target_id.as_ref()
                        && target.update_key.as_deref() == Some(&request.key)
                        && Some(&target.evidence_revision)
                            == request.expected_evidence_revision.as_ref()
                })
                .ok_or_else(|| AgentLifecycleError::new(AgentLifecycleErrorKind::TargetChanged))?;
            if !catalog
                .automatic_updates
                .iter()
                .any(|entry| entry.target_id.as_deref() == Some(&target.id) && entry.can_enable)
            {
                return Err(AgentLifecycleError::new(
                    AgentLifecycleErrorKind::ActionUnavailable,
                ));
            }
            if self.has_active_update(&request.key) {
                return Err(AgentLifecycleError::new(
                    AgentLifecycleErrorKind::OperationInProgress,
                ));
            }
            Some(target)
        } else {
            None
        };
        let automatic = self.automatic();
        let key = request.key.clone();
        let enabled = request.enabled;
        tauri::async_runtime::spawn_blocking(move || automatic.set(&key, enabled, target.as_ref()))
            .await
            .map_err(|_| internal())?
            .map_err(automatic_error)?;
        if !enabled {
            for operation in self.operations(AUTOMATIC_ACTOR)? {
                if operation.origin == AgentLifecycleOrigin::Automatic
                    && operation.update_key.as_deref() == Some(&request.key)
                    && operation.can_cancel
                {
                    self.cancel(AUTOMATIC_ACTOR, &operation.id)?;
                }
            }
        }
        self.publish_automatic();
        self.background.wake.notify_one();
        match self.cached_catalog() {
            Some(catalog) => Ok(catalog),
            None => {
                self.catalog(settings, home, AgentLifecycleCatalogRequest::default())
                    .await
            }
        }
    }
}

fn automatic_error(message: String) -> AgentLifecycleError {
    let mut error = internal();
    error.message = message;
    error
}
