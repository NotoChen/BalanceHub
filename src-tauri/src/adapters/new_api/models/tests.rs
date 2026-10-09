use crate::adapters::test_support::{serve, Reply};
use crate::{
    adapters::{new_api::NewApiAdapter, protocol::ProtocolAdapter},
    models::{
        AppSettings, AuthMode, NewApiSession, Provider, ProviderModelScope, ProviderProtocol,
        ProviderStatus,
    },
};

async fn fetch(provider: &Provider) -> Result<Vec<String>, String> {
    ProtocolAdapter
        .fetch_available_models(&AppSettings::default(), provider)
        .await
        .map(|operation| operation.value.models)
}

#[tokio::test]
async fn account_auth_always_prefers_the_current_keys_gateway_scope() {
    for mode in [AuthMode::Session, AuthMode::Password, AuthMode::AccessToken] {
        let (mut provider, requests, server) = serve(vec![Reply(
            200,
            r#"{"data":[{"id":"restricted-key-model"}]}"#,
        )]);
        provider.auth.mode = mode;
        provider.auth.login_password.clear();
        provider.auth.session_cookie.clear();
        let result = ProtocolAdapter
            .fetch_available_models(&AppSettings::default(), &provider)
            .await
            .unwrap()
            .value;
        server.join().unwrap();
        assert_eq!(result.scope, ProviderModelScope::ApiKey);
        assert_eq!(result.models, ["restricted-key-model"]);
        assert!(result.fallback_reason.is_none());
        let request = requests.recv().unwrap().to_lowercase();
        assert!(request.starts_with("get /relay/v1/models "));
        assert!(!request.contains("cookie:"));
        assert!(!request.contains("new-api-user:"));
    }
}

#[tokio::test]
async fn key_failure_falls_back_to_account_models_with_the_actual_source_and_reason() {
    let (provider, requests, server) = serve(vec![
        Reply(401, r#"{"type":"unauthorized_client_error"}"#),
        Reply(
            200,
            r#"{"success":true,"data":["group-a-model","group-b-model"]}"#,
        ),
    ]);
    let result = ProtocolAdapter
        .fetch_available_models(&AppSettings::default(), &provider)
        .await
        .unwrap()
        .value;
    server.join().unwrap();
    assert_eq!(result.scope, ProviderModelScope::Account);
    assert_eq!(result.models.len(), 2);
    assert!(result.fallback_reason.unwrap().contains("401"));
    let requests = requests.iter().collect::<Vec<_>>();
    assert!(requests[0].starts_with("GET /relay/v1/models "));
    assert!(requests[1].starts_with("GET /relay/api/user/models "));
    assert!(!requests[1].contains("sk-fixture-model-key"));
}

#[tokio::test]
async fn empty_key_models_never_expand_to_the_account_union() {
    let (provider, requests, server) = serve(vec![Reply(200, r#"{"data":[]}"#)]);
    let result = ProtocolAdapter
        .fetch_available_models(&AppSettings::default(), &provider)
        .await
        .unwrap()
        .value;
    server.join().unwrap();
    assert!(result.models.is_empty());
    assert_eq!(result.scope, ProviderModelScope::ApiKey);
    assert_eq!(requests.iter().count(), 1);
}

#[tokio::test]
async fn successful_key_lookup_never_runs_account_authentication() {
    let (provider, _, server) = serve(vec![Reply(200, r#"{"data":[]}"#)]);
    let client = crate::adapters::transport::build_client(&AppSettings::default(), &provider)
        .await
        .unwrap();
    let result = super::fetch_models_with_account_auth(&client, &provider, |_| async {
        panic!("a successful Key response must not authenticate the account")
    })
    .await
    .unwrap();
    server.join().unwrap();
    assert_eq!(result.value.scope, ProviderModelScope::ApiKey);
}

#[tokio::test]
async fn fallback_prepares_credentials_before_a_model_failure_without_repeating_the_key_request() {
    let (provider, requests, server) = serve(vec![
        Reply(401, r#"{"error":{"message":"key rejected"}}"#),
        Reply(503, r#"{"success":false,"message":"models unavailable"}"#),
    ]);
    let client = crate::adapters::transport::build_client(&AppSettings::default(), &provider)
        .await
        .unwrap();
    let mut prepared_token = None;
    let prepared = &mut prepared_token;
    let error =
        super::fetch_models_with_account_auth(&client, &provider, |mut candidate| async move {
            candidate.auth.mode = AuthMode::AccessToken;
            candidate.auth.access_token = "fixture-prepared-token".into();
            *prepared = Some(candidate.auth.access_token.clone());
            Ok(candidate)
        })
        .await
        .unwrap_err();
    server.join().unwrap();
    assert_eq!(prepared_token.as_deref(), Some("fixture-prepared-token"));
    assert!(error.contains("503"));
    let requests = requests.iter().collect::<Vec<_>>();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("GET /relay/v1/models "));
    assert!(requests[1]
        .to_lowercase()
        .contains("authorization: bearer fixture-prepared-token"));
}

#[tokio::test]
async fn failed_key_and_account_endpoints_both_appear_in_the_error() {
    let (provider, _, server) = serve(vec![
        Reply(401, r#"{"error":{"message":"key rejected"}}"#),
        Reply(503, r#"{"success":false,"message":"account unavailable"}"#),
    ]);
    let error = fetch(&provider).await.unwrap_err();
    server.join().unwrap();
    assert!(error.contains("401"));
    assert!(error.contains("503"));
}

#[tokio::test]
async fn session_and_cached_password_can_use_account_models_without_a_key() {
    for mode in [AuthMode::Session, AuthMode::Password] {
        let (mut provider, requests, server) = serve(vec![Reply(
            200,
            r#"{"success":true,"data":["model-b"," model-a ","model-b"]}"#,
        )]);
        provider.auth.mode = mode;
        provider.auth.api_key.clear();
        assert_eq!(fetch(&provider).await.unwrap(), ["model-a", "model-b"]);
        server.join().unwrap();
        let request = requests.recv().unwrap().to_lowercase();
        assert!(request.starts_with("get /relay/api/user/models "));
        assert!(request.contains("cookie: session=fixture-session"));
        assert!(request.contains("new-api-user: 42"));
        assert!(!request.contains("authorization:"));
        assert!(!request.contains("sk-fixture-model-key"));
    }
}

#[tokio::test]
async fn account_models_load_with_no_model_key() {
    let (mut provider, requests, server) = serve(vec![Reply(
        200,
        r#"{"success":true,"data":["account-model"]}"#,
    )]);
    provider.auth.api_key.clear();
    assert_eq!(fetch(&provider).await.unwrap(), ["account-model"]);
    server.join().unwrap();
    assert!(requests
        .recv()
        .unwrap()
        .starts_with("GET /relay/api/user/models "));
}

#[tokio::test]
async fn access_tokens_and_modern_sessions_use_their_account_credential() {
    for modern in [false, true] {
        let (mut provider, requests, server) = serve(vec![Reply(
            200,
            r#"{"success":true,"data":["account-model"]}"#,
        )]);
        provider.auth.api_key.clear();
        if modern {
            provider.auth.new_api_session = Some(NewApiSession {
                access_token: "fixture-account-token".into(),
                access_expires_at: Some(4102444800),
                refresh_cookie: "fixture-refresh".into(),
                session_id: "fixture-session-id".into(),
            });
        } else {
            provider.auth.mode = AuthMode::AccessToken;
            provider.auth.access_token = "fixture-account-token".into();
            provider.auth.api_key.clear();
        }
        assert_eq!(fetch(&provider).await.unwrap(), ["account-model"]);
        server.join().unwrap();
        let request = requests.recv().unwrap().to_lowercase();
        assert!(request.starts_with("get /relay/api/user/models "));
        assert!(request.contains("authorization: bearer fixture-account-token"));
        assert!(!request.contains("cookie:"));
        assert!(!request.contains("sk-fixture-model-key"));
    }
}

#[tokio::test]
async fn every_key_only_protocol_uses_the_gateway_without_account_credentials() {
    for protocol in [
        ProviderProtocol::NewApi,
        ProviderProtocol::Sub2Api,
        ProviderProtocol::Api,
    ] {
        let (mut provider, requests, server) =
            serve(vec![Reply(200, r#"{"data":[{"id":"key-model"}]}"#)]);
        provider.identity.protocol = protocol;
        provider.auth.mode = AuthMode::ApiKey;
        assert_eq!(fetch(&provider).await.unwrap(), ["key-model"]);
        server.join().unwrap();
        let request = requests.recv().unwrap().to_lowercase();
        assert!(request.starts_with("get /relay/v1/models "));
        assert!(request.contains("authorization: bearer sk-fixture-model-key"));
        assert!(!request.contains("cookie:"));
        assert!(!request.contains("new-api-user:"));
    }
}

#[tokio::test]
async fn rejected_session_retries_once_with_the_existing_account_access_token() {
    let (mut provider, requests, server) = serve(vec![
        Reply(401, r#"{"success":false,"message":"not logged in"}"#),
        Reply(200, r#"{"success":true,"data":["account-model"]}"#),
    ]);
    provider.auth.access_token = "fixture-account-token".into();
    provider.auth.api_key.clear();
    assert_eq!(fetch(&provider).await.unwrap(), ["account-model"]);
    server.join().unwrap();
    let requests = requests
        .iter()
        .map(|request| request.to_lowercase())
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|request| request.starts_with("get /relay/api/user/models ")));
    assert!(requests[0].contains("cookie: session=fixture-session"));
    assert!(requests[1].contains("authorization: bearer fixture-account-token"));
    assert!(requests
        .iter()
        .all(|request| !request.contains("sk-fixture-model-key")));
}

#[tokio::test]
async fn account_failure_without_a_key_is_reported() {
    for code in [401, 404] {
        let (mut provider, requests, server) = serve(vec![Reply(
            code,
            r#"{"success":false,"message":"account models unavailable"}"#,
        )]);
        provider.auth.api_key.clear();
        let error = fetch(&provider).await.unwrap_err();
        assert!(error.contains(&format!("HTTP {code}")));
        server.join().unwrap();
        let requests = requests.iter().collect::<Vec<_>>();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /relay/api/user/models "));
    }
}

#[tokio::test]
async fn client_rejection_is_reported_without_a_raw_json_blob() {
    let (mut provider, _requests, server) = serve(vec![Reply(
        401,
        r#"{"success":false,"type":"unauthorized_client_error","error":{"message":"unauthorized client detected"}}"#,
    )]);
    provider.auth.mode = AuthMode::ApiKey;
    assert_eq!(
        fetch(&provider).await.unwrap_err(),
        "站点拒绝此客户端访问模型接口（HTTP 401）"
    );
    server.join().unwrap();
}

const SITE: &str = r#"{"success":true,"data":{"system_name":"Fixture","quota_per_unit":500000}}"#;
const QUOTA: &str = r#"{"success":true,"data":{"id":42,"quota":50000000,"used_quota":500000}}"#;

#[tokio::test]
async fn card_refresh_retains_cached_models_and_reports_an_account_model_failure() {
    let (mut provider, requests, server) = serve(vec![
        Reply(200, SITE),
        Reply(200, QUOTA),
        Reply(
            503,
            r#"{"success":false,"message":"model service unavailable"}"#,
        ),
    ]);
    provider
        .capabilities
        .set_available_models(crate::models::ProviderModelList::new(
            vec!["cached-model".into()],
            ProviderModelScope::Account,
        ));
    provider.auth.api_key.clear();
    let prior = provider.capabilities.available_models_state.clone();
    let refreshed = NewApiAdapter
        .refresh_provider(&AppSettings::default(), &provider)
        .await;
    server.join().unwrap();
    assert_eq!(refreshed.capabilities.available_models, ["cached-model"]);
    assert_eq!(
        refreshed.capabilities.available_models_state.updated_at,
        prior.updated_at
    );
    assert!(refreshed
        .capabilities
        .available_models_state
        .error
        .as_ref()
        .unwrap()
        .contains("HTTP 503"));
    assert!(
        matches!(refreshed.runtime.status, ProviderStatus::Ok),
        "quota remains valid"
    );
    assert!(matches!(
        crate::contracts::ProviderView::from(refreshed)
            .provider
            .runtime
            .status,
        ProviderStatus::Warning
    ));
    let requests = requests.iter().collect::<Vec<_>>();
    assert!(requests[2].starts_with("GET /relay/api/user/models "));
}

#[tokio::test]
async fn card_refresh_records_a_successful_empty_account_list() {
    let (mut provider, _requests, server) = serve(vec![
        Reply(200, SITE),
        Reply(200, QUOTA),
        Reply(200, r#"{"success":true,"data":[]}"#),
    ]);
    provider
        .capabilities
        .set_available_models(crate::models::ProviderModelList::new(
            vec!["cached-model".into()],
            ProviderModelScope::ApiKey,
        ));
    provider.capabilities.available_models_state.error = Some("old failure".into());
    provider.auth.api_key.clear();
    let refreshed = NewApiAdapter
        .refresh_provider(&AppSettings::default(), &provider)
        .await;
    server.join().unwrap();
    assert!(refreshed.capabilities.available_models.is_empty());
    assert_eq!(
        refreshed.capabilities.available_models_state.scope,
        Some(ProviderModelScope::Account)
    );
    assert!(refreshed
        .capabilities
        .available_models_state
        .updated_at
        .is_some());
    assert!(refreshed
        .capabilities
        .available_models_state
        .error
        .is_none());
}
