use super::{find_provider, MutationDecision, ProviderRequestContext, ProviderService};
use crate::{
    adapters::{
        oauth::{http, tokens},
        protocol::contracts::ProviderCredentialPatch,
    },
    models::*,
    services::oauth_accounts,
};

impl ProviderService<'_> {
    pub(super) async fn prepare_oauth_account(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, Result<(), String>), String> {
        let app = self.app.clone();
        let settings = settings.clone();
        let provider = provider.clone();
        // Keep the task alive after caller cancellation until the rotated token is durable.
        tauri::async_runtime::spawn(async move {
            let lock = oauth_accounts::account_lock(&provider.identity.id);
            let _guard = lock.lock().await;
            let service = ProviderService::new(&app);
            let snapshot = service.snapshot_async().await?;
            let current = find_provider(&snapshot, &provider.identity.id)?;
            if current.auth.credential_revision != provider.auth.credential_revision
                || current.identity.protocol != ProviderProtocol::OAuth
            {
                return Err("账号授权已变更，请重新操作".into());
            }
            let identity = current.auth.oauth.as_ref().ok_or("缺少账号授权")?;
            let needs_refresh = current.auth.access_token.trim().is_empty()
                || current
                    .auth
                    .access_token_expires_at
                    .is_some_and(|at| at <= chrono::Utc::now().timestamp() + 90);
            if !needs_refresh {
                return Ok((current, Ok(())));
            }
            let owner = oauth_accounts::device_id(&app)?;
            let failure = if identity.refresh_pending {
                Some("上次授权续期未完成确认，请重新授权以避免重复使用刷新令牌".into())
            } else if let Some(error) = &identity.refresh_error {
                Some(error.clone())
            } else if identity.refresh_owner.as_deref() != Some(&owner) {
                Some("授权已到期；可重新授权，或在账号详情启用本机续期".into())
            } else if current.auth.refresh_token.is_empty() {
                Some("此授权没有刷新令牌，请重新授权".into())
            } else {
                None
            };
            if let Some(error) = failure {
                return Ok((current, Err(error)));
            }
            let context = ProviderRequestContext::capture(&current);
            let client = http::client(&settings, &current).await?;
            let mut pending = current.clone();
            pending
                .auth
                .oauth
                .as_mut()
                .expect("checked")
                .refresh_pending = true;
            let patch = ProviderCredentialPatch::from_authenticated(&current, &pending);
            let pending = service
                .persist_operation_credentials(&context, &patch)
                .await?
                .ok_or("账号已变更，未执行续期")?;
            let context = ProviderRequestContext::capture(&pending);
            let result = tokens::refresh(&client, &pending.auth).await;
            let mut next = pending.clone();
            let outcome = match result {
                Ok(auth) => {
                    next.auth = auth;
                    Ok(())
                }
                Err(error) => {
                    let identity = next.auth.oauth.as_mut().expect("checked");
                    identity.refresh_pending = false;
                    identity.refresh_error = Some(format!("{error}；续期已暂停，请重新授权"));
                    Err(identity.refresh_error.clone().unwrap_or_default())
                }
            };
            let patch = ProviderCredentialPatch::from_authenticated(&pending, &next);
            let saved = service
                .persist_operation_credentials(&context, &patch)
                .await?
                .ok_or("账号已变更，续期结果未应用")?;
            Ok((saved, outcome))
        })
        .await
        .map_err(|_| "授权续期任务异常，请重新核对账号状态".to_string())?
    }

    pub(crate) async fn set_oauth_renewal(
        &self,
        id: String,
        enabled: bool,
    ) -> Result<Provider, String> {
        let owner = oauth_accounts::device_id(self.app)?;
        let lock = oauth_accounts::account_lock(&id);
        let _guard = lock.lock().await;
        let provider_id = id.clone();
        self.mutate_decided_async(move |data| {
            let provider = data
                .providers
                .iter_mut()
                .find(|p| p.identity.id == id)
                .ok_or("账号不存在")?;
            let oauth = provider
                .auth
                .oauth
                .as_mut()
                .ok_or("此卡片不是 OAuth 账号")?;
            if enabled && (oauth.refresh_pending || oauth.refresh_error.is_some()) {
                return Err("上次续期结果未确认，请重新授权后再启用续期".into());
            }
            if enabled && provider.auth.refresh_token.is_empty() {
                return Err("此授权没有刷新令牌，请重新授权".into());
            }
            oauth.refresh_owner = enabled.then_some(owner);
            provider.auth.credential_revision = provider.auth.credential_revision.saturating_add(1);
            Ok(MutationDecision::changed(()))
        })
        .await?;
        self.providers_by_ids_async(&[provider_id])
            .await?
            .into_iter()
            .next()
            .ok_or("账号不存在".into())
    }

    pub(crate) async fn accept_oauth_login(
        &self,
        mut auth: ProviderAuth,
        target: Option<(String, u64)>,
    ) -> Result<Provider, String> {
        let owner = oauth_accounts::device_id(self.app)?;
        let oauth = auth.oauth.as_mut().ok_or("授权结果缺少账号类型")?;
        oauth.refresh_owner = Some(owner);
        oauth.source = "BalanceHub 授权".into();
        let snapshot = self.snapshot_async().await?;
        let existing = snapshot
            .providers
            .iter()
            .find(|p| oauth_accounts::interchange::same_account(&p.auth, &auth))
            .map(|p| p.identity.id.clone());
        let lock = oauth_accounts::account_lock(
            existing
                .as_deref()
                .or_else(|| target.as_ref().map(|(id, _)| id.as_str()))
                .unwrap_or("new-oauth-account"),
        );
        let _guard = lock.lock().await;
        let new_id = format!("oauth-{}", oauth_accounts::random_id()?);
        let id = self
            .mutate_decided_async(move |data| {
                Ok(MutationDecision::changed(apply_login(data, auth, new_id)?))
            })
            .await?;
        self.providers_by_ids_async(&[id])
            .await?
            .into_iter()
            .next()
            .ok_or("授权保存后未找到账号".into())
    }

    pub(crate) async fn import_oauth_accounts(
        &self,
        request: OAuthImportRequest,
    ) -> Result<Vec<Provider>, String> {
        let owner = oauth_accounts::device_id(self.app)?;
        let entries = oauth_accounts::imports::consume(&request)?;
        let mut lock_ids: Vec<_> = entries
            .iter()
            .filter_map(|entry| entry.existing.as_ref().map(|(id, _)| id.clone()))
            .collect();
        lock_ids.sort();
        lock_ids.dedup();
        let mut guards = Vec::new();
        for id in lock_ids {
            guards.push(oauth_accounts::account_lock(&id).lock_owned().await);
        }
        let ids = self
            .mutate_decided_async(move |data| {
                let mut ids = Vec::new();
                for entry in entries {
                    let mut auth = entry.auth;
                    let oauth = auth.oauth.as_mut().ok_or("授权类型缺失")?;
                    oauth.refresh_owner = (request.manage_refresh
                        && !auth.refresh_token.is_empty())
                    .then(|| owner.clone());
                    oauth.refresh_pending = false;
                    oauth.refresh_error = None;
                    let existing = data
                        .providers
                        .iter_mut()
                        .find(|p| oauth_accounts::interchange::same_account(&p.auth, &auth));
                    if let Some(existing) = existing {
                        if !request.replace_existing {
                            continue;
                        }
                        if existing
                            .auth
                            .oauth
                            .as_ref()
                            .is_some_and(|o| o.refresh_pending)
                        {
                            return Err("已有账号的续期结果尚未确认，请重新授权后再导入".into());
                        }
                        if entry.existing.as_ref().is_none_or(|(id, revision)| {
                            *id != existing.identity.id
                                || *revision != existing.auth.credential_revision
                        }) {
                            return Err("已有账号在预览后发生变化，请重新预览导入".into());
                        }
                        auth.credential_revision =
                            existing.auth.credential_revision.saturating_add(1);
                        existing.auth = auth;
                        update_identity(existing);
                        ids.push(existing.identity.id.clone());
                    } else {
                        if entry.existing.is_some() {
                            return Err("原账号已移除，请重新预览导入".into());
                        }
                        if data.providers.len() >= crate::limits::MAX_PROVIDERS {
                            return Err("中转站卡片数量已达上限".into());
                        }
                        let id = format!("oauth-{}", oauth_accounts::random_id()?);
                        data.providers.push(provider_from_auth(auth, id.clone()));
                        ids.push(id);
                    }
                }
                if ids.is_empty() {
                    Ok(MutationDecision::unchanged(ids))
                } else {
                    Ok(MutationDecision::changed(ids))
                }
            })
            .await?;
        self.providers_by_ids_async(&ids).await
    }
}

/// Applies a freshly exchanged authorization. The identity inside `auth`
/// decides which card it belongs to, so logging into another account adds its
/// own card instead of destroying the original card or the new tokens.
///
/// A save holds this account's lock, and a refresh holds the same lock for its
/// whole request, so a `refresh_pending` flag seen here is always an attempt
/// that was interrupted rather than one still running. The tokens being saved
/// come from a fresh authorization exchange and supersede whatever that attempt
/// left behind, which makes this the recovery path the panel offers after an
/// interrupted renewal — not a conflict to refuse.
fn apply_login(data: &mut AppData, auth: ProviderAuth, new_id: String) -> Result<String, String> {
    if let Some(provider) = data
        .providers
        .iter_mut()
        .find(|p| oauth_accounts::interchange::same_account(&p.auth, &auth))
    {
        let revision = provider.auth.credential_revision.saturating_add(1);
        provider.auth = auth;
        provider.auth.credential_revision = revision;
        update_identity(provider);
        return Ok(provider.identity.id.clone());
    }
    if data.providers.len() >= crate::limits::MAX_PROVIDERS {
        return Err("中转站卡片数量已达上限".into());
    }
    data.providers
        .push(provider_from_auth(auth, new_id.clone()));
    Ok(new_id)
}

fn provider_from_auth(auth: ProviderAuth, id: String) -> Provider {
    let oauth = auth.oauth.as_ref().expect("validated OAuth input");
    let mut input = ProviderInput::default();
    input.identity.protocol = ProviderProtocol::OAuth;
    input.identity.base_url = oauth.vendor.base_url().into();
    input.identity.name = oauth.vendor.label().into();
    input.auth = auth;
    input.liveness.use_global = false;
    input.automation.auto_shield = false;
    let mut provider = Provider::from_input(input, id);
    provider.quota.known = false;
    provider.quota.total_known = false;
    update_identity(&mut provider);
    provider
}
fn update_identity(provider: &mut Provider) {
    if let Some(oauth) = &provider.auth.oauth {
        provider.identity.username = oauth.email.clone();
        provider.identity.user_id = oauth.account_id.clone();
        provider.identity.display_name = oauth.email.clone();
    }
    provider.capabilities.subscription = None;
    provider.capabilities.clear_available_models();
    provider.automation.last_synced_at = None;
    provider.runtime.status = ProviderStatus::Warning;
    provider.runtime.error_message = Some("已保存授权，等待刷新账号信息".into());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authorization(subject: &str, email: &str) -> ProviderAuth {
        let mut credential = OAuthCredential::new(OAuthVendor::Claude);
        credential.subject = subject.into();
        credential.email = email.into();
        credential.source = "BalanceHub 授权".into();
        let mut auth = Provider::from_input(ProviderInput::default(), "oauth-new".into()).auth;
        auth.mode = AuthMode::AccessToken;
        auth.source = AuthSource::Oauth;
        auth.access_token = format!("access-{subject}");
        auth.refresh_token = format!("refresh-{subject}");
        auth.oauth = Some(credential);
        auth
    }

    fn stored() -> AppData {
        let mut data = AppData::default();
        let mut provider = Provider::from_input(ProviderInput::default(), "oauth-a".into());
        provider.auth = authorization("user-1", "first@example.test");
        if let Some(oauth) = provider.auth.oauth.as_mut() {
            oauth.refresh_pending = true;
            oauth.refresh_error = Some("续期已暂停，请重新授权".into());
        }
        data.providers.push(provider);
        data
    }

    /// An interrupted renewal leaves the card pending; a fresh authorization is
    /// the way out the panel offers, so saving it must replace the card and
    /// clear the state that renewal left behind.
    #[test]
    fn a_fresh_authorization_recovers_a_card_left_pending_by_an_interruption() {
        let mut data = stored();
        let id = apply_login(
            &mut data,
            authorization("user-1", "second@example.test"),
            "oauth-new".into(),
        )
        .unwrap();
        assert_eq!(id, "oauth-a");
        assert_eq!(data.providers.len(), 1);
        let provider = &data.providers[0];
        let oauth = provider.auth.oauth.as_ref().unwrap();
        assert!(!oauth.refresh_pending);
        assert_eq!(oauth.refresh_error, None);
        assert_eq!(oauth.email, "second@example.test");
        assert_eq!(provider.auth.access_token, "access-user-1");
        assert_eq!(provider.auth.credential_revision, 1);
        assert_eq!(provider.identity.username, "second@example.test");
    }

    /// Logging into another account leaves the original card and its tokens
    /// untouched and adds a card for the account that was authorized.
    #[test]
    fn authorizing_another_account_adds_a_card_instead_of_replacing_one() {
        let mut data = stored();
        let original = data.providers[0].auth.clone();
        let id = apply_login(
            &mut data,
            authorization("user-2", "other@example.test"),
            "oauth-new".into(),
        )
        .unwrap();
        assert_eq!(id, "oauth-new");
        assert_eq!(data.providers.len(), 2);
        assert_eq!(data.providers[0].auth, original);
        assert_eq!(
            data.providers[1].auth.oauth.as_ref().unwrap().subject,
            "user-2"
        );
    }
}
