use super::*;
use crate::{
    adapters::{
        new_api::NewApiAdapter,
        test_support::{serve, Reply},
    },
    models::{AppSettings, ProviderApiKeyQuotaInput},
};

const SITE: &str = r#"{"success":true,"data":{"system_name":"Fixture","quota_per_unit":500000}}"#;
const TOKEN: &str = r#"{"success":true,"data":{"id":17,"name":"original","group":"first","status":1,"remain_quota":432100,"unlimited_quota":false,"expired_time":4102444800,"model_limits_enabled":true,"model_limits":"model-a,model-b","allow_ips":"127.0.0.1\n10.0.0.0/8","cross_group_retry":false,"used_quota":123000}}"#;

fn body(request: &str) -> Value {
    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[tokio::test]
async fn changing_newapi_group_preserves_fresh_quota_expiry_and_access_limits() {
    let (provider, requests, server) = serve(vec![
        Reply(
            200,
            r#"{"success":true,"data":{"second":{"ratio":2,"desc":"Second group"}}}"#,
        ),
        Reply(200, SITE),
        Reply(200, TOKEN),
        Reply(200, TOKEN),
    ]);
    NewApiAdapter
        .update_api_key(
            &AppSettings::default(),
            &provider,
            "17",
            &ProviderApiKeyPatch {
                group: Some("second".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    server.join().unwrap();
    let requests = requests.iter().collect::<Vec<_>>();
    assert!(requests[2].starts_with("GET /relay/api/token/17 "));
    let sent = body(&requests[3]);
    assert_eq!(sent["group"], "second");
    assert_eq!(sent["remain_quota"], 432100);
    assert_eq!(sent["expired_time"], 4102444800i64);
    assert_eq!(sent["model_limits"], "model-a,model-b");
    assert_eq!(sent["allow_ips"], "127.0.0.1\n10.0.0.0/8");
    assert!(sent.get("used_quota").is_none());
    assert!(sent.get("status").is_none());
}

#[tokio::test]
async fn newapi_creation_identifies_the_new_token_among_duplicate_names() {
    let (provider, requests, server) = serve(vec![
        Reply(200, SITE),
        Reply(
            200,
            r#"{"success":true,"data":{"items":[{"id":16,"name":"same"}]}}"#,
        ),
        Reply(200, r#"{"success":true,"message":""}"#),
        Reply(
            200,
            r#"{"success":true,"data":{"items":[{"id":17,"name":"same","group":"","remain_quota":0,"unlimited_quota":true,"expired_time":-1},{"id":16,"name":"same"}]}}"#,
        ),
        Reply(
            200,
            r#"{"success":true,"data":{"key":"fixture-created-key"}}"#,
        ),
    ]);
    let (_, created) = NewApiAdapter
        .create_api_key(
            &AppSettings::default(),
            &provider,
            &ProviderApiKeyPatch {
                name: Some("same".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(created.token_id, "17");
    assert_eq!(created.key, "sk-fixture-created-key");
    assert!(created.unlimited_quota);
    let requests = requests.iter().collect::<Vec<_>>();
    assert!(requests[4].starts_with("POST /relay/api/token/17/key "));
    let sent = body(&requests[2]);
    assert_eq!(sent["remain_quota"], 0);
    assert_eq!(sent["unlimited_quota"], true);
}

#[tokio::test]
async fn readback_auth_failure_cannot_repeat_a_successful_create() {
    let (mut provider, requests, server) = serve(vec![
        Reply(200, SITE),
        Reply(200, r#"{"success":true,"data":{"items":[]}}"#),
        Reply(200, r#"{"success":true,"message":""}"#),
        Reply(401, r#"{"success":false,"message":"not logged in"}"#),
        Reply(401, r#"{"success":false,"message":"not logged in"}"#),
    ]);
    provider.auth.access_token = "fixture-access-token".into();
    let error = NewApiAdapter
        .create_api_key(
            &AppSettings::default(),
            &provider,
            &ProviderApiKeyPatch {
                name: Some("unique".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    server.join().unwrap();
    assert!(error.contains("已创建"));
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.starts_with("POST /relay/api/token/ "))
            .count(),
        1
    );
}

#[tokio::test]
async fn id_only_creation_response_reads_settings_before_returning_metadata() {
    let (provider, requests, server) = serve(vec![
        Reply(200, SITE),
        Reply(200, r#"{"success":true,"data":{"items":[]}}"#),
        Reply(200, r#"{"success":true,"data":{"id":17}}"#),
        Reply(200, TOKEN),
        Reply(
            200,
            r#"{"success":true,"data":{"key":"fixture-created-key"}}"#,
        ),
    ]);
    let (_, created) = NewApiAdapter
        .create_api_key(
            &AppSettings::default(),
            &provider,
            &ProviderApiKeyPatch {
                name: Some("original".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(created.token_id, "17");
    assert_eq!(created.name, "original");
    assert_eq!(created.group, "first");
    assert_eq!(created.remain_quota_raw, 432100);
    assert!(created.model_limits_enabled);
    let requests = requests.iter().collect::<Vec<_>>();
    assert!(requests[3].starts_with("GET /relay/api/token/17 "));
}

#[tokio::test]
async fn status_only_changes_do_not_replace_the_keys_other_settings() {
    let (provider, requests, server) =
        serve(vec![Reply(200, SITE), Reply(200, TOKEN), Reply(200, TOKEN)]);
    NewApiAdapter
        .update_api_key(
            &AppSettings::default(),
            &provider,
            "17",
            &ProviderApiKeyPatch {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    server.join().unwrap();
    let requests = requests.iter().collect::<Vec<_>>();
    assert_eq!(requests.len(), 3);
    assert!(requests[2].starts_with("PUT /relay/api/token/?status_only=true "));
    assert_eq!(body(&requests[2]), json!({"id":17,"status":2}));
}

#[test]
fn newapi_quota_conversion_and_explicit_clear_use_upstream_units() {
    let site = SiteMetadata {
        currency_exchange_rate: 2.0,
        ..Default::default()
    };
    let mut payload = json!({"group":"auto", "remain_quota":9, "model_limits":"old", "allow_ips":"old", "auto_groups":["a"]});
    apply_patch(
        &mut payload,
        &ProviderApiKeyPatch {
            quota: Some(ProviderApiKeyQuotaInput {
                unlimited: false,
                amount: 2.5,
            }),
            allow_ips: Some(Vec::new()),
            model_limits: Some(Vec::new()),
            auto_groups: Some(Vec::new()),
            expiration: Some(ProviderApiKeyExpiration::Never),
            ..Default::default()
        },
        &site,
    )
    .unwrap();
    assert_eq!(payload["remain_quota"], 625000);
    assert_eq!(payload["expired_time"], -1);
    assert_eq!(payload["allow_ips"], "");
    assert_eq!(payload["model_limits"], "");
    assert_eq!(payload["auto_groups"], json!([]));
}

#[test]
fn duplicate_creation_results_never_pick_an_existing_or_arbitrary_key() {
    let prior = HashSet::from(["16".into()]);
    assert!(find_created_token(
        &prior,
        vec![
            json!({"id":16,"name":"same"}),
            json!({"id":17,"name":"other"})
        ],
        "same"
    )
    .is_err());
    assert!(find_created_token(
        &prior,
        vec![
            json!({"id":17,"name":"same"}),
            json!({"id":18,"name":"same"})
        ],
        "same"
    )
    .is_err());
}

#[tokio::test]
async fn exhausted_key_receives_new_quota_before_it_is_enabled() {
    let (provider, requests, server) = serve(vec![
        Reply(200, SITE),
        Reply(
            200,
            r#"{"success":true,"data":{"id":17,"name":"empty","status":4,"group":"","remain_quota":0,"unlimited_quota":false,"expired_time":-1}}"#,
        ),
        Reply(200, TOKEN),
        Reply(200, TOKEN),
    ]);
    NewApiAdapter
        .update_api_key(
            &AppSettings::default(),
            &provider,
            "17",
            &ProviderApiKeyPatch {
                quota: Some(ProviderApiKeyQuotaInput {
                    unlimited: false,
                    amount: 5.0,
                }),
                enabled: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    server.join().unwrap();
    let requests = requests.iter().collect::<Vec<_>>();
    assert!(requests[2].starts_with("PUT /relay/api/token/ "));
    assert_eq!(body(&requests[2])["remain_quota"], 2_500_000);
    assert!(body(&requests[2]).get("status").is_none());
    assert!(requests[3].starts_with("PUT /relay/api/token/?status_only=true "));
    assert_eq!(body(&requests[3])["status"], 1);
}

#[tokio::test]
async fn update_success_without_metadata_reads_the_single_key_instead_of_using_old_data() {
    let (provider, requests, server) = serve(vec![
        Reply(200, SITE),
        Reply(200, TOKEN),
        Reply(200, r#"{"success":true,"message":""}"#),
        Reply(200, TOKEN),
    ]);
    let (_, result) = NewApiAdapter
        .update_api_key(
            &AppSettings::default(),
            &provider,
            "17",
            &ProviderApiKeyPatch {
                name: Some("changed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(result.token_id, "17");
    let requests = requests.iter().collect::<Vec<_>>();
    assert!(requests[3].starts_with("GET /relay/api/token/17 "));
}
