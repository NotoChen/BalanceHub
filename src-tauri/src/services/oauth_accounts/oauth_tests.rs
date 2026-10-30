use super::{imports, interchange, login_callback};
use crate::{adapters::oauth::tokens, contracts::ProviderView, models::*};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};

fn jwt(value: Value) -> String {
    format!("e30.{}.fixture", URL_SAFE_NO_PAD.encode(value.to_string()))
}

fn account(vendor: OAuthVendor) -> Provider {
    let mut input = ProviderInput::default();
    input.identity.protocol = ProviderProtocol::OAuth;
    input.identity.name = "测试账号".into();
    input.identity.base_url = vendor.base_url().into();
    input.auth.mode = AuthMode::AccessToken;
    input.auth.source = AuthSource::Oauth;
    input.auth.access_token = "fixture-access".into();
    input.auth.refresh_token = "fixture-refresh".into();
    input.auth.access_token_expires_at = Some(1_900_000_000);
    let mut identity = OAuthCredential::new(vendor);
    identity.email = "person@example.test".into();
    identity.subject = "user-fixture".into();
    identity.account_id = if vendor == OAuthVendor::Codex {
        "workspace-fixture".into()
    } else {
        String::new()
    };
    identity.id_token = jwt(
        json!({"sub":"user-fixture", "email":"person@example.test", "https://api.openai.com/auth":{"chatgpt_account_id":"workspace-fixture"}}),
    );
    if vendor != OAuthVendor::Codex {
        identity.id_token = jwt(json!({"sub":"user-fixture","email":"person@example.test"}));
    }
    identity.client_id = tokens::client_id(vendor).into();
    identity.scopes = tokens::scopes(vendor)
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    input.auth.oauth = Some(identity);
    Provider::from_input(input, "oauth-test".into())
}

fn parse(value: Value, hint: Option<OAuthVendor>) -> Result<Vec<ProviderAuth>, String> {
    interchange::parse_file(
        &OAuthImportFile {
            name: "fixture.json".into(),
            content: value.to_string(),
        },
        hint,
    )?
    .into_iter()
    .collect()
}

#[test]
fn supported_exports_round_trip_without_confusing_local_and_upstream_ids() {
    for vendor in [
        OAuthVendor::Codex,
        OAuthVendor::Claude,
        OAuthVendor::Gemini,
        OAuthVendor::Grok,
    ] {
        let source = account(vendor);
        for option in interchange::options(&source.auth)
            .into_iter()
            .filter(|o| o.available)
        {
            let file = interchange::export(&source, option.format).unwrap();
            assert_eq!(file.name, option.file_name);
            let imported = interchange::parse_file(
                &OAuthImportFile {
                    name: file.name,
                    content: file.content,
                },
                None,
            )
            .unwrap();
            assert_eq!(imported.len(), 1);
            let auth = imported.into_iter().next().unwrap().unwrap();
            let identity = auth.oauth.as_ref().unwrap();
            assert_eq!(identity.vendor, vendor, "{:?}", option.format);
            assert_eq!(auth.refresh_token, source.auth.refresh_token);
            assert_eq!(
                identity.account_id,
                source.auth.oauth.as_ref().unwrap().account_id,
                "{:?}",
                option.format
            );
            assert!(
                interchange::same_account(&source.auth, &auth),
                "{vendor:?} {:?}",
                option.format
            );
        }
    }
}

#[test]
fn ccswitch_codex_local_identifier_is_not_a_chatgpt_workspace() {
    let auth = parse(json!({"version":2, "accounts":{"local-key":{"account_id":"local-key","chatgpt_account_id":"remote-workspace","refresh_token":"fixture-refresh","authenticated_at":1800000000,"email":"person@example.test"}}}), Some(OAuthVendor::Codex)).unwrap().remove(0);
    assert_eq!(auth.oauth.unwrap().account_id, "remote-workspace");
}

#[test]
fn sub2api_uses_provider_credential_fields_and_accepts_expiration_formats() {
    for expiration in [
        json!(1900000000),
        json!("1900000000"),
        json!(1900000000000i64),
        json!("2030-03-17T17:46:40Z"),
    ] {
        let auth = parse(json!({"type":"sub2api-data", "version":1,"accounts":[{"platform":"anthropic","type":"oauth","credentials":{"access_token":"fixture-access","refresh_token":"fixture-refresh","org_uuid":"org-fixture","account_uuid":"user-fixture","expires_at":expiration}}]}), None).unwrap().remove(0);
        assert_eq!(auth.access_token_expires_at, Some(1900000000));
        let identity = auth.oauth.unwrap();
        assert_eq!(identity.organization_id, "org-fixture");
        assert_eq!(identity.subject, "user-fixture");
    }
}

#[test]
fn sub2api_extra_identity_is_retained_without_overriding_credentials() {
    for include_identity in [false, true] {
        let mut credentials =
            json!({"access_token":"fixture-access","refresh_token":"fixture-refresh"});
        if include_identity {
            credentials["email_address"] = "credential@example.test".into();
            credentials["account_uuid"] = "credential-user".into();
            credentials["org_uuid"] = "credential-org".into();
        }
        let auth = parse(json!({
            "platform":"anthropic", "type":"oauth", "credentials": credentials,
            "extra": {"email_address":"extra@example.test", "account_uuid":"extra-user", "org_uuid":"extra-org"}
        }), None).unwrap().remove(0);
        let identity = auth.oauth.unwrap();
        let expected = if include_identity {
            "credential"
        } else {
            "extra"
        };
        assert_eq!(identity.email, format!("{expected}@example.test"));
        assert_eq!(identity.subject, format!("{expected}-user"));
        assert_eq!(identity.organization_id, format!("{expected}-org"));
    }
}

#[test]
fn imported_jwt_expiration_is_a_bounded_seconds_timestamp() {
    for expiration in [i64::MIN, i64::MAX, 1_900_000_000] {
        let auth = parse(
            json!({
                "type":"codex", "access_token": jwt(json!({"exp": expiration}))
            }),
            None,
        )
        .unwrap()
        .remove(0);
        assert_eq!(
            auth.access_token_expires_at,
            (expiration == 1_900_000_000).then_some(expiration)
        );
    }
}

#[test]
fn native_grok_reads_all_issuer_entries() {
    let values = parse(
        json!({
            "https://auth.x.ai::client-a":{"key":"fixture-a","refresh_token":"refresh-a"},
            "https://auth.x.ai::client-b":{"key":"fixture-b","refresh_token":"refresh-b"}
        }),
        None,
    )
    .unwrap();
    assert_eq!(values.len(), 2);
    assert!(values
        .iter()
        .any(|a| a.oauth.as_ref().unwrap().client_id == "client-b"));
}

#[test]
fn unrelated_auth_protocols_and_masked_values_cannot_be_relabelled() {
    for value in [
        json!({"platform":"grok","type":"sso","access_token":"fixture-sso"}),
        json!({"type":"antigravity","access_token":"fixture-access"}),
        json!({"platform":"gemini","type":"oauth","credentials":{"access_token":"fixture-access","oauth_type":"ai_studio"}}),
        json!({"format":"balancehub-oauth","version":9,"accounts":[]}),
        json!({"type":"codex","access_token":"sk-*****"}),
        json!({"type":"codex","access_token":"contains whitespace"}),
    ] {
        assert!(parse(value, Some(OAuthVendor::Gemini)).is_err());
    }
    assert!(parse(
        json!({"tokens":{"access_token":"fixture-access"}}),
        Some(OAuthVendor::Claude)
    )
    .is_err());
}

#[test]
fn matching_tokens_do_not_merge_conflicting_workspaces_or_subjects() {
    let source = account(OAuthVendor::Codex);
    for field in ["account", "organization", "project", "subject"] {
        let mut left = source.auth.clone();
        let mut right = source.auth.clone();
        let a = left.oauth.as_mut().unwrap();
        let b = right.oauth.as_mut().unwrap();
        match field {
            "account" => {
                a.account_id = "a".into();
                b.account_id = "b".into();
            }
            "organization" => {
                a.organization_id = "a".into();
                b.organization_id = "b".into();
            }
            "project" => {
                a.project_id = "a".into();
                b.project_id = "b".into();
            }
            _ => {
                a.subject = "a".into();
                b.subject = "b".into();
            }
        }
        assert!(!interchange::same_account(&left, &right), "{field}");
    }
}

#[test]
fn export_readiness_matches_the_actual_serializer() {
    let mut provider = account(OAuthVendor::Codex);
    provider.auth.oauth.as_mut().unwrap().id_token.clear();
    for option in interchange::options(&provider.auth) {
        assert_eq!(
            option.available,
            interchange::export(&provider, option.format).is_ok()
        );
    }
    provider.auth.access_token.clear();
    provider.auth.refresh_token.clear();
    assert!(interchange::options(&provider.auth)
        .iter()
        .all(|o| !o.available));
    let mut custom = account(OAuthVendor::Gemini);
    custom.auth.oauth.as_mut().unwrap().client_id = "different-public-client".into();
    let enabled: Vec<_> = interchange::options(&custom.auth)
        .into_iter()
        .filter(|o| o.available)
        .map(|o| o.format)
        .collect();
    assert_eq!(enabled, vec![OAuthExportFormat::BalanceHub]);
}

#[test]
fn magpie_claude_export_preserves_org_identity_in_its_supported_import_shape() {
    let mut provider = account(OAuthVendor::Claude);
    provider.auth.oauth.as_mut().unwrap().organization_id = "fixture-org".into();
    let file = interchange::export(&provider, OAuthExportFormat::Magpie).unwrap();
    let output: serde_json::Value = serde_json::from_str(&file.content).unwrap();
    assert_eq!(output["platform"], "anthropic");
    assert_eq!(output["type"], "oauth");
    assert_eq!(output["credentials"]["org_uuid"], "fixture-org");
    assert_eq!(
        output["credentials"]["account_uuid"],
        provider.auth.oauth.as_ref().unwrap().subject
    );
    assert_eq!(
        output["credentials"]["scope"],
        provider.auth.oauth.as_ref().unwrap().scopes.join(" ")
    );
    provider.auth.oauth.as_mut().unwrap().email.clear();
    assert!(interchange::export(&provider, OAuthExportFormat::Magpie)
        .unwrap_err()
        .contains("邮箱"));
}

#[test]
fn public_views_and_import_previews_never_include_raw_oauth_credentials() {
    let mut provider = account(OAuthVendor::Codex);
    provider.auth.oauth.as_mut().unwrap().client_secret = "fixture-client-secret".into();
    provider.auth.oauth.as_mut().unwrap().refresh_owner = Some("fixture-owner".into());
    let view = serde_json::to_string(&ProviderView::from(provider.clone())).unwrap();
    for secret in [
        "fixture-access",
        "fixture-refresh",
        "fixture-client-secret",
        "fixture-owner",
    ] {
        assert!(!view.contains(secret));
    }
    let file = interchange::export(&provider, OAuthExportFormat::BalanceHub).unwrap();
    let preview = imports::prepare(
        vec![OAuthImportFile {
            name: file.name,
            content: file.content,
        }],
        None,
        &[provider],
    )
    .unwrap();
    let text = serde_json::to_string(&preview).unwrap();
    assert!(!text.contains("fixture-refresh"));
    assert!(preview.entries[0].duplicate_id.is_some());
    let request = OAuthImportRequest {
        token: preview.token,
        entry_ids: vec![preview.entries[0].id.clone()],
        replace_existing: true,
        manage_refresh: false,
    };
    assert_eq!(imports::consume(&request).unwrap().len(), 1);
    assert!(
        imports::consume(&request).is_err(),
        "a preview is single-use"
    );
}

#[test]
fn loopback_redirect_must_match_the_registered_callback() {
    // 授权服务器按 client 登记的地址校验跳转，端口和路径都不能自己换。
    // Codex 登记了两个端口，但一次授权只用其中一个，另一个也不能认成同一次回调。
    for vendor in [OAuthVendor::Codex, OAuthVendor::Claude] {
        let (path, ports) = login_callback::loopback_spec(vendor).unwrap();
        let port = ports.preferred();
        let redirect = login_callback::loopback_redirect(path, port);
        let parsed = reqwest::Url::parse(&redirect).unwrap();
        assert_eq!(parsed.port(), Some(port));
        assert_eq!(parsed.path(), path);
        let callback =
            |url: &str, redirect: &str| login_callback::parse_code(url, vendor, "s", redirect);
        let valid = format!("http://localhost:{port}{path}?code=c&state=s");
        assert_eq!(callback(&valid, &redirect).unwrap(), "c");
        // 端口不同即视为不属于本次授权，粘贴时不能放行。
        let moved = login_callback::loopback_redirect(path, port + 1);
        assert!(callback(
            &valid.replace(&port.to_string(), &(port + 1).to_string()),
            &redirect
        )
        .is_err());
        assert!(callback(&valid, &moved).is_err());
    }
    assert_eq!(login_callback::loopback_spec(OAuthVendor::Gemini), None);
    assert_eq!(login_callback::loopback_spec(OAuthVendor::Grok), None);
}

#[test]
fn codex_only_offers_ports_the_authorization_server_registered() {
    // 实测 auth.openai.com 只接受 1455 与 1457；1456、43219 都会返回
    // invalid_authorize_request。这份清单与上游注册一一对应，不能随手加端口。
    let Some((_, login_callback::CallbackPorts::Registered(ports))) =
        login_callback::loopback_spec(OAuthVendor::Codex)
    else {
        panic!("Codex 必须使用登记端口");
    };
    assert_eq!(ports, [1455u16, 1457].as_slice());
}

#[tokio::test]
async fn codex_never_moves_beyond_the_registered_ports() {
    let (path, ports) = login_callback::loopback_spec(OAuthVendor::Codex).unwrap();
    let login_callback::CallbackPorts::Registered(registered) = ports else {
        panic!("Codex 必须使用登记端口");
    };
    if let Some((listener, redirect)) = super::login::bind_loopback(path, ports).await {
        let url = reqwest::Url::parse(&redirect).unwrap();
        assert_eq!(url.path(), path);
        assert!(
            registered.contains(&url.port().unwrap()),
            "不能改用未登记的端口：{redirect}"
        );
        drop(listener);
    }
    // 登记端口在两族地址上都被占用时，只能退回粘贴，不能换端口。
    let mut holders = Vec::new();
    for port in registered {
        for address in [
            std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, *port)),
            std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, *port)),
        ] {
            if let Ok(holder) = std::net::TcpListener::bind(address) {
                holders.push(holder);
            }
        }
    }
    if holders.len() == registered.len() * 2 {
        assert!(
            super::login::bind_loopback(path, ports).await.is_none(),
            "登记端口全被占用时不能改用临时端口"
        );
    }
}

#[tokio::test]
async fn claude_moves_to_another_port_when_the_preferred_one_is_busy() {
    // 用系统刚分配并释放的端口模拟"首选端口被占用"，不依赖宿主机已有服务；
    // 两族地址都占住，因为只要有一族能绑上就会继续用首选端口。
    let probe = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let preferred = probe.local_addr().unwrap().port();
    let Ok(peer) = std::net::TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, preferred)) else {
        return;
    };
    let ports = login_callback::CallbackPorts::Flexible(preferred);

    let (listener, redirect) = super::login::bind_loopback("/callback", ports)
        .await
        .expect("Claude 接受任意 loopback 端口，不应退回粘贴");
    let url = reqwest::Url::parse(&redirect).unwrap();
    assert_eq!(url.path(), "/callback");
    assert_ne!(url.port(), Some(preferred), "首选端口被占用时必须换端口");
    // 换到的端口仍要能和粘贴回来的回调地址对上，回调与粘贴走同一条校验。
    let pasted = format!(
        "http://localhost:{}{}?code=c&state=s",
        url.port().unwrap(),
        url.path()
    );
    assert_eq!(
        login_callback::parse_code(&pasted, OAuthVendor::Claude, "s", &redirect).unwrap(),
        "c"
    );
    drop((listener, probe, peer));
}

#[test]
fn callback_requires_matching_state_location_and_single_values() {
    let state = "fixture-state";
    let redirect = login_callback::loopback_redirect("/auth/callback", 1455);
    let valid = "http://localhost:1455/auth/callback?code=fixture-code&state=fixture-state";
    let parse =
        |value: &str| login_callback::parse_code(value, OAuthVendor::Codex, state, &redirect);
    assert_eq!(parse(valid).unwrap(), "fixture-code");
    for invalid in [
        valid.replace("fixture-state", "other-state"),
        valid.replace("1455", "1555"),
        valid.replace("localhost", "remote.example"),
        valid.replace("/auth/callback", "/another"),
        format!("{valid}&code=duplicate"),
        format!("{valid}&state=duplicate"),
        format!("{valid}#fragment"),
    ] {
        assert!(parse(&invalid).is_err());
    }
    let claude = login_callback::loopback_redirect("/callback", 54545);
    assert_eq!(
        login_callback::parse_code("manual#fixture-state", OAuthVendor::Claude, state, &claude)
            .unwrap(),
        "manual"
    );
    assert!(
        login_callback::parse_code("manual#other-state", OAuthVendor::Claude, state, &claude)
            .is_err()
    );
    let gemini = login_callback::manual_redirect(OAuthVendor::Gemini);
    assert_eq!(
        login_callback::parse_code("google-manual-code", OAuthVendor::Gemini, state, &gemini)
            .unwrap(),
        "google-manual-code"
    );
}

#[test]
fn authorization_uses_pkce_s256_and_the_same_redirect_as_token_exchange() {
    use sha2::{Digest, Sha256};
    for vendor in [OAuthVendor::Codex, OAuthVendor::Claude, OAuthVendor::Gemini] {
        let redirect = match login_callback::loopback_spec(vendor) {
            Some((path, ports)) => login_callback::loopback_redirect(path, ports.preferred()),
            None => login_callback::manual_redirect(vendor),
        };
        assert!(!redirect.is_empty(), "{vendor:?}");
        let url = reqwest::Url::parse(
            &login_callback::authorization_url(vendor, "state", "verifier", &redirect).unwrap(),
        )
        .unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(
            query["code_challenge"],
            URL_SAFE_NO_PAD.encode(Sha256::digest("verifier"))
        );
        assert_eq!(query["redirect_uri"], redirect);
        assert_eq!(query["state"], "state");
        assert!(!url.as_str().contains("client_secret"));
    }
}

#[test]
fn credential_source_round_trips_for_the_in_app_editor() {
    let source = account(OAuthVendor::Codex);
    let content = interchange::source(&source).unwrap();
    let auth = interchange::parse_file(
        &OAuthImportFile {
            name: "credential.json".into(),
            content,
        },
        Some(OAuthVendor::Codex),
    )
    .unwrap()
    .remove(0)
    .unwrap();
    assert_eq!(auth.access_token, source.auth.access_token);
    assert_eq!(auth.refresh_token, source.auth.refresh_token);
    assert_eq!(
        auth.access_token_expires_at,
        source.auth.access_token_expires_at
    );
    assert!(interchange::same_account(&source.auth, &auth));
}

#[test]
fn credential_source_stays_available_while_a_refresh_is_pending() {
    let mut source = account(OAuthVendor::Claude);
    source.auth.oauth.as_mut().unwrap().refresh_pending = true;
    assert!(interchange::export(&source, OAuthExportFormat::BalanceHub).is_err());
    assert!(interchange::source(&source).is_ok());
}
