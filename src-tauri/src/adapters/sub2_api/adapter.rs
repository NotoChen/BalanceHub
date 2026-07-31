//! Sub2API protocol adapter.
//!
//! Sub2API deliberately lives beside the NewAPI implementation instead of
//! sharing its request helpers.  The two protocols use different response
//! envelopes and, more importantly, different authentication semantics:
//! Sub2API uses JWTs for account APIs and API Keys only for gateway APIs.

mod account;
mod credentials;
mod refresh;
mod usage;

pub(crate) struct Sub2ApiAdapter;
