//! Explicit upgrade plans for existing Agent installations and independently tracked operations.

mod filesystem;
mod homebrew;
mod journal;
mod launch;
pub(crate) use journal::initialize as initialize_journal;
pub(crate) use launch::{annotate as annotate_launch, annotate_batch as annotate_launches};
mod native;
mod npm;
mod planning;
mod service;

pub(crate) use service::LifecycleService;

#[cfg(test)]
mod tests;
