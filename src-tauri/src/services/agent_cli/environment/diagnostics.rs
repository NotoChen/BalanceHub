//! A single, incrementally bounded diagnostic sink for one inventory refresh.
//!
//! Diagnostics used to be accumulated in every adapter result and truncated at
//! the end of a refresh.  That protected only the serialized response, not the
//! work and allocations needed to create the intermediate vectors.  This
//! collector reserves one slot for a terminal truncation marker and accepts
//! diagnostics as they are produced.

use crate::models::{AgentAssetDiagnostic, AgentAssetLimitKind};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum DiagnosticOwner {
    Inventory,
    Context(String),
    Source(String),
    Installation(String),
    Declaration(String),
    Record(String),
    Resolution(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiagnosticEmission {
    Accepted,
    Duplicate,
    Saturated,
}

#[derive(Debug, Default)]
pub(super) struct DiagnosticCollector {
    limit: usize,
    entries: Vec<(DiagnosticOwner, AgentAssetDiagnostic)>,
    seen: BTreeSet<(DiagnosticOwner, AgentAssetDiagnostic)>,
    materialized: BTreeSet<(DiagnosticOwner, AgentAssetDiagnostic)>,
    retained_count: usize,
    overflow_observed_at_least: Option<u64>,
}

impl DiagnosticCollector {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            limit,
            entries: Vec::new(),
            seen: BTreeSet::new(),
            materialized: BTreeSet::new(),
            retained_count: 0,
            overflow_observed_at_least: None,
        }
    }

    /// Whether a normal diagnostic can still be retained. This intentionally
    /// does not expose the remaining count to an adapter.
    pub(super) fn has_regular_capacity(&self) -> bool {
        self.limit > 0
            && self.retained_count < self.limit.saturating_sub(1)
            && self.overflow_observed_at_least.is_none()
    }

    pub(super) fn emit(
        &mut self,
        owner: DiagnosticOwner,
        diagnostic: AgentAssetDiagnostic,
    ) -> DiagnosticEmission {
        // Retained keys are bounded by `limit - 1`, so duplicate detection can
        // borrow the existing key without allocating another copy. This must
        // happen before the capacity check: an already retained diagnostic is
        // not an overflow event.
        if self.seen.iter().any(|(seen_owner, seen_diagnostic)| {
            seen_owner == &owner && seen_diagnostic == &diagnostic
        }) {
            return DiagnosticEmission::Duplicate;
        }

        // Once saturated, do not retain or clone another owner/diagnostic key.
        // The terminal marker only needs a monotonic lower bound for the
        // number of diagnostics observed after the retained prefix.
        if !self.has_regular_capacity() {
            if self.limit > 0 {
                let observed = self
                    .overflow_observed_at_least
                    .unwrap_or(self.retained_count.try_into().unwrap_or(u64::MAX))
                    .saturating_add(1);
                self.overflow_observed_at_least = Some(
                    self.overflow_observed_at_least
                        .map_or(observed, |previous| previous.max(observed)),
                );
            }
            return DiagnosticEmission::Saturated;
        }

        let key = (owner.clone(), diagnostic.clone());
        self.seen.insert(key);

        self.entries.push((owner, diagnostic));
        self.retained_count = self.retained_count.saturating_add(1);
        DiagnosticEmission::Accepted
    }

    pub(super) fn take_owner(&mut self, owner: &DiagnosticOwner) -> Vec<AgentAssetDiagnostic> {
        let mut owned = Vec::new();
        let mut remaining = Vec::with_capacity(self.entries.len());
        for (entry_owner, diagnostic) in self.entries.drain(..) {
            if &entry_owner == owner {
                self.materialized
                    .insert((entry_owner.clone(), diagnostic.clone()));
                owned.push(diagnostic);
            } else {
                remaining.push((entry_owner, diagnostic));
            }
        }
        self.entries = remaining;
        owned
    }

    /// Move retained diagnostics between public owners without reopening the
    /// diagnostic budget. Source-owned diagnostics also carry the source ID in
    /// their payload, so both identities are changed as one collector
    /// operation. The old dedup keys are removed before the new keys are
    /// installed, preventing a rebinding from leaving a stale provisional ID.
    /// Historical/materialized keys stay in the dedup set; only keys belonging
    /// to the current entries being transformed are removed.
    pub(super) fn rebind_owner(
        &mut self,
        old_owner: &DiagnosticOwner,
        new_owner: DiagnosticOwner,
        source_id: Option<&str>,
    ) -> usize {
        let mut rebound = 0;
        let current_old_keys = self
            .entries
            .iter()
            .filter(|(owner, _)| owner == old_owner)
            .map(|(owner, diagnostic)| (owner.clone(), diagnostic.clone()))
            .collect::<BTreeSet<_>>();
        for old_key in current_old_keys {
            if !self.materialized.contains(&old_key) {
                self.seen.remove(&old_key);
            }
        }

        let mut entries = Vec::with_capacity(self.entries.len());
        for (owner, diagnostic) in self.entries.drain(..) {
            if &owner != old_owner {
                entries.push((owner, diagnostic));
                continue;
            }

            let diagnostic = {
                rebound += 1;
                match source_id {
                    Some(source_id) => rebind_source_id(diagnostic, source_id),
                    None => diagnostic,
                }
            };
            let owner = new_owner.clone();
            let key = (owner.clone(), diagnostic.clone());
            if self.seen.insert(key) {
                entries.push((owner, diagnostic));
            } else {
                self.retained_count = self.retained_count.saturating_sub(1);
            }
        }
        self.entries = entries;
        rebound
    }

    /// Materialize diagnostics not assigned to a narrower public owner. Any
    /// owner not explicitly materialized is safely surfaced at inventory scope.
    pub(super) fn finish_inventory(&mut self) -> Vec<AgentAssetDiagnostic> {
        let mut output = self.take_owner(&DiagnosticOwner::Inventory);
        output.extend(self.entries.drain(..).map(|(owner, diagnostic)| {
            self.materialized.insert((owner, diagnostic.clone()));
            diagnostic
        }));
        if let Some(observed_at_least) = self.overflow_observed_at_least.take() {
            output.push(AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::Diagnostics,
                accepted: self.limit.saturating_sub(1) as u64,
                observed_at_least,
            });
        }
        output
    }

    #[cfg(test)]
    pub(super) fn retained_len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(super) fn retained_count(&self) -> usize {
        self.retained_count
    }

    #[cfg(test)]
    pub(super) fn seen_len(&self) -> usize {
        self.seen.len()
    }
}

pub(super) fn rebind_source_id(
    diagnostic: AgentAssetDiagnostic,
    source_id: &str,
) -> AgentAssetDiagnostic {
    match diagnostic {
        AgentAssetDiagnostic::SymlinkRejected { .. } => AgentAssetDiagnostic::SymlinkRejected {
            source_id: source_id.to_owned(),
        },
        AgentAssetDiagnostic::ReadFailed { error_kind, .. } => AgentAssetDiagnostic::ReadFailed {
            source_id: source_id.to_owned(),
            error_kind,
        },
        AgentAssetDiagnostic::SourceOutsideAllowedRoot { .. } => {
            AgentAssetDiagnostic::SourceOutsideAllowedRoot {
                source_id: source_id.to_owned(),
            }
        }
        AgentAssetDiagnostic::SourceTypeMismatch {
            expected, actual, ..
        } => AgentAssetDiagnostic::SourceTypeMismatch {
            source_id: source_id.to_owned(),
            expected,
            actual,
        },
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgentAssetCategory, AgentAssetDocumentFormat};

    fn malformed(path: &str) -> AgentAssetDiagnostic {
        AgentAssetDiagnostic::Malformed {
            format: AgentAssetDocumentFormat::Json,
            location: Some(path.to_string()),
        }
    }

    #[test]
    fn exact_capacity_has_no_terminal_marker() {
        let mut collector = DiagnosticCollector::new(3);
        assert_eq!(
            collector.emit(DiagnosticOwner::Inventory, malformed("a")),
            DiagnosticEmission::Accepted
        );
        assert_eq!(
            collector.emit(DiagnosticOwner::Inventory, malformed("b")),
            DiagnosticEmission::Accepted
        );
        assert_eq!(collector.retained_len(), 2);
        assert_eq!(collector.retained_count(), 2);
        assert_eq!(collector.finish_inventory().len(), 2);
    }

    #[test]
    fn one_over_reserves_a_single_terminal_marker() {
        let mut collector = DiagnosticCollector::new(3);
        collector.emit(DiagnosticOwner::Inventory, malformed("a"));
        collector.emit(DiagnosticOwner::Inventory, malformed("b"));
        assert_eq!(
            collector.emit(DiagnosticOwner::Inventory, malformed("c")),
            DiagnosticEmission::Saturated
        );
        let values = collector.finish_inventory();
        assert_eq!(values.len(), 3);
        assert!(matches!(
            values.last(),
            Some(AgentAssetDiagnostic::Truncated {
                accepted: 2,
                observed_at_least: 3,
                ..
            })
        ));
    }

    #[test]
    fn duplicate_does_not_consume_capacity_or_raise_observed_count() {
        let mut collector = DiagnosticCollector::new(3);
        let value = AgentAssetDiagnostic::InvalidNativeId {
            category: AgentAssetCategory::Mcp,
        };
        assert_eq!(
            collector.emit(DiagnosticOwner::Inventory, value.clone()),
            DiagnosticEmission::Accepted
        );
        assert_eq!(
            collector.emit(DiagnosticOwner::Inventory, value),
            DiagnosticEmission::Duplicate
        );
        assert_eq!(collector.finish_inventory().len(), 1);
    }

    #[test]
    fn zero_and_one_limits_are_fail_closed() {
        let mut zero = DiagnosticCollector::new(0);
        assert_eq!(
            zero.emit(DiagnosticOwner::Inventory, malformed("a")),
            DiagnosticEmission::Saturated
        );
        assert!(zero.finish_inventory().is_empty());

        let mut one = DiagnosticCollector::new(1);
        one.emit(DiagnosticOwner::Inventory, malformed("a"));
        let values = one.finish_inventory();
        assert_eq!(values.len(), 1);
        assert!(matches!(
            values[0],
            AgentAssetDiagnostic::Truncated {
                accepted: 0,
                observed_at_least: 1,
                ..
            }
        ));
    }

    #[test]
    fn rebind_owner_is_atomic_and_does_not_reopen_capacity() {
        let mut collector = DiagnosticCollector::new(3);
        let old = DiagnosticOwner::Source("provisional".to_owned());
        let new = DiagnosticOwner::Source("final".to_owned());
        let diagnostic = AgentAssetDiagnostic::ReadFailed {
            source_id: "provisional".to_owned(),
            error_kind: crate::models::AgentAssetIoErrorKind::PermissionDenied,
        };
        assert_eq!(
            collector.emit(old.clone(), diagnostic),
            DiagnosticEmission::Accepted
        );
        assert_eq!(collector.retained_count(), 1);
        assert_eq!(collector.seen_len(), 1);
        assert_eq!(collector.rebind_owner(&old, new.clone(), Some("final")), 1);
        assert_eq!(collector.retained_count(), 1);
        assert_eq!(collector.seen_len(), 1);
        assert_eq!(collector.rebind_owner(&old, new.clone(), Some("final")), 0);
        assert!(collector.take_owner(&old).is_empty());
        assert!(matches!(
            collector.take_owner(&new).as_slice(),
            [AgentAssetDiagnostic::ReadFailed { source_id, .. }] if source_id == "final"
        ));
    }

    #[test]
    fn take_owner_then_rebind_preserves_historical_dedup() {
        let mut collector = DiagnosticCollector::new(6);
        let old = DiagnosticOwner::Source("old".to_owned());
        let new = DiagnosticOwner::Source("new".to_owned());
        let historical = malformed("historical");
        let current = malformed("current");

        assert_eq!(
            collector.emit(old.clone(), historical.clone()),
            DiagnosticEmission::Accepted
        );
        assert_eq!(collector.take_owner(&old), vec![historical.clone()]);
        assert_eq!(collector.retained_len(), 0);
        assert_eq!(collector.retained_count(), 1);
        assert_eq!(collector.seen_len(), 1);
        assert_eq!(
            collector.emit(old.clone(), current.clone()),
            DiagnosticEmission::Accepted
        );

        assert_eq!(collector.rebind_owner(&old, new.clone(), None), 1);
        assert_eq!(collector.retained_len(), 1);
        assert_eq!(collector.retained_count(), 2);
        assert_eq!(collector.seen_len(), 2);
        assert_eq!(
            collector.emit(old, historical),
            DiagnosticEmission::Duplicate
        );
        assert_eq!(collector.emit(new, current), DiagnosticEmission::Duplicate);
    }

    #[test]
    fn rebind_collision_keeps_first_occurrence_and_distinct_sentinels_in_order() {
        let mut collector = DiagnosticCollector::new(6);
        let old = DiagnosticOwner::Source("old".to_owned());
        let destination = DiagnosticOwner::Source("new".to_owned());
        collector.emit(old.clone(), malformed("before"));
        collector.emit(destination.clone(), malformed("collision"));
        collector.emit(old.clone(), malformed("collision"));
        collector.emit(old.clone(), malformed("after"));

        assert_eq!(collector.rebind_owner(&old, destination.clone(), None), 3);
        assert_eq!(collector.retained_len(), 3);
        assert_eq!(collector.retained_count(), 3);
        assert_eq!(collector.seen_len(), 3);
        assert_eq!(
            collector.take_owner(&destination),
            vec![
                malformed("before"),
                malformed("collision"),
                malformed("after")
            ]
        );
    }

    #[test]
    fn rebind_owner_preserves_multiple_sources_at_exact_capacity() {
        let mut collector = DiagnosticCollector::new(3);
        let first = DiagnosticOwner::Source("one".to_owned());
        let second = DiagnosticOwner::Source("two".to_owned());
        collector.emit(
            first.clone(),
            AgentAssetDiagnostic::ReadFailed {
                source_id: "one".to_owned(),
                error_kind: crate::models::AgentAssetIoErrorKind::Other,
            },
        );
        collector.emit(
            second.clone(),
            AgentAssetDiagnostic::ReadFailed {
                source_id: "two".to_owned(),
                error_kind: crate::models::AgentAssetIoErrorKind::Other,
            },
        );
        let retained = collector.retained_count();
        assert_eq!(
            collector.rebind_owner(
                &first,
                DiagnosticOwner::Source("one-final".to_owned()),
                Some("one-final")
            ),
            1
        );
        assert_eq!(
            collector.rebind_owner(
                &second,
                DiagnosticOwner::Source("two-final".to_owned()),
                Some("two-final")
            ),
            1
        );
        assert_eq!(collector.retained_count(), retained);
        assert!(!collector.has_regular_capacity());
        assert_eq!(collector.seen_len(), 2);
    }

    #[test]
    fn rebind_owner_at_exact_capacity_keeps_all_retained_entries() {
        let mut collector = DiagnosticCollector::new(3);
        let old = DiagnosticOwner::Source("old".to_owned());
        let other = DiagnosticOwner::Source("other".to_owned());
        collector.emit(old.clone(), malformed("old"));
        collector.emit(other.clone(), malformed("other"));
        assert_eq!(collector.retained_count(), 2);
        assert_eq!(
            collector.rebind_owner(&old, DiagnosticOwner::Source("new".to_owned()), Some("new"),),
            1
        );
        assert_eq!(collector.retained_count(), 2);
        assert_eq!(collector.seen_len(), 2);
        assert_eq!(collector.finish_inventory().len(), 2);
    }

    #[test]
    fn rebind_owner_coalesces_existing_destination_key_exactly_once() {
        let mut collector = DiagnosticCollector::new(4);
        let old = DiagnosticOwner::Source("old".to_owned());
        let destination = DiagnosticOwner::Source("final".to_owned());
        let diagnostic = |source_id: &str| AgentAssetDiagnostic::ReadFailed {
            source_id: source_id.to_owned(),
            error_kind: crate::models::AgentAssetIoErrorKind::Other,
        };
        collector.emit(old.clone(), diagnostic("old"));
        collector.emit(destination.clone(), diagnostic("final"));
        assert_eq!(collector.retained_count(), 2);
        assert_eq!(
            collector.rebind_owner(&old, destination.clone(), Some("final")),
            1
        );
        assert_eq!(collector.retained_count(), 1);
        assert_eq!(collector.retained_len(), 1);
        assert_eq!(collector.seen_len(), 1);
        assert_eq!(collector.rebind_owner(&old, destination, Some("final")), 0);
        assert!(matches!(
            collector.finish_inventory().as_slice(),
            [AgentAssetDiagnostic::ReadFailed { source_id, .. }] if source_id == "final"
        ));
    }

    #[test]
    fn rebind_owner_coalesces_multiple_old_keys_into_one_destination() {
        let mut collector = DiagnosticCollector::new(5);
        let first = DiagnosticOwner::Source("one".to_owned());
        let second = DiagnosticOwner::Source("two".to_owned());
        let destination = DiagnosticOwner::Source("final".to_owned());
        collector.emit(
            first.clone(),
            AgentAssetDiagnostic::ReadFailed {
                source_id: "one".to_owned(),
                error_kind: crate::models::AgentAssetIoErrorKind::Other,
            },
        );
        collector.emit(
            second.clone(),
            AgentAssetDiagnostic::ReadFailed {
                source_id: "two".to_owned(),
                error_kind: crate::models::AgentAssetIoErrorKind::Other,
            },
        );
        assert_eq!(
            collector.rebind_owner(&first, destination.clone(), Some("final")),
            1
        );
        assert_eq!(
            collector.rebind_owner(&second, destination, Some("final")),
            1
        );
        assert_eq!(collector.retained_count(), 1);
        assert_eq!(collector.retained_len(), 1);
        assert_eq!(collector.seen_len(), 1);
    }

    #[test]
    fn rebind_collision_at_exact_capacity_does_not_create_truncation() {
        let mut collector = DiagnosticCollector::new(3);
        let old = DiagnosticOwner::Source("old".to_owned());
        let destination = DiagnosticOwner::Source("final".to_owned());
        collector.emit(
            old.clone(),
            AgentAssetDiagnostic::ReadFailed {
                source_id: "old".to_owned(),
                error_kind: crate::models::AgentAssetIoErrorKind::Other,
            },
        );
        collector.emit(
            destination.clone(),
            AgentAssetDiagnostic::ReadFailed {
                source_id: "final".to_owned(),
                error_kind: crate::models::AgentAssetIoErrorKind::Other,
            },
        );
        assert!(!collector.has_regular_capacity());
        assert_eq!(collector.rebind_owner(&old, destination, Some("final")), 1);
        assert_eq!(collector.retained_count(), 1);
        assert!(collector
            .finish_inventory()
            .iter()
            .all(|diagnostic| { !matches!(diagnostic, AgentAssetDiagnostic::Truncated { .. }) }));
    }

    #[test]
    fn saturation_does_not_grow_dedup_state_or_reopen_capacity_after_owner_take() {
        let mut collector = DiagnosticCollector::new(3);
        collector.emit(DiagnosticOwner::Source("one".to_string()), malformed("a"));
        collector.emit(DiagnosticOwner::Source("two".to_string()), malformed("b"));
        assert_eq!(collector.seen_len(), 2);
        assert!(matches!(
            collector.emit(DiagnosticOwner::Source("three".to_string()), malformed("c")),
            DiagnosticEmission::Saturated
        ));
        for index in 0..1000 {
            collector.emit(
                DiagnosticOwner::Source(format!("overflow-{index}")),
                malformed("overflow"),
            );
        }
        assert_eq!(collector.seen_len(), 2);
        assert_eq!(collector.retained_count(), 2);
        assert!(!collector.has_regular_capacity());
        assert_eq!(
            collector
                .take_owner(&DiagnosticOwner::Source("one".to_string()))
                .len(),
            1
        );
        assert!(!collector.has_regular_capacity());
    }
}
