use super::format::{SyncDocument, SyncDocuments};
use crate::models::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// Only portable user preferences enter the wire format. Automatic execution,
/// local paths, proxies and browser sessions remain owned by each device.
const SETTINGS: &[(&str, &str)] = &[
    ("themeMode", "主题"),
    ("refreshInterval", "刷新间隔"),
    ("checkInTime", "签到时间"),
    ("livenessModel", "测活模型"),
    ("livenessIntervalMode", "测活间隔方式"),
    ("livenessInterval", "测活间隔"),
    ("livenessRandomMinInterval", "测活最短间隔"),
    ("livenessRandomMaxInterval", "测活最长间隔"),
    ("livenessTimeout", "测活超时"),
    ("livenessPromptMode", "测活提示词方式"),
    ("livenessFixedPrompt", "测活提示词"),
    ("livenessPromptLibrary", "提示词库"),
    ("livenessPlaceholderPools", "提示词变量"),
    ("livenessNumberMin", "提示词数值下限"),
    ("livenessNumberMax", "提示词数值上限"),
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableProvider {
    identity: ProviderIdentityInput,
    auth: PortableAuth,
    cli: ProviderCliInput,
    enabled: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableAuth {
    mode: AuthMode,
    source: AuthSource,
    api_key: String,
    api_key_token_id: String,
    api_user: String,
    access_token: String,
    session_cookie: String,
    login_username: String,
    login_password: String,
    keys: Vec<PortableKey>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableKey {
    local_id: String,
    local_name: String,
    name: String,
    key: String,
    token_id: String,
    user_id: String,
}

fn portable_auth(auth: &ProviderAuth) -> PortableAuth {
    PortableAuth {
        mode: auth.mode,
        source: auth.source,
        api_key: auth.api_key.clone(),
        api_key_token_id: auth.api_key_token_id.clone(),
        api_user: auth.api_user.clone(),
        // Expiring tokens and rotating refresh tokens are device sessions.
        access_token: if auth.access_token_expires_at.is_none() {
            auth.access_token.clone()
        } else {
            String::new()
        },
        session_cookie: if auth.mode == AuthMode::Session || auth.source == AuthSource::Manual {
            auth.session_cookie.clone()
        } else {
            String::new()
        },
        login_username: auth.login_username.clone(),
        login_password: auth.login_password.clone(),
        keys: auth
            .api_key_options
            .iter()
            .map(|key| PortableKey {
                local_id: key.local_id.clone(),
                local_name: key.local_name.clone(),
                name: key.name.clone(),
                key: key.key.clone(),
                token_id: key.token_id.clone(),
                user_id: key.user_id.clone(),
            })
            .collect(),
    }
}

pub(crate) fn app_documents(data: &AppData) -> Result<SyncDocuments, String> {
    let mut documents = BTreeMap::new();
    for provider in &data.providers {
        let portable = PortableProvider {
            identity: ProviderIdentityInput {
                name: provider.identity.name.clone(),
                base_url: provider.identity.base_url.clone(),
                protocol: provider.identity.protocol,
                remark: provider.identity.remark.clone(),
                user_id: provider.identity.user_id.clone(),
                backup_urls: provider.identity.backup_urls.clone(),
            },
            auth: portable_auth(&provider.auth),
            cli: ProviderCliInput {
                preferred_model: provider.cli.preferred_model.clone(),
            },
            enabled: provider.runtime.enabled,
        };
        documents.insert(
            format!("provider/{}", provider.identity.id),
            SyncDocument {
                title: provider.display_label(),
                category: "中转站".to_owned(),
                value: serde_json::to_value(portable).map_err(|_| "中转站同步数据无效")?,
            },
        );
    }
    if !data.providers.is_empty() {
        documents.insert(
            "order/providers".to_owned(),
            SyncDocument {
                title: "中转站排序".to_owned(),
                category: "偏好".to_owned(),
                value: json!(data
                    .providers
                    .iter()
                    .map(|p| &p.identity.id)
                    .collect::<Vec<_>>()),
            },
        );
    }
    let settings = serde_json::to_value(&data.settings).map_err(|_| "偏好同步数据无效")?;
    let defaults = serde_json::to_value(AppSettings::default()).map_err(|_| "偏好默认值无效")?;
    for (key, title) in SETTINGS {
        if settings[*key] != defaults[*key] {
            documents.insert(
                format!("setting/{key}"),
                SyncDocument {
                    title: (*title).to_owned(),
                    category: "偏好".to_owned(),
                    value: settings[*key].clone(),
                },
            );
        }
    }
    Ok(documents)
}

pub(crate) fn app_changed(before: &AppData, after: &AppData) -> bool {
    match (app_documents(before), app_documents(after)) {
        (Ok(left), Ok(right)) => left != right,
        _ => true,
    }
}

pub(super) fn apply_app_documents(
    current: &AppData,
    documents: &SyncDocuments,
) -> Result<AppData, String> {
    let mut next = current.clone();
    let existing: BTreeMap<_, _> = current
        .providers
        .iter()
        .map(|p| (p.identity.id.as_str(), p))
        .collect();
    let mut providers = BTreeMap::new();
    for (key, document) in documents {
        if let Some(id) = key.strip_prefix("provider/") {
            if id.is_empty() || id.len() > 256 || id.contains('/') {
                return Err("云端中转站标识无效".to_owned());
            }
            let value: PortableProvider =
                serde_json::from_value(document.value.clone()).map_err(|_| "云端中转站格式无效")?;
            let parsed_url =
                reqwest::Url::parse(&value.identity.base_url).map_err(|_| "云端中转站地址无效")?;
            if !matches!(parsed_url.scheme(), "http" | "https") {
                return Err("云端中转站地址不支持".to_owned());
            }
            let old = existing.get(id).copied();
            let mut provider = old.cloned().unwrap_or_else(|| {
                Provider::from_input(
                    ProviderInput {
                        identity: value.identity.clone(),
                        ..ProviderInput::default()
                    },
                    id.to_owned(),
                )
            });
            let old_auth =
                serde_json::to_value(portable_auth(&provider.auth)).map_err(|_| "认证数据无效")?;
            let new_auth = serde_json::to_value(&value.auth).map_err(|_| "认证数据无效")?;
            let identity_changed = provider.identity.base_url != value.identity.base_url
                || provider.identity.protocol != value.identity.protocol
                || provider.identity.user_id != value.identity.user_id;
            if old.is_none() || old_auth != new_auth || identity_changed {
                let auth = value.auth;
                let session_changed = identity_changed
                    || provider.auth.login_username != auth.login_username
                    || provider.auth.login_password != auth.login_password
                    || provider.auth.api_user != auth.api_user
                    || portable_auth(&provider.auth).session_cookie != auth.session_cookie;
                let retain_expiring_token = !session_changed
                    && auth.access_token.is_empty()
                    && provider.auth.access_token_expires_at.is_some();
                let local_auth = provider.auth.clone();
                let previous_keys: BTreeMap<_, _> = provider
                    .auth
                    .api_key_options
                    .iter()
                    .map(|key| (key.local_id.clone(), key.clone()))
                    .collect();
                let keys = auth
                    .keys
                    .into_iter()
                    .map(|key| {
                        let mut full = previous_keys
                            .get(&key.local_id)
                            .filter(|old| old.key == key.key)
                            .cloned()
                            .unwrap_or_default();
                        full.local_id = key.local_id;
                        full.local_name = key.local_name;
                        full.name = key.name;
                        full.key = key.key;
                        full.token_id = key.token_id;
                        full.user_id = key.user_id;
                        full.normalize_for_protocol(value.identity.protocol)
                    })
                    .collect();
                provider.auth = normalize_provider_auth(
                    ProviderAuth {
                        mode: auth.mode,
                        source: auth.source,
                        api_key: auth.api_key,
                        api_key_token_id: auth.api_key_token_id,
                        api_key_options: keys,
                        access_token: if retain_expiring_token {
                            local_auth.access_token
                        } else {
                            auth.access_token
                        },
                        session_cookie: if !session_changed && auth.session_cookie.is_empty() {
                            local_auth.session_cookie
                        } else {
                            auth.session_cookie
                        },
                        api_user: auth.api_user,
                        login_username: auth.login_username,
                        login_password: auth.login_password,
                        refresh_token: if retain_expiring_token {
                            local_auth.refresh_token
                        } else {
                            String::new()
                        },
                        access_token_expires_at: if retain_expiring_token {
                            local_auth.access_token_expires_at
                        } else {
                            None
                        },
                        new_api_session: if session_changed {
                            None
                        } else {
                            local_auth.new_api_session
                        },
                        browser_binding: if session_changed {
                            None
                        } else {
                            local_auth.browser_binding
                        },
                        credential_revision: local_auth.credential_revision.saturating_add(1),
                        session_updated_at: if session_changed {
                            None
                        } else {
                            local_auth.session_updated_at
                        },
                    },
                    value.identity.protocol,
                );
                provider.capabilities = ProviderCapabilities::default();
                provider.quota.known = false;
                provider.quota.total_known = false;
                provider.quota.scope = if value.identity.protocol == ProviderProtocol::Api {
                    ProviderQuotaScope::Token
                } else {
                    ProviderQuotaScope::Account
                };
                provider.runtime.error_message = None;
                provider.runtime.status = ProviderStatus::Warning;
            }
            provider.identity.name = value.identity.name;
            provider.identity.base_url = value.identity.base_url;
            provider.identity.protocol = value.identity.protocol;
            provider.identity.remark = value.identity.remark;
            provider.identity.user_id = value.identity.user_id;
            provider.identity.backup_urls = value.identity.backup_urls;
            provider.cli.preferred_model = value.cli.preferred_model;
            provider.runtime.enabled = value.enabled;
            providers.insert(id.to_owned(), provider);
        }
    }
    let mut order: Vec<String> = documents
        .get("order/providers")
        .map(|doc| serde_json::from_value(doc.value.clone()).map_err(|_| "中转站排序数据无效"))
        .transpose()?
        .unwrap_or_default();
    let ordered: BTreeSet<_> = order.iter().cloned().collect();
    order.extend(
        providers
            .keys()
            .filter(|id| !ordered.contains(*id))
            .cloned(),
    );
    next.providers = order
        .into_iter()
        .filter_map(|id| providers.remove(&id))
        .collect();
    let mut settings = serde_json::to_value(&current.settings).map_err(|_| "本机偏好无效")?;
    let defaults = serde_json::to_value(AppSettings::default()).map_err(|_| "偏好默认值无效")?;
    for (key, _) in SETTINGS {
        settings[*key] = documents
            .get(&format!("setting/{key}"))
            .map(|doc| doc.value.clone())
            .unwrap_or_else(|| defaults[*key].clone());
    }
    for key in documents
        .keys()
        .filter_map(|key| key.strip_prefix("setting/"))
    {
        if !SETTINGS.iter().any(|(field, _)| *field == key) {
            return Err("云端包含此版本尚不支持的偏好，请先升级 BalanceHub".to_owned());
        }
    }
    next.settings = serde_json::from_value(settings).map_err(|_| "云端偏好格式无效")?;
    crate::limits::validate_app_data_limits(&next)?;
    crate::limits::normalize_app_data(&mut next);
    Ok(next)
}
