use super::*;
use crate::{
    adapters::{
        sub2_api::Sub2ApiAdapter,
        test_support::{serve, Reply},
    },
    models::{AppSettings, AuthMode, ProviderApiKeyQuotaInput, ProviderApiKeySpendingLimits},
};

const CREATED: &str = r#"{"code":0,"data":{"id":17,"name":"fixture","key":"custom-fixture-key","group_id":3,"group":{"id":3,"name":"premium"},"status":"active","quota":15,"quota_used":18,"ip_whitelist":["10.0.0.0/8"],"ip_blacklist":["10.1.0.0/16"],"rate_limit_5h":2,"rate_limit_1d":5,"rate_limit_7d":10,"expires_at":null}}"#;

#[tokio::test]
async fn sub2_creation_sends_group_total_quota_network_rules_and_all_spending_periods() {
    let (mut provider, requests, server) = serve(vec![
        Reply(200, r#"{"code":0,"data":[{"id":3,"name":"premium"}]}"#),
        Reply(200, CREATED),
    ]);
    provider.identity.protocol = ProviderProtocol::Sub2Api;
    provider.auth.mode = AuthMode::AccessToken;
    provider.auth.access_token = "fixture-jwt".into();
    let (_, option) = Sub2ApiAdapter
        .create_api_key(
            &AppSettings::default(),
            &provider,
            &ProviderApiKeyPatch {
                name: Some("fixture".into()),
                group: Some("3".into()),
                quota: Some(ProviderApiKeyQuotaInput {
                    unlimited: false,
                    amount: 15.0,
                }),
                allow_ips: Some(vec!["10.0.0.0/8".into()]),
                deny_ips: Some(vec!["10.1.0.0/16".into()]),
                spending_limits: Some(ProviderApiKeySpendingLimits {
                    five_hours: 2.0,
                    one_day: 5.0,
                    seven_days: 10.0,
                }),
                expiration: Some(ProviderApiKeyExpiration::AfterDays { days: 30 }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    server.join().unwrap();
    let requests = requests.iter().collect::<Vec<_>>();
    assert!(requests[1].starts_with("POST /relay/api/v1/keys "));
    let sent: Value = serde_json::from_str(requests[1].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(sent["group_id"], 3);
    assert_eq!(sent["quota"], 15.0);
    assert_eq!(sent["expires_in_days"], 30);
    assert_eq!(sent["ip_blacklist"], json!(["10.1.0.0/16"]));
    assert_eq!(sent["rate_limit_5h"], 2.0);
    assert_eq!(sent["rate_limit_1d"], 5.0);
    assert_eq!(sent["rate_limit_7d"], 10.0);
    assert_eq!(option.group_id, "3");
    assert_eq!(option.group, "premium");
    assert_eq!(option.remain_quota, 0.0);
    assert_eq!(
        ProviderApiKeySettings::from_option(&option, ProviderProtocol::Sub2Api)
            .quota
            .amount,
        15.0,
        "total limit must not be reconstructed from clamped remaining quota"
    );
}

#[test]
fn sub2_edits_are_sparse_and_empty_arrays_explicitly_clear_restrictions() {
    let renamed = payload(
        &ProviderApiKeyPatch {
            name: Some("new".into()),
            ..Default::default()
        },
        false,
    )
    .unwrap();
    assert_eq!(renamed, json!({"name":"new"}));
    let cleared = payload(
        &ProviderApiKeyPatch {
            allow_ips: Some(vec![]),
            deny_ips: Some(vec![]),
            expiration: Some(ProviderApiKeyExpiration::Never),
            ..Default::default()
        },
        false,
    )
    .unwrap();
    assert_eq!(
        cleared,
        json!({"ip_whitelist":[],"ip_blacklist":[],"expires_at":""})
    );
    assert!(payload(
        &ProviderApiKeyPatch {
            group: Some(String::new()),
            ..Default::default()
        },
        false
    )
    .is_err());
}

#[test]
fn invalid_limits_and_protocol_specific_fields_fail_before_a_remote_write() {
    assert!(payload(
        &ProviderApiKeyPatch {
            quota: Some(ProviderApiKeyQuotaInput {
                unlimited: false,
                amount: 0.0
            }),
            ..Default::default()
        },
        false
    )
    .is_err());
    assert!(payload(
        &ProviderApiKeyPatch {
            allow_ips: Some(vec!["10.0.0.1/99".into()]),
            ..Default::default()
        },
        false
    )
    .is_err());
    assert!(payload(
        &ProviderApiKeyPatch {
            model_limits: Some(vec!["model-a".into()]),
            ..Default::default()
        },
        false
    )
    .is_err());
    assert!(ProviderApiKeyPatch {
        deny_ips: Some(vec![]),
        ..Default::default()
    }
    .validate(ProviderProtocol::NewApi, false)
    .is_err());
}

#[test]
fn sub2_expiration_uses_days_on_create_and_rfc3339_on_update() {
    let patch = ProviderApiKeyPatch {
        expiration: Some(ProviderApiKeyExpiration::At {
            timestamp: 4102444800000,
        }),
        ..Default::default()
    };
    assert_eq!(
        payload(&patch, false).unwrap()["expires_at"],
        "2100-01-01T00:00:00Z"
    );
    assert!(payload(&patch, true).is_err());
}
