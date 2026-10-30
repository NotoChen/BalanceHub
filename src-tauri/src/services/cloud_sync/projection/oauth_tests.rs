use super::*;

fn account() -> AppData {
    let mut identity = OAuthCredential::new(OAuthVendor::Codex);
    identity.account_id = "workspace-fixture".into();
    identity.subject = "subject-fixture".into();
    identity.email = "fixture@example.test".into();
    identity.id_token = "fixture-private-id-token".into();
    identity.client_secret = "fixture-private-client-secret".into();
    identity.refresh_owner = Some("fixture-local-owner".into());
    let mut input = ProviderInput::default();
    input.identity.protocol = ProviderProtocol::OAuth;
    input.identity.base_url = OAuthVendor::Codex.base_url().into();
    input.auth.oauth = Some(identity);
    input.auth.access_token = "fixture-private-access-token".into();
    input.auth.refresh_token = "fixture-private-refresh-token".into();
    input.auth.access_token_expires_at = Some(1900000000);
    input.auth.mode = AuthMode::AccessToken;
    input.auth.source = AuthSource::Oauth;
    AppData::new_current(
        vec![Provider::from_input(input, "oauth-fixture".into())],
        AppSettings::default(),
    )
}

#[test]
fn sync_transfers_identity_without_installing_a_second_oauth_session() {
    let current = account();
    let documents = app_documents(&current).unwrap();
    let json = serde_json::to_string(&documents).unwrap();
    for excluded in [
        "fixture-private-",
        "idToken",
        "clientSecret",
        "refreshOwner",
        "refreshPending",
    ] {
        assert!(!json.contains(excluded), "{excluded}");
    }
    let imported = apply_app_documents(&AppData::default(), &documents).unwrap();
    let auth = &imported.providers[0].auth;
    assert!(auth.access_token.is_empty());
    assert!(auth.refresh_token.is_empty());
    assert!(auth.access_token_expires_at.is_none());
    assert_eq!(auth.oauth.as_ref().unwrap().email, "fixture@example.test");
    assert!(auth.oauth.as_ref().unwrap().refresh_owner.is_none());
    assert_eq!(app_documents(&imported).unwrap(), documents);
}

#[test]
fn same_identity_keeps_local_tokens_when_remote_metadata_changes() {
    let current = account();
    let mut remote = current.clone();
    remote.providers[0].auth.oauth.as_mut().unwrap().plan = "pro".into();
    let imported = apply_app_documents(&current, &app_documents(&remote).unwrap()).unwrap();
    let auth = &imported.providers[0].auth;
    assert_eq!(auth.access_token, current.providers[0].auth.access_token);
    assert_eq!(auth.refresh_token, current.providers[0].auth.refresh_token);
    assert_eq!(
        auth.oauth.as_ref().unwrap().id_token,
        "fixture-private-id-token"
    );
    assert_eq!(
        auth.oauth.as_ref().unwrap().refresh_owner.as_deref(),
        Some("fixture-local-owner")
    );
    assert_eq!(
        app_documents(&imported).unwrap(),
        app_documents(&remote).unwrap()
    );
}

#[test]
fn changed_identity_never_inherits_the_previous_oauth_tokens() {
    for field in [
        "accountId",
        "subject",
        "email",
        "organizationId",
        "projectId",
    ] {
        let current = account();
        let mut documents = app_documents(&current).unwrap();
        documents.get_mut("provider/oauth-fixture").unwrap().value["auth"]["oauth"][field] =
            json!("different-fixture");
        let imported = apply_app_documents(&current, &documents).unwrap();
        let auth = &imported.providers[0].auth;
        assert!(auth.access_token.is_empty(), "{field}");
        assert!(auth.refresh_token.is_empty(), "{field}");
        assert!(auth.access_token_expires_at.is_none(), "{field}");
        assert!(auth.oauth.as_ref().unwrap().id_token.is_empty(), "{field}");
        assert!(
            auth.oauth.as_ref().unwrap().refresh_owner.is_none(),
            "{field}"
        );
    }
}

#[test]
fn sync_rejects_oauth_secrets_mismatched_types_and_nonofficial_hosts() {
    let current = account();
    for field in ["idToken", "clientSecret", "refreshOwner"] {
        let mut documents = app_documents(&current).unwrap();
        documents.get_mut("provider/oauth-fixture").unwrap().value["auth"]["oauth"][field] =
            json!("fixture-private-injected");
        assert!(
            apply_app_documents(&current, &documents).is_err(),
            "{field}"
        );
    }
    for (path, value) in [
        (["auth", "accessToken"], json!("fixture-private-injected")),
        (["auth", "oauth"], serde_json::Value::Null),
        (["identity", "protocol"], json!("newApi")),
        (["identity", "baseUrl"], json!("https://other.example.test")),
    ] {
        let mut documents = app_documents(&current).unwrap();
        let document = &mut documents.get_mut("provider/oauth-fixture").unwrap().value;
        document[path[0]][path[1]] = value;
        assert!(
            apply_app_documents(&current, &documents).is_err(),
            "{path:?}"
        );
    }
}
