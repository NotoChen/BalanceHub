//! Bounded, read-only inventory of documented Agent user/workspace resources.
//!
//! This module deliberately does not execute any discovered hook, plugin or status command.

mod inventory;
mod path_access;
mod versioning;

#[cfg(test)]
mod tests;

pub(crate) use inventory::{from_templates, inventory, AssetRoot, AssetTemplate};
pub(crate) use path_access::{open_asset_path, read_asset};
pub(crate) use versioning::check_latest_versions;
