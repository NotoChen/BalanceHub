use super::{find_provider, MutationDecision, ProviderService};
use crate::{
    app_events::PROVIDERS_CHANGED_EVENT,
    models::{
        AuthMode, BrowserLoginBinding, BrowserLoginMechanism, Provider, ProviderProtocol,
        ProviderStatus,
    },
    util::unix_secs,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CredentialKind {
    DashboardJwt,
    RefreshCookie,
    SessionCookie,
    AccessToken,
    RefreshToken,
    ApiKey,
    Password,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CredentialSummary {
    pub kind: CredentialKind,
    pub label: String,
    pub source: String,
    pub expires_at: Option<i64>,
    pub status: String,
    pub clear_label: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderCredentialDetails {
    pub provider_id: String,
    pub provider_name: String,
    pub credential_revision: u64,
    pub binding: Option<BrowserLoginBinding>,
    pub account_name: Option<String>,
    pub session_id: Option<String>,
    pub updated_at: Option<i64>,
    pub verified_at: Option<String>,
    pub error: Option<String>,
    pub authentication_label: String,
    pub validation_label: String,
    pub validation_scope: String,
    pub entries: Vec<CredentialSummary>,
    pub can_login: bool,
    pub can_validate: bool,
}

impl ProviderService<'_> {
    pub(crate) fn credential_details(&self, id: &str) -> Result<ProviderCredentialDetails, String> {
        self.ensure_storage_ready()?;
        let data = self.snapshot();
        let provider = find_provider(&data, id)?;
        let definition = crate::adapters::protocol::definition(provider.identity.protocol);
        let authentication_label = definition
            .auth_schemas
            .iter()
            .find(|schema| schema.mode == provider.auth.mode)
            .map(|schema| schema.label)
            .unwrap_or("当前认证配置");
        let (validation_label, validation_scope) = validation_scope(&provider);
        let entries = credential_summaries(&provider);
        let can_validate = provider.runtime.enabled && !entries.is_empty();
        let account_name = provider
            .auth
            .browser_binding
            .as_ref()
            .and_then(|b| b.account_id.as_ref())
            .and_then(|id| data.login_accounts.iter().find(|a| &a.id == id))
            .map(|a| a.name.clone());
        Ok(ProviderCredentialDetails {
            provider_id: provider.identity.id.clone(),
            provider_name: provider.display_label(),
            credential_revision: provider.auth.credential_revision,
            binding: provider.auth.browser_binding.clone(),
            account_name,
            session_id: provider
                .auth
                .new_api_session
                .as_ref()
                .map(|s| s.session_id.clone()),
            updated_at: provider.auth.session_updated_at,
            verified_at: provider.automation.last_synced_at.clone(),
            error: provider.runtime.error_message.clone(),
            authentication_label: authentication_label.into(),
            validation_label: validation_label.into(),
            validation_scope: validation_scope.into(),
            entries,
            can_login: crate::models::provider_domain::auth::browser_login_supported(
                provider.identity.protocol,
            ),
            can_validate,
        })
    }

    pub(crate) fn read_credential(
        &self,
        id: &str,
        kind: CredentialKind,
        revision: u64,
    ) -> Result<String, String> {
        let provider = find_provider(&self.snapshot(), id)?;
        if provider.auth.credential_revision != revision {
            return Err("凭据已更改，请刷新详情".into());
        }
        credential_value(&provider, kind)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| "该凭据已不存在，请刷新详情".into())
    }

    pub(crate) async fn clear_credential(
        &self,
        id: String,
        kind: CredentialKind,
        revision: u64,
    ) -> Result<(), String> {
        // Wait for a rotating refresh to persist before removing an unrelated
        // credential; otherwise removing a PAT could strand a valid session.
        let state = self.app.state::<crate::state::AppState>();
        let _refresh = state.refresh_gate.lock().await;
        let _gate = super::credentials::AUTH_GATE.lock().await;
        self.mutate_decided_async(move |data| {
            let provider = data
                .providers
                .iter_mut()
                .find(|p| p.identity.id == id)
                .ok_or("中转站已不存在")?;
            if provider.auth.credential_revision != revision {
                return Err("凭据已更改，请刷新详情后再操作".into());
            }
            clear_credential_value(provider, kind)?;
            Ok(MutationDecision::changed(()))
        })
        .await?;
        let _ = self.app.emit(PROVIDERS_CHANGED_EVENT, ());
        Ok(())
    }

    pub(crate) async fn validate_credentials(
        &self,
        id: String,
    ) -> Result<ProviderCredentialDetails, String> {
        let provider = find_provider(&self.snapshot_async().await?, &id)?;
        if !provider.runtime.enabled {
            return Err("中转站已停用，请先启用再验证".into());
        }
        tokio::time::timeout(
            std::time::Duration::from_secs(90),
            self.refresh_by_ids(vec![id.clone()]),
        )
        .await
        .map_err(|_| "验证超时，可稍后刷新查看结果")??;
        let _ = self.app.emit(PROVIDERS_CHANGED_EVENT, ());
        self.credential_details(&id)
    }
}

fn validation_scope(provider: &Provider) -> (&'static str, &'static str) {
    if provider.identity.protocol == ProviderProtocol::Api {
        (
            "验证模型列表访问",
            "使用当前 API Key 读取模型列表，检查该接口的访问权限。",
        )
    } else if provider.auth.mode == AuthMode::ApiKey {
        (
            "验证 Key 额度查询",
            "使用当前 API Key 查询 Key 维度额度，检查该接口的访问权限。",
        )
    } else {
        (
            "验证站点同步",
            "按当前认证方式同步站点账号与额度，登录会话会按需续期。",
        )
    }
}

fn credential_value(provider: &Provider, kind: CredentialKind) -> Option<&str> {
    let auth = &provider.auth;
    Some(match kind {
        CredentialKind::DashboardJwt => &auth.new_api_session.as_ref()?.access_token,
        CredentialKind::RefreshCookie => &auth.new_api_session.as_ref()?.refresh_cookie,
        CredentialKind::SessionCookie => &auth.session_cookie,
        CredentialKind::AccessToken => &auth.access_token,
        CredentialKind::RefreshToken => &auth.refresh_token,
        CredentialKind::ApiKey => &auth.api_key,
        CredentialKind::Password => &auth.login_password,
    })
}

fn credential_summaries(provider: &Provider) -> Vec<CredentialSummary> {
    use CredentialKind::*;
    let session_source = match provider.auth.browser_binding.as_ref().map(|b| b.mechanism) {
        Some(BrowserLoginMechanism::Oauth) => "第三方登录后由中转站签发",
        Some(BrowserLoginMechanism::Password) => "站点账号密码登录",
        Some(BrowserLoginMechanism::Unknown) => "浏览器登录，方式未确认",
        None => "已有本地凭据，登录来源未记录",
    };
    [
        DashboardJwt,
        RefreshCookie,
        SessionCookie,
        AccessToken,
        RefreshToken,
        ApiKey,
        Password,
    ]
    .into_iter()
    .filter(|kind| credential_value(provider, *kind).is_some_and(|s| !s.is_empty()))
    .map(|kind| {
        let (label, source, expiry, clear) = match kind {
            DashboardJwt => (
                "站点 JWT",
                session_source,
                provider
                    .auth
                    .new_api_session
                    .as_ref()
                    .and_then(|s| s.access_expires_at),
                Some("清除本地登录会话"),
            ),
            RefreshCookie => (
                "续期 Cookie",
                session_source,
                None,
                Some("清除本地登录会话"),
            ),
            SessionCookie => (
                "站点 Cookie",
                session_source,
                None,
                Some("清除本地站点 Cookie"),
            ),
            AccessToken => (
                if provider.identity.protocol == ProviderProtocol::NewApi {
                    "访问令牌 / PAT"
                } else {
                    "访问令牌"
                },
                "已配置的访问令牌",
                provider.auth.access_token_expires_at,
                Some("清除本地访问令牌"),
            ),
            RefreshToken => ("刷新令牌", "站点登录会话", None, Some("清除本地令牌组")),
            ApiKey => (
                "API Key",
                "当前选用的 API Key；可在 API Key 管理中删除",
                None,
                None,
            ),
            Password => ("登录密码", "本地保存", None, Some("清除本地密码")),
        };
        CredentialSummary {
            kind,
            label: label.into(),
            source: source.into(),
            expires_at: expiry,
            status: if expiry.is_some_and(|t| t <= unix_secs() as i64) {
                "已到期，验证时尝试续期"
            } else if expiry.is_some() {
                "有效期内，待验证"
            } else {
                "已保存，有效期由站点决定"
            }
            .into(),
            clear_label: clear.map(str::to_owned),
        }
    })
    .collect()
}

fn clear_credential_value(provider: &mut Provider, kind: CredentialKind) -> Result<(), String> {
    match kind {
        CredentialKind::DashboardJwt | CredentialKind::RefreshCookie => {
            provider.auth.new_api_session = None
        }
        CredentialKind::SessionCookie => provider.auth.session_cookie.clear(),
        CredentialKind::AccessToken | CredentialKind::RefreshToken => {
            provider.auth.access_token.clear();
            provider.auth.refresh_token.clear();
            provider.auth.access_token_expires_at = None;
        }
        CredentialKind::Password => provider.auth.login_password.clear(),
        CredentialKind::ApiKey => return Err("请在 API Key 管理中删除对应密钥".into()),
    }
    provider.auth.credential_revision += 1;
    provider.auth.session_updated_at = Some(crate::util::unix_millis() as i64);
    provider.runtime.status = ProviderStatus::Warning;
    provider.runtime.error_message = Some("本地凭据已清除，请验证剩余凭据或重新登录".into());
    provider.automation.last_synced_at = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NewApiSession, ProviderInput};
    #[test]
    fn validation_scope_uses_the_active_mode_even_when_other_credentials_are_saved() {
        let mut provider = Provider::from_input(ProviderInput::default(), "fixture".into());
        provider.auth.session_cookie = "session=stored".into();
        provider.auth.access_token = "stored-pat".into();
        provider.auth.api_key = "sk-fixture".into();
        provider.auth.mode = AuthMode::ApiKey;
        assert_eq!(validation_scope(&provider).0, "验证 Key 额度查询");
        provider.identity.protocol = ProviderProtocol::Api;
        assert_eq!(validation_scope(&provider).0, "验证模型列表访问");
        provider.identity.protocol = ProviderProtocol::NewApi;
        provider.auth.mode = AuthMode::Session;
        assert_eq!(validation_scope(&provider).0, "验证站点同步");
    }

    #[test]
    fn removal_clears_rotating_pair_and_preserves_pat_and_key() {
        let mut provider = Provider::from_input(ProviderInput::default(), "fixture".into());
        provider.auth.new_api_session = Some(NewApiSession {
            refresh_cookie: "refresh".into(),
            session_id: "sid".into(),
            access_token: "jwt".into(),
            access_expires_at: Some(1),
        });
        provider.auth.access_token = "pat".into();
        provider.auth.api_key = "key".into();
        let before = super::super::ProviderRequestContext::capture(&provider);
        clear_credential_value(&mut provider, CredentialKind::DashboardJwt).unwrap();
        assert!(provider.auth.new_api_session.is_none());
        assert_eq!(provider.auth.access_token, "pat");
        assert_eq!(provider.auth.api_key, "key");
        assert!(!before.matches(&provider));
        let json = serde_json::to_string(&credential_summaries(&provider)).unwrap();
        assert!(!json.contains("\"pat\""));
        assert!(!json.contains("\"key\""));
    }
}
