//! Provider-independent native configuration editing and guarded transactions.
mod catalog_content;
mod codec;
pub(crate) mod contracts;
mod errors;
pub(crate) mod native_support;
mod operations;
mod receipts;
mod resource;
mod resource_read;
mod resource_selection;
mod selection;
mod service;
pub(crate) mod sources;
mod writer;

pub(crate) use catalog_content::read_catalog_content;
pub(crate) use service::ConfigurationService;

const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
const MAX_PRIVATE_BYTES: usize = 16 * 1024 * 1024;
const MAX_DOCUMENTS: usize = 4;
const EDIT_TTL: std::time::Duration = std::time::Duration::from_secs(30 * 60);
const PLAN_TTL: std::time::Duration = std::time::Duration::from_secs(5 * 60);
