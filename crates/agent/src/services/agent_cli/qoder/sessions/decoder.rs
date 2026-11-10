//! Qoder CLI transcript decoder.
//!
//! Qoder writes Claude-compatible JSONL, so the shared Claude record decoder
//! handles the content blocks and turn state. The wrapper differs in the
//! message id prefix and skips Claude's `subagents` path filter, which does
//! not exist in Qoder's project layout.

use crate::services::agent_cli::contracts::SessionTranscriptMutation;
use serde_json::Value;
use std::path::Path;

pub(super) fn decode(
    _path: &Path,
    sequence: u64,
    line: &[u8],
    state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    crate::services::agent_cli::claude::sessions::decoder::decode_record(
        "qoder", &value, sequence, state,
    )
}
