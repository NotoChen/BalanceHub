//! Hook bridge modules.  Event contract, spool and stdin ingest have separate
//! ownership boundaries so future Hook adapters do not grow one core file.

pub mod event;
pub mod ingest;
pub mod spool;

pub use event::{
    HookDecodeError, HookEventDecoder, NormalizedHookEvent, NormalizedHookLifecycle,
    NORMALIZED_HOOK_SCHEMA_VERSION,
};
#[cfg(test)]
pub use ingest::{ingest_payload, ingest_stdin};
pub use ingest::{ingest_payload_with_context, HookIngestContext};
#[cfg(test)]
pub use spool::{HookSpoolLimits, DEFAULT_HOOK_PAYLOAD_MAX_BYTES, DEFAULT_SPOOL_MAX_AGE_MILLIS};
pub use spool::{HookSpoolRepository, SpoolDiagnostic, DEFAULT_SPOOL_BATCH_SIZE};
