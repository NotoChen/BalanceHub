use serde::Serialize;

use crate::models::{AuthMode, Provider, ProviderModelScope, ProviderProtocol};

use super::{auth, capabilities};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelListAction {
    pub can_sync: bool,
    pub scope: ProviderModelScope,
    pub unavailable_reason: Option<&'static str>,
}

pub fn can_use_account_models(provider: &Provider) -> bool {
    provider.identity.protocol == ProviderProtocol::NewApi
        && capabilities::supports_account_management(provider)
}

/// 计划优先使用的来源。实际来源以每次请求返回的模型快照为准。
pub fn scope(provider: &Provider) -> ProviderModelScope {
    if !auth::has_api_key(provider)
        && provider.identity.protocol == ProviderProtocol::NewApi
        && provider.auth.mode != AuthMode::ApiKey
    {
        ProviderModelScope::Account
    } else {
        ProviderModelScope::ApiKey
    }
}

pub fn action(provider: &Provider) -> ProviderModelListAction {
    let scope = scope(provider);
    let unavailable_reason = if provider.identity.base_url.trim().is_empty() {
        Some("请先填写中转站地址")
    } else {
        match scope {
            ProviderModelScope::Account if !can_use_account_models(provider) => {
                Some("请先登录账号或补全账号认证信息，再获取账号可用模型")
            }
            ProviderModelScope::ApiKey if !auth::has_api_key(provider) => {
                Some("请先填写 API Key，再获取该 Key 的可用模型")
            }
            _ => None,
        }
    };
    ProviderModelListAction {
        can_sync: unavailable_reason.is_none(),
        scope,
        unavailable_reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        ProviderCapabilities, ProviderInput, ProviderModelListState, ProviderStatus,
    };

    #[test]
    fn changing_current_key_discards_models_from_the_old_group_and_advances_request_identity() {
        let mut provider = account();
        provider
            .capabilities
            .set_available_models(crate::models::ProviderModelList::new(
                vec!["account-model".into()],
                ProviderModelScope::Account,
            ));
        let revision = provider.auth.credential_revision;
        provider.add_api_key("sk-first").unwrap();
        assert!(provider.capabilities.available_models.is_empty());
        assert!(provider.auth.credential_revision > revision);
        provider.add_api_key("sk-second").unwrap();
        provider
            .capabilities
            .set_available_models(crate::models::ProviderModelList::new(
                vec!["first-key-model".into()],
                ProviderModelScope::ApiKey,
            ));
        let second = provider
            .auth
            .api_key_options
            .iter()
            .find(|option| option.key == "sk-second")
            .unwrap()
            .local_id
            .clone();
        let revision = provider.auth.credential_revision;
        provider.set_default_api_key(&second).unwrap();
        assert!(provider.capabilities.available_models.is_empty());
        assert!(provider.auth.credential_revision > revision);
        assert!(provider
            .capabilities
            .available_models_state
            .updated_at
            .is_none());
        provider
            .capabilities
            .set_available_models(crate::models::ProviderModelList::new(
                vec!["second-key-model".into()],
                ProviderModelScope::ApiKey,
            ));
        let revision = provider.auth.credential_revision;
        provider.remove_local_api_key(&second).unwrap();
        assert_eq!(provider.auth.api_key, "sk-first");
        assert!(provider.auth.credential_revision > revision);
        assert!(provider.capabilities.available_models.is_empty());
    }

    fn account() -> Provider {
        let mut input = ProviderInput::default();
        input.identity.base_url = "https://relay.example.invalid".into();
        input.auth.mode = AuthMode::Session;
        input.auth.session_cookie = "session=fixture-session".into();
        input.auth.api_user = "42".into();
        Provider::from_input(input, "model-list-fixture".into())
    }

    #[test]
    fn new_api_account_models_do_not_require_an_api_key() {
        let mut provider = account();
        assert!(provider.auth.api_key.is_empty());
        assert!(action(&provider).can_sync);
        assert_eq!(scope(&provider), ProviderModelScope::Account);

        provider.auth.mode = AuthMode::Password;
        provider.auth.session_cookie.clear();
        provider.auth.api_user.clear();
        provider.auth.login_username = "fixture-user".into();
        provider.auth.login_password = "fixture-password".into();
        assert!(action(&provider).can_sync);

        provider.auth.login_password.clear();
        provider.auth.api_key = "sk-fixture".into();
        assert!(
            action(&provider).can_sync,
            "Key discovery must not depend on account login"
        );
        assert_eq!(scope(&provider), ProviderModelScope::ApiKey);
    }

    #[test]
    fn key_model_routes_require_a_key_for_every_protocol() {
        for protocol in [
            ProviderProtocol::NewApi,
            ProviderProtocol::Sub2Api,
            ProviderProtocol::Api,
        ] {
            let mut provider = account();
            provider.identity.protocol = protocol;
            provider.auth.mode = AuthMode::ApiKey;
            assert_eq!(scope(&provider), ProviderModelScope::ApiKey);
            assert!(!action(&provider).can_sync);
            provider.auth.api_key = "sk-fixture".into();
            assert!(action(&provider).can_sync);
            provider.identity.base_url.clear();
            assert!(!action(&provider).can_sync);
        }
        let mut provider = account();
        provider.identity.protocol = ProviderProtocol::Sub2Api;
        provider.auth.access_token = "fixture-account-token".into();
        assert_eq!(scope(&provider), ProviderModelScope::ApiKey);
        assert!(!action(&provider).can_sync);
    }

    #[test]
    fn model_state_defaults_do_not_claim_a_source_or_a_successful_sync() {
        let capabilities: ProviderCapabilities =
            serde_json::from_str(r#"{"availableModels":["cached-model"]}"#).unwrap();
        assert_eq!(capabilities.available_models, ["cached-model"]);
        assert_eq!(
            capabilities.available_models_state,
            ProviderModelListState::default()
        );
    }

    #[test]
    fn empty_success_clears_old_models_and_failure_without_changing_quota_sync_time() {
        let mut provider = account();
        provider.automation.last_synced_at = Some("123".into());
        provider
            .capabilities
            .set_available_models(crate::models::ProviderModelList::new(
                vec!["old-model".into()],
                ProviderModelScope::ApiKey,
            ));
        provider.capabilities.available_models_state.error = Some("request failed".into());
        provider
            .capabilities
            .set_available_models(crate::models::ProviderModelList::new(
                Vec::new(),
                ProviderModelScope::Account,
            ));
        assert!(provider.capabilities.available_models.is_empty());
        assert_eq!(
            provider.capabilities.available_models_state.scope,
            Some(ProviderModelScope::Account)
        );
        assert!(provider
            .capabilities
            .available_models_state
            .updated_at
            .is_some());
        assert!(provider.capabilities.available_models_state.error.is_none());
        assert_eq!(provider.automation.last_synced_at.as_deref(), Some("123"));
        provider.capabilities.clear_available_models();
        assert_eq!(
            provider.capabilities.available_models_state,
            ProviderModelListState::default()
        );
    }

    #[test]
    fn model_errors_are_visible_in_ipc_and_do_not_persist_as_quota_errors() {
        let mut provider = account();
        provider.runtime.status = ProviderStatus::Ok;
        provider.runtime.error_message = None;
        provider.capabilities.available_models_state.error = Some("HTTP 503".into());
        let failed_view = crate::contracts::ProviderView::from(provider.clone());
        assert!(matches!(
            failed_view.provider.runtime.status,
            ProviderStatus::Warning
        ));
        assert!(failed_view
            .provider
            .runtime
            .error_message
            .unwrap()
            .contains("HTTP 503"));
        assert!(matches!(provider.runtime.status, ProviderStatus::Ok));
        assert!(provider.runtime.error_message.is_none());
        provider
            .capabilities
            .set_available_models(crate::models::ProviderModelList::new(
                vec!["new-model".into()],
                ProviderModelScope::Account,
            ));
        let recovered_view = crate::contracts::ProviderView::from(provider);
        assert!(matches!(
            recovered_view.provider.runtime.status,
            ProviderStatus::Ok
        ));
        assert!(recovered_view.provider.runtime.error_message.is_none());
    }

    #[test]
    fn refresh_observation_keeps_model_source_time_and_error_together() {
        let mut original = account();
        original
            .capabilities
            .set_available_models(crate::models::ProviderModelList::new(
                vec!["old".into()],
                ProviderModelScope::ApiKey,
            ));
        let mut refreshed = original.clone();
        refreshed
            .capabilities
            .set_available_models(crate::models::ProviderModelList::new(
                vec!["new".into()],
                ProviderModelScope::Account,
            ));
        refreshed.capabilities.available_models_state.error = Some("later request failed".into());
        let outcome =
            crate::adapters::protocol::contracts::ProviderOperationOutcome::<()>::refreshed(
                &original,
                refreshed.clone(),
            );
        outcome.apply_to(&mut original);
        assert_eq!(original.capabilities.available_models, ["new"]);
        assert_eq!(
            original.capabilities.available_models_state,
            refreshed.capabilities.available_models_state
        );
    }
}
