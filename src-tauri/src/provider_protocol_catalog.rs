//! Single compile-time catalog for built-in provider protocol identities.
//!
//! The model enum and protocol registry consume the same variant, serialized key and descriptor
//! module. Adding a protocol therefore cannot update the persisted identity while forgetting to
//! register its runtime definition, or vice versa.

macro_rules! for_each_provider_protocol {
    ($consumer:ident) => {
        $consumer! {
            NewApi => { key: "newApi", module: new_api },
            Sub2Api => { key: "sub2Api", module: sub2_api },
            Api => { key: "api", module: api },
        }
    };
}

pub(crate) use for_each_provider_protocol;
