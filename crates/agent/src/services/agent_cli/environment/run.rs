//! Shared resource state for one Agent environment inventory refresh.

use super::diagnostics::{DiagnosticCollector, DiagnosticEmission, DiagnosticOwner};
#[cfg(test)]
use crate::models::AgentCliKind;
use crate::models::{AgentAssetDiagnostic, AgentAssetLimitKind, AgentAssetLimits};
use crate::services::agent_cli::contracts::{AgentDiagnosticEmission, AgentDiagnosticOutput};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub(in crate::services::agent_cli) trait MonotonicClock:
    Send + Sync
{
    fn now(&self) -> Instant;
}

#[derive(Debug, Default)]
pub(in crate::services::agent_cli) struct SystemMonotonicClock;

impl MonotonicClock for SystemMonotonicClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[cfg(test)]
#[derive(Debug)]
pub(in crate::services::agent_cli) struct ManualClock {
    base: Instant,
    elapsed_nanos: std::sync::atomic::AtomicU64,
}

#[cfg(test)]
impl ManualClock {
    pub(in crate::services::agent_cli) fn new() -> Self {
        Self {
            base: Instant::now(),
            elapsed_nanos: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub(in crate::services::agent_cli) fn advance(&self, amount: Duration) {
        let nanos = amount.as_nanos().min(u64::MAX as u128) as u64;
        self.elapsed_nanos
            .try_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |current| Some(current.saturating_add(nanos)),
            )
            .expect("manual clock update always succeeds");
    }
}

#[cfg(test)]
impl MonotonicClock for ManualClock {
    fn now(&self) -> Instant {
        let elapsed =
            Duration::from_nanos(self.elapsed_nanos.load(std::sync::atomic::Ordering::SeqCst));
        self.base.checked_add(elapsed).unwrap_or(self.base)
    }
}

#[cfg(test)]
pub(super) trait InventoryCheckpointProbe: Send + Sync {
    fn before_checkpoint(&self, stage: AgentInventoryStage);
    fn record_event(&self, event: InventoryPipelineEvent);
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InventoryPipelineEvent {
    ContextDiscovery {
        agent_kind: AgentCliKind,
    },
    InitialSourceDiscovery {
        context_id: String,
    },
    FollowUpSourceDiscovery {
        context_id: String,
        parent_source_key: String,
    },
    Parse {
        context_id: String,
        native_source_key: String,
    },
    Resolve {
        context_id: String,
    },
    Project {
        context_id: String,
    },
}

#[cfg(test)]
#[derive(Debug)]
pub(super) struct StageCheckpointProbe {
    clock: Arc<ManualClock>,
    schedule: std::sync::Mutex<std::collections::BTreeMap<(AgentInventoryStage, u32), Duration>>,
    hits: std::sync::Mutex<std::collections::BTreeMap<AgentInventoryStage, u32>>,
    events: std::sync::Mutex<Vec<InventoryPipelineEvent>>,
}

#[cfg(test)]
impl StageCheckpointProbe {
    pub(super) fn new(clock: Arc<ManualClock>) -> Self {
        Self {
            clock,
            schedule: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            hits: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            events: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub(super) fn advance_on(&self, stage: AgentInventoryStage, hit: u32, amount: Duration) {
        self.schedule
            .lock()
            .expect("checkpoint schedule lock")
            .insert((stage, hit), amount);
    }

    pub(super) fn hit_count(&self, stage: AgentInventoryStage) -> u32 {
        self.hits
            .lock()
            .expect("checkpoint hit lock")
            .get(&stage)
            .copied()
            .unwrap_or_default()
    }

    pub(super) fn events(&self) -> Vec<InventoryPipelineEvent> {
        self.events.lock().expect("pipeline event lock").clone()
    }
}

#[cfg(test)]
impl InventoryCheckpointProbe for StageCheckpointProbe {
    fn before_checkpoint(&self, stage: AgentInventoryStage) {
        let hit = {
            let mut hits = self.hits.lock().expect("checkpoint hit lock");
            let hit = hits.entry(stage).or_default();
            *hit = hit.saturating_add(1);
            *hit
        };
        if let Some(amount) = self
            .schedule
            .lock()
            .expect("checkpoint schedule lock")
            .get(&(stage, hit))
            .copied()
        {
            self.clock.advance(amount);
        }
    }

    fn record_event(&self, event: InventoryPipelineEvent) {
        self.events.lock().expect("pipeline event lock").push(event);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum AgentInventoryStage {
    InstallationDiscovery,
    ContextDiscovery,
    InitialSourceDiscovery,
    FollowUpSourceDiscovery,
    Snapshot,
    Parse,
    Resolve,
    Project,
}

/// Common owner of the absolute refresh deadline, byte charge and diagnostic
/// cap. Adapters never receive this value; they only receive bounded output
/// sinks from the common inventory layer.
pub(in crate::services::agent_cli) struct AgentInventoryRun {
    clock: Arc<dyn MonotonicClock>,
    limits: AgentAssetLimits,
    started_at: Instant,
    deadline: Instant,
    canceled: Option<Arc<std::sync::atomic::AtomicBool>>,
    bytes_read: usize,
    reads_closed: bool,
    bytes_reported: bool,
    deadline_closed: bool,
    deadline_reported: bool,
    diagnostics: DiagnosticCollector,
    #[cfg(test)]
    checkpoint_probe: Option<Arc<dyn InventoryCheckpointProbe>>,
}

impl AgentInventoryRun {
    #[cfg(test)]
    pub(super) fn new(limits: AgentAssetLimits) -> Self {
        Self::with_clock(limits, Arc::new(SystemMonotonicClock))
    }

    pub(in crate::services::agent_cli) fn with_clock(
        limits: AgentAssetLimits,
        clock: Arc<dyn MonotonicClock>,
    ) -> Self {
        Self::with_clock_and_probe(limits, clock, None)
    }

    #[cfg(test)]
    pub(super) fn with_clock_and_probe(
        limits: AgentAssetLimits,
        clock: Arc<dyn MonotonicClock>,
        checkpoint_probe: Option<Arc<dyn InventoryCheckpointProbe>>,
    ) -> Self {
        let limits = limits.bounded();
        let started_at = clock.now();
        let deadline = started_at
            .checked_add(Duration::from_millis(limits.refresh_budget_ms))
            .unwrap_or(started_at);
        Self {
            clock,
            diagnostics: DiagnosticCollector::new(limits.diagnostics),
            limits,
            started_at,
            deadline,
            canceled: None,
            bytes_read: 0,
            reads_closed: false,
            bytes_reported: false,
            deadline_closed: false,
            deadline_reported: false,
            checkpoint_probe,
        }
    }

    #[cfg(not(test))]
    fn with_clock_and_probe(
        limits: AgentAssetLimits,
        clock: Arc<dyn MonotonicClock>,
        _checkpoint_probe: Option<()>,
    ) -> Self {
        let limits = limits.bounded();
        let started_at = clock.now();
        let deadline = started_at
            .checked_add(Duration::from_millis(limits.refresh_budget_ms))
            .unwrap_or(started_at);
        Self {
            clock,
            diagnostics: DiagnosticCollector::new(limits.diagnostics),
            limits,
            started_at,
            deadline,
            canceled: None,
            bytes_read: 0,
            reads_closed: false,
            bytes_reported: false,
            deadline_closed: false,
            deadline_reported: false,
        }
    }

    pub(super) fn set_cancellation(
        &mut self,
        canceled: Option<Arc<std::sync::atomic::AtomicBool>>,
    ) {
        self.canceled = canceled;
    }

    pub(in crate::services::agent_cli) fn limits(&self) -> &AgentAssetLimits {
        &self.limits
    }

    #[cfg(test)]
    pub(super) fn record_event(&self, event: InventoryPipelineEvent) {
        if let Some(probe) = &self.checkpoint_probe {
            probe.record_event(event);
        }
    }

    pub(in crate::services::agent_cli) fn deadline(&self) -> Instant {
        self.deadline
    }

    #[cfg(test)]
    pub(super) fn bytes_read(&self) -> usize {
        self.bytes_read
    }

    #[cfg(test)]
    pub(super) fn reads_closed(&self) -> bool {
        self.reads_closed
    }

    pub(super) fn diagnostics(&self) -> &DiagnosticCollector {
        &self.diagnostics
    }

    /// Check whether a new stage may begin. The first expired check emits one
    /// typed budget diagnostic; later checks are side-effect free.
    pub(super) fn checkpoint(&mut self, stage: AgentInventoryStage) -> bool {
        #[cfg(not(test))]
        let _ = stage;
        #[cfg(test)]
        if let Some(probe) = &self.checkpoint_probe {
            probe.before_checkpoint(stage);
        }
        self.deadline_diagnostic(DiagnosticOwner::Inventory)
            .is_none()
    }

    pub(super) fn deadline_diagnostic(
        &mut self,
        owner: DiagnosticOwner,
    ) -> Option<AgentAssetDiagnostic> {
        let now = self.clock.now();
        let canceled = self
            .canceled
            .as_ref()
            .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed));
        if !canceled && !self.deadline_closed && now < self.deadline {
            return None;
        }
        self.deadline_closed = true;
        let diagnostic = AgentAssetDiagnostic::BudgetExceeded {
            elapsed_ms: now
                .saturating_duration_since(self.started_at)
                .as_millis()
                .min(u64::MAX as u128) as u64,
            budget_ms: self.limits.refresh_budget_ms,
        };
        if !self.deadline_reported {
            self.deadline_reported = true;
            self.diagnostics.emit(owner, diagnostic.clone());
        }
        Some(diagnostic)
    }

    /// Reserve a physical read before opening it. A failed reservation closes
    /// subsequent reads for this refresh, while already completed units may
    /// still be parsed/projected if the deadline remains open.
    pub(super) fn can_start_read(
        &mut self,
        expected_bytes: usize,
    ) -> Result<(), AgentAssetDiagnostic> {
        if let Some(diagnostic) = self.deadline_diagnostic(DiagnosticOwner::Inventory) {
            return Err(diagnostic);
        }
        if self.reads_closed {
            return Err(AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::BytesPerRefresh,
                accepted: self.bytes_read as u64,
                observed_at_least: self.bytes_read.saturating_add(expected_bytes) as u64,
            });
        }
        if expected_bytes
            > self
                .limits
                .bytes_per_refresh
                .saturating_sub(self.bytes_read)
        {
            self.reads_closed = true;
            return Err(AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::BytesPerRefresh,
                accepted: self.bytes_read as u64,
                observed_at_least: self.bytes_read.saturating_add(expected_bytes) as u64,
            });
        }
        Ok(())
    }

    pub(super) fn commit_read(&mut self, actual_bytes: usize) {
        self.bytes_read = self.bytes_read.saturating_add(actual_bytes);
        if self.bytes_read >= self.limits.bytes_per_refresh {
            self.reads_closed = true;
        }
    }

    pub(super) fn remaining_bytes(&self) -> usize {
        self.limits
            .bytes_per_refresh
            .saturating_sub(self.bytes_read)
    }

    pub(super) fn reads_open(&self) -> bool {
        !self.reads_closed
    }

    /// Materialize one diagnostic when a source was accepted but cannot be
    /// opened after the shared refresh byte budget closed future reads.
    pub(super) fn report_closed_read(
        &mut self,
        owner: DiagnosticOwner,
    ) -> Option<AgentAssetDiagnostic> {
        if !self.reads_closed || self.bytes_reported {
            return None;
        }
        self.bytes_reported = true;
        let diagnostic = AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::BytesPerRefresh,
            accepted: self.bytes_read as u64,
            observed_at_least: self.bytes_read.saturating_add(1) as u64,
        };
        self.diagnostics.emit(owner, diagnostic.clone());
        Some(diagnostic)
    }

    pub(super) fn emit(
        &mut self,
        owner: DiagnosticOwner,
        diagnostic: AgentAssetDiagnostic,
    ) -> AgentDiagnosticEmission {
        match self.diagnostics.emit(owner, diagnostic) {
            DiagnosticEmission::Accepted => AgentDiagnosticEmission::Accepted,
            DiagnosticEmission::Duplicate => AgentDiagnosticEmission::Duplicate,
            DiagnosticEmission::Saturated => AgentDiagnosticEmission::Saturated,
        }
    }

    pub(super) fn take_diagnostics_for(
        &mut self,
        owner: DiagnosticOwner,
    ) -> Vec<AgentAssetDiagnostic> {
        self.diagnostics.take_owner(&owner)
    }

    pub(super) fn rebind_diagnostics(
        &mut self,
        old_owner: DiagnosticOwner,
        new_owner: DiagnosticOwner,
        source_id: Option<&str>,
    ) -> usize {
        self.diagnostics
            .rebind_owner(&old_owner, new_owner, source_id)
    }

    pub(in crate::services::agent_cli) fn emit_inventory_diagnostic(
        &mut self,
        diagnostic: AgentAssetDiagnostic,
    ) -> AgentDiagnosticEmission {
        self.emit(DiagnosticOwner::Inventory, diagnostic)
    }

    pub(in crate::services::agent_cli) fn emit_installation_diagnostic(
        &mut self,
        installation_id: &str,
        diagnostic: AgentAssetDiagnostic,
    ) -> AgentDiagnosticEmission {
        self.emit(
            DiagnosticOwner::Installation(installation_id.to_owned()),
            diagnostic,
        )
    }

    #[cfg(test)]
    pub(in crate::services::agent_cli) fn take_installation_diagnostics(
        &mut self,
        installation_id: &str,
    ) -> Vec<AgentAssetDiagnostic> {
        self.take_diagnostics_for(DiagnosticOwner::Installation(installation_id.to_owned()))
    }

    pub(in crate::services::agent_cli) fn finish_diagnostics(
        &mut self,
    ) -> Vec<AgentAssetDiagnostic> {
        self.diagnostics.finish_inventory()
    }
}

impl AgentDiagnosticOutput for AgentInventoryRun {
    fn has_regular_capacity(&self) -> bool {
        self.diagnostics.has_regular_capacity()
    }

    fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.emit(DiagnosticOwner::Inventory, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_deadline_is_reported_once() {
        let clock = Arc::new(ManualClock::new());
        let mut run = AgentInventoryRun::with_clock(AgentAssetLimits::defaults(), clock.clone());
        clock.advance(Duration::from_millis(
            AgentAssetLimits::defaults().refresh_budget_ms,
        ));
        assert!(!run.checkpoint(AgentInventoryStage::Snapshot));
        assert!(!run.checkpoint(AgentInventoryStage::Parse));
        let values = run.finish_diagnostics();
        assert_eq!(
            values
                .iter()
                .filter(|value| matches!(value, AgentAssetDiagnostic::BudgetExceeded { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn checkpoint_before_deadline_tick_is_allowed() {
        let mut limits = AgentAssetLimits::defaults();
        limits.refresh_budget_ms = 1;
        let clock = Arc::new(ManualClock::new());
        let mut run = AgentInventoryRun::with_clock(limits.clone(), clock.clone());
        clock.advance(Duration::from_millis(limits.refresh_budget_ms) - Duration::from_nanos(1));

        assert!(run.checkpoint(AgentInventoryStage::Snapshot));
        assert!(run.finish_diagnostics().is_empty());
    }

    #[test]
    fn deadline_equality_expires_and_reports_exact_budget() {
        let mut limits = AgentAssetLimits::defaults();
        limits.refresh_budget_ms = 7;
        let clock = Arc::new(ManualClock::new());
        let mut run = AgentInventoryRun::with_clock(limits.clone(), clock.clone());
        clock.advance(Duration::from_millis(limits.refresh_budget_ms));

        assert!(!run.checkpoint(AgentInventoryStage::Parse));
        assert!(!run.checkpoint(AgentInventoryStage::Resolve));
        assert_eq!(
            run.finish_diagnostics(),
            vec![AgentAssetDiagnostic::BudgetExceeded {
                elapsed_ms: limits.refresh_budget_ms,
                budget_ms: limits.refresh_budget_ms,
            }]
        );
    }

    #[test]
    fn stage_probe_expires_at_scheduled_hit_without_poll_count_coupling() {
        let mut limits = AgentAssetLimits::defaults();
        limits.refresh_budget_ms = 5;
        let clock = Arc::new(ManualClock::new());
        let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
        probe.advance_on(
            AgentInventoryStage::Parse,
            2,
            Duration::from_millis(limits.refresh_budget_ms),
        );
        let mut run = AgentInventoryRun::with_clock_and_probe(limits.clone(), clock, Some(probe));

        assert!(run.checkpoint(AgentInventoryStage::Parse));
        assert!(!run.checkpoint(AgentInventoryStage::Parse));
        assert!(!run.checkpoint(AgentInventoryStage::Resolve));
        assert_eq!(
            run.finish_diagnostics()
                .iter()
                .filter(|value| matches!(value, AgentAssetDiagnostic::BudgetExceeded { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn diagnostic_rebind_delegation_preserves_capacity_and_source_identity() {
        let mut run =
            AgentInventoryRun::with_clock(AgentAssetLimits::DEFAULT, Arc::new(ManualClock::new()));
        let old = DiagnosticOwner::Source("provisional".to_owned());
        run.emit(
            old.clone(),
            AgentAssetDiagnostic::ReadFailed {
                source_id: "provisional".to_owned(),
                error_kind: crate::models::AgentAssetIoErrorKind::Other,
            },
        );
        assert_eq!(
            run.rebind_diagnostics(
                old,
                DiagnosticOwner::Source("final".to_owned()),
                Some("final"),
            ),
            1
        );
        assert_eq!(run.diagnostics().retained_count(), 1);
        assert_eq!(run.diagnostics().seen_len(), 1);
        assert!(matches!(
            run.finish_diagnostics().as_slice(),
            [AgentAssetDiagnostic::ReadFailed { source_id, .. }] if source_id == "final"
        ));
    }

    #[test]
    fn refresh_bytes_close_future_reads_but_do_not_change_completed_charge() {
        let mut limits = AgentAssetLimits::defaults();
        limits.bytes_per_refresh = 2;
        let mut run = AgentInventoryRun::with_clock(limits, Arc::new(ManualClock::new()));
        assert!(run.can_start_read(2).is_ok());
        run.commit_read(2);
        assert!(run.reads_closed());
        assert!(run.can_start_read(1).is_err());
        assert_eq!(run.bytes_read(), 2);
    }
}
