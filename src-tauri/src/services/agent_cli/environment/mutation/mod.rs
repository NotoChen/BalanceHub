//! Confirmed asset operations. Native adapters prepare exact plans; this kernel
//! owns actor binding, preconditions, locks, commit boundaries and outcomes.

pub(crate) mod atomic;
pub(crate) mod catalog;
pub(crate) mod execution;
mod inspection;
pub(crate) use inspection::scoped_inspector;

mod files;
pub(crate) mod locking;
mod prepared;
mod service;
pub(crate) mod token;

pub(crate) use execution::ExactCliCommand;
pub(crate) use files::{GuardedDirectory, GuardedFile, WriteObservation};
pub(crate) use prepared::{
    prepare_request, AgentAssetMechanismCatalog, AgentAssetMutationPreparer,
    AgentAssetNativeUnavailable, MutationExecution, MutationInspector, MutationInventory,
    MutationPreparation, MutationVerification, PreparedMutation,
};
pub(crate) use service::{classify_outcome, ApplyEvidence, MutationService};

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod native_acceptance;
#[cfg(test)]
mod native_preparer_tests;
#[cfg(test)]
pub(crate) mod native_test_support;
#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod overlay_plan_tests;
#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod shared_source_tests;
#[cfg(test)]
pub(super) mod test_support;
#[cfg(test)]
mod tests;
