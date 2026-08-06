#[path = "provider/defaults.rs"]
mod defaults;
#[path = "provider/input.rs"]
mod input;
#[path = "provider/normalize.rs"]
mod normalize;
#[path = "provider/state.rs"]
mod state;

pub(crate) use input::normalize_provider_auth;
pub use input::ProviderInput;
pub(crate) use normalize::{
    check_in_message_indicates_disabled, normalize_api_key, normalize_api_key_for_protocol,
    normalize_invite_link, provider_duplicate_kind, ProviderDuplicateKind,
};
pub use state::*;
