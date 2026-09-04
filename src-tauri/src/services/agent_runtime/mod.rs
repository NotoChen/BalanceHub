//! Unified Agent runtime projection.
//!
//! The reducer is intentionally independent of Tauri state, the filesystem and
//! Hook implementations.  Each producer supplies normalized events; the same
//! rules therefore apply to BalanceHub launches and future external Hooks.

pub mod decoders;
pub(crate) mod enrichment;
pub mod hook;
mod launcher;
pub mod managed_hook;
mod reducer;
pub mod repository;
pub mod service;

#[cfg(test)]
mod hook_tests;
#[cfg(test)]
mod tests;
