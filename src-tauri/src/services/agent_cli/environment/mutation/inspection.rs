//! A selected preview and its commit rechecks use the same Agent boundary.
use super::{MutationInspector, MutationInventory, MutationPreparation, PreparedMutation};
use crate::{models::*, services::agent_cli::environment};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
};

pub(crate) fn scoped_inspector(
    base: Arc<dyn MutationInspector>,
    kinds: BTreeSet<AgentCliKind>,
) -> Arc<dyn MutationInspector> {
    Arc::new(ScopedInspector { base, kinds })
}

struct ScopedInspector {
    base: Arc<dyn MutationInspector>,
    kinds: BTreeSet<AgentCliKind>,
}

impl ScopedInspector {
    fn scan(
        &self,
        canceled: Option<Arc<AtomicBool>>,
    ) -> Result<MutationInventory, AgentAssetMutationError> {
        let build = environment::inventory_for_agents_cancellable(
            self.settings(),
            self.workspace(),
            &self.kinds,
            canceled,
        )
        .map_err(|_| {
            AgentAssetMutationError::new(AgentAssetMutationErrorKind::PreparationFailed)
        })?;
        Ok(MutationInventory {
            inventory: build.inventory,
            source_anchors: build
                .access_evidence
                .into_iter()
                .map(|evidence| (evidence.source_id, evidence.anchor))
                .collect(),
        })
    }
}

impl MutationInspector for ScopedInspector {
    fn inspect(&self) -> Result<MutationInventory, AgentAssetMutationError> {
        self.scan(None)
    }
    fn inspect_for_read(
        &self,
        canceled: Arc<AtomicBool>,
    ) -> Result<MutationInventory, AgentAssetMutationError> {
        self.scan(Some(canceled))
    }
    fn home(&self) -> &Path {
        self.base.home()
    }
    fn workspace(&self) -> Option<&Path> {
        self.base.workspace()
    }
    fn settings(&self) -> &AppSettings {
        self.base.settings()
    }
    fn prepare(
        &self,
        request: MutationPreparation<'_>,
    ) -> Result<PreparedMutation, AgentAssetMutationError> {
        self.base.prepare(request)
    }
}
