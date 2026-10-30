//! Local installation policies and the App-owned release/update worker.
mod store;
mod worker;

pub(super) use store::AutomaticUpdates;
pub(crate) use worker::start_worker;

pub(super) const AUTOMATIC_ACTOR: &str = "balancehub:automatic-agent-updates";
pub(crate) const CATALOG_EVENT: &str = "agent-lifecycle-catalog-updated";
pub(crate) const OPERATION_EVENT: &str = "agent-lifecycle-operation-updated";
