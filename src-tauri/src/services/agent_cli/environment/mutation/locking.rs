use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct MutationLocks {
    held: Mutex<BTreeSet<String>>,
    changed: Condvar,
}

/// Every native writer, including optional session integration, participates
/// in the same domains. A second controller must not create its own lock set.
pub(crate) fn shared_locks() -> Arc<MutationLocks> {
    static LOCKS: OnceLock<Arc<MutationLocks>> = OnceLock::new();
    Arc::clone(LOCKS.get_or_init(|| Arc::new(MutationLocks::default())))
}

pub(crate) struct MutationGuard<'a> {
    locks: &'a MutationLocks,
    keys: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LockFailure {
    Canceled,
    TimedOut,
    Internal,
}

impl MutationLocks {
    /// Acquire the complete physical write set in one transaction. This avoids
    /// deadlocks and makes a user+project CLI edit conflict with either file edit.
    pub(crate) fn acquire(
        &self,
        keys: &[String],
        canceled: &AtomicBool,
        deadline: Instant,
    ) -> Result<MutationGuard<'_>, LockFailure> {
        let mut keys = keys.to_vec();
        keys.sort();
        keys.dedup();
        if keys.is_empty() || keys.iter().any(String::is_empty) {
            return Err(LockFailure::Internal);
        }
        let mut held = self.held.lock().map_err(|_| LockFailure::Internal)?;
        loop {
            if canceled.load(Ordering::Acquire) {
                return Err(LockFailure::Canceled);
            }
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or(LockFailure::TimedOut)?;
            if keys.iter().all(|key| !held.contains(key)) {
                held.extend(keys.iter().cloned());
                return Ok(MutationGuard { locks: self, keys });
            }
            held = self
                .changed
                .wait_timeout(held, remaining.min(Duration::from_millis(25)))
                .map_err(|_| LockFailure::Internal)?
                .0;
        }
    }
}

impl Drop for MutationGuard<'_> {
    fn drop(&mut self) {
        let mut held = self
            .locks
            .held
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for key in &self.keys {
            held.remove(key);
        }
        self.locks.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_write_sets_wait_and_disjoint_sets_proceed() {
        let locks = MutationLocks::default();
        let canceled = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(1);
        let first = locks
            .acquire(&["user".into(), "project".into()], &canceled, deadline)
            .unwrap();
        let disjoint = locks
            .acquire(&["other".into()], &canceled, deadline)
            .unwrap();
        assert!(matches!(
            locks.acquire(
                &["project".into()],
                &canceled,
                Instant::now() + Duration::from_millis(5)
            ),
            Err(LockFailure::TimedOut)
        ));
        drop(first);
        drop(disjoint);
        assert!(locks.acquire(&["user".into()], &canceled, deadline).is_ok());
    }
    #[test]
    fn cancel_does_not_acquire_a_write_domain() {
        let locks = MutationLocks::default();
        let canceled = AtomicBool::new(true);
        assert!(matches!(
            locks.acquire(
                &["file".into()],
                &canceled,
                Instant::now() + Duration::from_secs(1)
            ),
            Err(LockFailure::Canceled)
        ));
    }
}
