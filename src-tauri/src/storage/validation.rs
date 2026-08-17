use crate::{
    limits,
    models::{
        normalize_api_key_for_protocol, normalize_invite_link, normalize_provider_auth, AppData,
        CURRENT_SCHEMA_VERSION,
    },
};

pub(super) fn validate_app_data_schema(data: &AppData) -> Result<(), String> {
    if data.schema_version != CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "配置结构版本不兼容：当前应用只支持 schemaVersion {}，检测到 {}。请重新初始化配置或导入新版配置。",
            CURRENT_SCHEMA_VERSION, data.schema_version
        ));
    }
    limits::validate_app_data_limits(data)
}

pub(super) fn normalize_provider_cached_values(data: &mut AppData) -> bool {
    let mut changed = false;

    for provider in &mut data.providers {
        let current_auth = provider.auth.clone();
        provider.auth = normalize_provider_auth(current_auth.clone(), provider.identity.protocol);
        if provider.auth != current_auth {
            changed = true;
        }

        let normalized =
            normalize_api_key_for_protocol(&provider.auth.api_key, provider.identity.protocol);
        if normalized != provider.auth.api_key {
            provider.auth.api_key = normalized;
            changed = true;
        }

        let normalized_invite_link = normalize_invite_link(&provider.capabilities.invite_link);
        if normalized_invite_link != provider.capabilities.invite_link {
            provider.capabilities.invite_link = normalized_invite_link;
            changed = true;
        }
    }
    changed
}
