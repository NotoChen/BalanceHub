use crate::models::{
    AgentAssetActionKind, AgentAssetApplyRequest, AgentAssetMutationError,
    AgentAssetMutationErrorKind,
};
use std::{
    collections::BTreeMap,
    sync::Mutex,
    time::{Duration, Instant},
};

pub(crate) const DEFAULT_PLAN_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_PLAN_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_PENDING_PLANS: usize = 128;
const MAX_TOMBSTONES: usize = 256;

pub(crate) struct BoundPlan<T, A = AgentAssetActionKind> {
    pub(crate) actor: String,
    pub(crate) asset_id: String,
    pub(crate) action: A,
    pub(crate) expires: Instant,
    pub(crate) value: T,
}

#[derive(Clone, Copy)]
enum TombstoneKind {
    Consumed,
    Expired,
}

struct Registry<T, A> {
    pending: BTreeMap<String, BoundPlan<T, A>>,
    tombstones: BTreeMap<String, (Instant, TombstoneKind)>,
}

impl<T, A> Default for Registry<T, A> {
    fn default() -> Self {
        Self {
            pending: BTreeMap::new(),
            tombstones: BTreeMap::new(),
        }
    }
}

pub(crate) struct PlanRegistry<T, A = AgentAssetActionKind> {
    state: Mutex<Registry<T, A>>,
}

impl<T, A> Default for PlanRegistry<T, A> {
    fn default() -> Self {
        Self {
            state: Mutex::new(Registry::default()),
        }
    }
}

impl<T, A: PartialEq> PlanRegistry<T, A> {
    /// Release expired private plans before an owner reserves new cache space.
    pub(crate) fn prune_expired(&self, now: Instant) {
        if let Ok(mut state) = self.state.lock() {
            state.prune(now);
        }
    }
    pub(crate) fn remove_actor(&self, actor: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.pending.retain(|_, plan| plan.actor != actor);
        }
    }
    pub(crate) fn issue(
        &self,
        actor: &str,
        asset_id: &str,
        action: A,
        value: T,
        now: Instant,
        ttl: Duration,
    ) -> Result<String, AgentAssetMutationError> {
        if actor.is_empty() || asset_id.is_empty() || ttl.is_zero() || ttl > MAX_PLAN_TTL {
            return Err(AgentAssetMutationError::new(
                AgentAssetMutationErrorKind::InvalidRequest,
            ));
        }
        let mut state = self.state.lock().map_err(|_| internal_error())?;
        state.prune(now);
        if state.pending.len() >= MAX_PENDING_PLANS {
            return Err(AgentAssetMutationError::new(
                AgentAssetMutationErrorKind::CapacityExceeded,
            ));
        }
        let token = opaque_id()?;
        state.pending.insert(
            token.clone(),
            BoundPlan {
                actor: actor.to_owned(),
                asset_id: asset_id.to_owned(),
                action,
                expires: now + ttl,
                value,
            },
        );
        Ok(token)
    }

    /// Validate and consume under one mutex. A different window cannot burn a
    /// rightful window's token; no caller ever receives a partially consumed plan.
    pub(crate) fn consume_bound(
        &self,
        actor: &str,
        plan_token: &str,
        target_id: &str,
        action: &A,
        now: Instant,
    ) -> Result<BoundPlan<T, A>, AgentAssetMutationError> {
        let mut state = self.state.lock().map_err(|_| internal_error())?;
        state.prune(now);
        let plan = state.pending.get(plan_token).ok_or_else(|| {
            let kind = match state.tombstones.get(plan_token) {
                Some((_, TombstoneKind::Expired)) => AgentAssetMutationErrorKind::PlanExpired,
                Some((_, TombstoneKind::Consumed)) => AgentAssetMutationErrorKind::PlanConsumed,
                None => AgentAssetMutationErrorKind::InvalidRequest,
            };
            AgentAssetMutationError::new(kind)
        })?;
        let mismatch = if plan.actor != actor {
            Some(AgentAssetMutationErrorKind::ActorMismatch)
        } else if plan.asset_id != target_id {
            Some(AgentAssetMutationErrorKind::TargetMismatch)
        } else if &plan.action != action {
            Some(AgentAssetMutationErrorKind::ActionMismatch)
        } else {
            None
        };
        if let Some(kind) = mismatch {
            return Err(AgentAssetMutationError::new(kind));
        }
        let plan = state
            .pending
            .remove(plan_token)
            .ok_or_else(internal_error)?;
        state
            .tombstones
            .insert(plan_token.to_owned(), (now, TombstoneKind::Consumed));
        Ok(plan)
    }
}

impl<T> PlanRegistry<T> {
    pub(crate) fn consume(
        &self,
        actor: &str,
        request: &AgentAssetApplyRequest,
        now: Instant,
    ) -> Result<BoundPlan<T>, AgentAssetMutationError> {
        self.consume_bound(
            actor,
            &request.plan_token,
            &request.asset_id,
            &request.action,
            now,
        )
    }
}

impl<T, A> Registry<T, A> {
    fn prune(&mut self, now: Instant) {
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, plan)| now >= plan.expires)
            .map(|(token, _)| token.clone())
            .collect();
        for token in expired {
            self.pending.remove(&token);
            self.tombstones.insert(token, (now, TombstoneKind::Expired));
        }
        self.tombstones
            .retain(|_, (at, _)| now.saturating_duration_since(*at) < MAX_PLAN_TTL);
        while self.tombstones.len() > MAX_TOMBSTONES {
            let oldest = self
                .tombstones
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(token, _)| token.clone());
            if let Some(token) = oldest {
                self.tombstones.remove(&token);
            }
        }
    }
}

pub(crate) fn opaque_id() -> Result<String, AgentAssetMutationError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| internal_error())?;
    use std::fmt::Write;
    let mut id = String::with_capacity(64);
    for byte in bytes {
        write!(&mut id, "{byte:02x}").map_err(|_| internal_error())?;
    }
    Ok(id)
}

fn internal_error() -> AgentAssetMutationError {
    AgentAssetMutationError::new(AgentAssetMutationErrorKind::InternalFailure)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(token: String) -> AgentAssetApplyRequest {
        AgentAssetApplyRequest {
            plan_token: token,
            asset_id: "asset".into(),
            action: AgentAssetActionKind::Disable,
        }
    }

    #[test]
    fn tokens_are_single_use_and_expire_at_the_deadline() {
        let registry = PlanRegistry::default();
        let now = Instant::now();
        let token = registry
            .issue(
                "window",
                "asset",
                AgentAssetActionKind::Disable,
                7,
                now,
                DEFAULT_PLAN_TTL,
            )
            .unwrap();
        let apply = request(token);
        assert_eq!(registry.consume("window", &apply, now).unwrap().value, 7);
        assert_eq!(
            registry.consume("window", &apply, now).err().unwrap().kind,
            AgentAssetMutationErrorKind::PlanConsumed
        );
        let token = registry
            .issue(
                "window",
                "asset",
                AgentAssetActionKind::Disable,
                8,
                now,
                DEFAULT_PLAN_TTL,
            )
            .unwrap();
        assert_eq!(
            registry
                .consume("window", &request(token), now + DEFAULT_PLAN_TTL)
                .err()
                .unwrap()
                .kind,
            AgentAssetMutationErrorKind::PlanExpired
        );
    }

    #[test]
    fn binding_is_checked_atomically_without_invalidating_another_actor() {
        let registry = PlanRegistry::default();
        let now = Instant::now();
        let token = registry
            .issue(
                "window-a",
                "asset",
                AgentAssetActionKind::Disable,
                (),
                now,
                DEFAULT_PLAN_TTL,
            )
            .unwrap();
        let mut request = request(token);
        assert_eq!(
            registry
                .consume("window-b", &request, now)
                .err()
                .unwrap()
                .kind,
            AgentAssetMutationErrorKind::ActorMismatch
        );
        request.asset_id = "other".into();
        assert_eq!(
            registry
                .consume("window-a", &request, now)
                .err()
                .unwrap()
                .kind,
            AgentAssetMutationErrorKind::TargetMismatch
        );
        request.asset_id = "asset".into();
        request.action = AgentAssetActionKind::Enable;
        assert_eq!(
            registry
                .consume("window-a", &request, now)
                .err()
                .unwrap()
                .kind,
            AgentAssetMutationErrorKind::ActionMismatch
        );
        request.action = AgentAssetActionKind::Disable;
        assert!(registry.consume("window-a", &request, now).is_ok());
    }

    #[test]
    fn generic_action_tokens_retain_binding_and_expire_without_sleeping() {
        let registry: PlanRegistry<u8, String> = PlanRegistry::default();
        let now = Instant::now();
        let action = "install:codex".to_owned();
        let token = registry
            .issue("window", "target", action.clone(), 9, now, DEFAULT_PLAN_TTL)
            .unwrap();
        assert_eq!(
            registry
                .consume_bound("window", &token, "target", &"install:grok".to_owned(), now)
                .err()
                .unwrap()
                .kind,
            AgentAssetMutationErrorKind::ActionMismatch
        );
        assert_eq!(
            registry
                .consume_bound("window", &token, "target", &action, now)
                .unwrap()
                .value,
            9
        );
        let expiring = registry
            .issue(
                "window",
                "target",
                action.clone(),
                10,
                now,
                DEFAULT_PLAN_TTL,
            )
            .unwrap();
        assert_eq!(
            registry
                .consume_bound(
                    "window",
                    &expiring,
                    "target",
                    &action,
                    now + DEFAULT_PLAN_TTL
                )
                .err()
                .unwrap()
                .kind,
            AgentAssetMutationErrorKind::PlanExpired
        );
        let other = registry
            .issue("other", "target", action.clone(), 11, now, DEFAULT_PLAN_TTL)
            .unwrap();
        let removed = registry
            .issue(
                "window",
                "target",
                action.clone(),
                12,
                now,
                DEFAULT_PLAN_TTL,
            )
            .unwrap();
        registry.remove_actor("window");
        assert!(registry
            .consume_bound("window", &removed, "target", &action, now)
            .is_err());
        assert_eq!(
            registry
                .consume_bound("other", &other, "target", &action, now)
                .unwrap()
                .value,
            11
        );
    }

    #[test]
    fn replay_race_returns_exactly_one_plan() {
        let registry = std::sync::Arc::new(PlanRegistry::default());
        let now = Instant::now();
        let token = registry
            .issue(
                "window",
                "asset",
                AgentAssetActionKind::Disable,
                (),
                now,
                DEFAULT_PLAN_TTL,
            )
            .unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let registry = registry.clone();
                let barrier = barrier.clone();
                let request = request(token.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    registry.consume("window", &request, now).is_ok()
                })
            })
            .collect();
        barrier.wait();
        assert_eq!(
            threads
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .filter(|success| *success)
                .count(),
            1
        );
    }
}
