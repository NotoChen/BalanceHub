use super::*;
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

type Requests = Arc<Mutex<Vec<String>>>;

async fn server(
    count: usize,
    handler: impl Fn(&str) -> (u16, Value) + Send + 'static,
) -> (Provider, AppSettings, Requests, tokio::task::JoinHandle<()>) {
    server_with_headers(count, move |request| {
        let (code, body) = handler(request);
        (code, body, Vec::new())
    })
    .await
}

/// Same server, but the response can carry the headers a generation needs to
/// answer with, such as the v8 revision tag.
async fn server_with_headers(
    count: usize,
    handler: impl Fn(&str) -> (u16, Value, Vec<(&'static str, String)>) + Send + 'static,
) -> (Provider, AppSettings, Requests, tokio::task::JoinHandle<()>) {
    raw_server(count, move |request| {
        let (code, body, headers) = handler(request);
        (code, body.to_string().into_bytes(), headers)
    })
    .await
}

/// Serves a body verbatim. The gateway answers some reads with an attachment
/// rather than JSON, so those tests must not go through a JSON encoder.
async fn raw_server(
    count: usize,
    handler: impl Fn(&str) -> (u16, Vec<u8>, Vec<(&'static str, String)>) + Send + 'static,
) -> (Provider, AppSettings, Requests, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests: Requests = Arc::default();
    let observed = requests.clone();
    let worker = tokio::spawn(async move {
        for _ in 0..count {
            let (mut socket, _) =
                tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
                    .await
                    .unwrap()
                    .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let n = socket.read(&mut chunk).await.unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]);
                    let size = header
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + size {
                        break;
                    }
                }
                assert!(bytes.len() < 64 * 1024);
            }
            let request = String::from_utf8(bytes).unwrap();
            let (code, body, headers) = handler(&request);
            observed.lock().unwrap().push(request);
            let headers = headers
                .into_iter()
                .map(|(name, value)| format!("{name}: {value}\r\n"))
                .collect::<String>();
            let response = format!("HTTP/1.1 {code} response\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n", body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
        }
    });
    let mut input = ProviderInput::default();
    input.identity.protocol = ProviderProtocol::CliProxyApi;
    input.identity.base_url = format!("http://{address}");
    input.auth.access_token = "fixture-management-key".into();
    input.proxy.mode = ProviderProxyMode::NoProxy;
    (
        Provider::from_input(input, "cpa-fixture".into()),
        AppSettings::default(),
        requests,
        worker,
    )
}

fn files() -> Value {
    json!({"files":[
        {"name":"a.json","auth_index":"idx-a","type":"codex","email":"a@example.test","status":"active"},
        {"name":"b.json","auth_index":"idx-b","type":"claude","disabled":true}
    ]})
}

#[tokio::test]
async fn v8_route_falls_back_only_when_the_route_is_absent() {
    for initial_status in [404, 401] {
        let (provider, _, observed, worker) =
            server(if initial_status == 404 { 2 } else { 1 }, move |request| {
                if request.starts_with("GET /v8/") {
                    (initial_status, json!({"error":"fixture"}))
                } else {
                    (200, files())
                }
            })
            .await;
        let result = inventory(&Client::builder().no_proxy().build().unwrap(), &provider).await;
        if initial_status == 404 {
            assert_eq!(result.unwrap().version, "v0");
        } else {
            assert!(result.is_err());
        }
        worker.await.unwrap();
        assert_eq!(
            observed.lock().unwrap().len(),
            if initial_status == 404 { 2 } else { 1 }
        );
    }
}

#[tokio::test]
async fn selected_downloads_share_one_inventory_and_one_request_per_file() {
    let (provider, settings, observed, worker) = server(3, |request| {
        if request.starts_with("GET /v8/management/credentials/download?") {
            (200, json!({"type":"codex","access_token":"fixture-access","refresh_token":"fixture-refresh"}))
        } else { (200, files()) }
    }).await;
    let files = download_selected(
        &settings,
        &provider,
        vec!["a.json".into(), "b.json".into(), "a.json".into()],
    )
    .await
    .unwrap();
    assert_eq!(files.len(), 2);
    worker.await.unwrap();
    let observed = observed.lock().unwrap();
    assert_eq!(
        observed
            .iter()
            .filter(|r| r.starts_with("GET /v8/management/credentials HTTP"))
            .count(),
        1
    );
    assert!(observed.iter().all(|r| r
        .to_ascii_lowercase()
        .contains("authorization: bearer fixture-management-key")));
    drop(observed);
}

#[tokio::test]
async fn status_changes_include_auth_index_and_require_upstream_confirmation() {
    for status in ["ok", "error"] {
        let (provider, settings, observed, worker) = server(2, move |request| {
            if request.starts_with("PATCH ") {
                (200, json!({"status":status, "disabled":true}))
            } else {
                (200, files())
            }
        })
        .await;
        assert_eq!(
            set_disabled(&settings, &provider, "a.json", "idx-a", true)
                .await
                .is_ok(),
            status == "ok"
        );
        worker.await.unwrap();
        let observed = observed.lock().unwrap();
        let patch = observed.iter().find(|r| r.starts_with("PATCH ")).unwrap();
        let body: Value = serde_json::from_str(patch.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["auth_index"], "idx-a");
        assert_eq!(body["disabled"], true);
    }
}

#[tokio::test]
async fn uploading_never_overwrites_an_existing_auth_file() {
    let (provider, settings, observed, worker) = server(1, |_| (200, files())).await;
    let file = OAuthExportFile {
        name: "a.json".into(),
        content: "{}".into(),
        instructions: String::new(),
    };
    assert!(upload(&settings, &provider, &file)
        .await
        .unwrap_err()
        .contains("已有同名"));
    worker.await.unwrap();
    assert!(observed
        .lock()
        .unwrap()
        .iter()
        .all(|r| r.starts_with("GET ")));
}

#[test]
fn address_and_inventory_normalization_keep_gateway_and_account_boundaries() {
    assert_eq!(
        crate::models::normalize_cpa_base_url(
            "https://gateway.example.test/prefix/v8/management?unused=1"
        )
        .unwrap(),
        "https://gateway.example.test/prefix"
    );
    assert!(crate::models::normalize_cpa_base_url("https://secret@gateway.example.test").is_err());
    let accounts = parse_inventory(&files()).unwrap();
    assert_eq!(accounts[0].auth_index, "idx-a");
    assert!(accounts[1].disabled);
    assert!(parse_inventory(&json!({"files":[],"has_more":true})).is_err());
    assert!(validate_name("../auth.json").is_err());
}

#[test]
fn inventory_distinguishes_readiness_and_downloadable_files() {
    let accounts = parse_inventory(&json!({"files":[
        {"name":"ready.json","status":"active","source":"file"},
        {"name":"pending.json","status":"pending"},
        {"name":"failed.json","status":"error"},
        {"name":"unknown.json"},
        {"name":"disabled.json","status":"disabled"},
        {"name":"memory.json","status":"active","runtime_only":true,"source":"memory"},
        {"name":"gone.json","status":"active","source":"memory"},
        {"name":"../unsafe.json","status":"active","source":"file"}
    ]}))
    .unwrap();
    assert!(accounts[0].available);
    assert!(accounts[0].can_download);
    assert_eq!(accounts[1].status_label, "待验证");
    assert_eq!(accounts[2].status_label, "连接异常");
    assert!(accounts[1..5].iter().all(|a| !a.available));
    assert!(accounts[4].disabled);
    assert!(accounts[5].available);
    assert!(accounts[5..].iter().all(|a| !a.can_download));
}

#[tokio::test]
async fn runtime_only_credentials_are_rejected_before_download() {
    let (provider, settings, observed, worker) = server(1, |_| {
        (
            200,
            json!({"files":[
                {"name":"memory.json","status":"active","runtime_only":true,"source":"memory"}
            ]}),
        )
    })
    .await;
    assert!(
        download_selected(&settings, &provider, vec!["memory.json".into()])
            .await
            .unwrap_err()
            .contains("没有可下载")
    );
    worker.await.unwrap();
    assert_eq!(observed.lock().unwrap().len(), 1);
}

#[test]
fn inventory_exposes_scheduling_fields_and_accepts_string_lists() {
    let accounts = parse_inventory(&json!({"files":[{
        "name":"a.json","auth_index":"idx-a","provider":"codex",
        "success":7,"failed":"2","priority":"4","weight":9,
        "note":" fixture note ","excluded_models":"gpt-4, gpt-5 ",
        "models":["m1","m2"],"request_retry":0,
        "last_refresh":100,"next_retry_after":200
    }]}))
    .unwrap();
    let account = &accounts[0];
    assert_eq!(account.success, Some(7));
    assert_eq!(account.failed, Some(2));
    assert_eq!(account.priority, Some(4));
    assert_eq!(account.weight, Some(9));
    assert_eq!(account.note, "fixture note");
    assert_eq!(account.excluded_models, vec!["gpt-4", "gpt-5"]);
    assert_eq!(account.models, vec!["m1", "m2"]);
    assert_eq!(account.request_retry, Some(0));
    assert_eq!(account.last_refresh, Some(100));
    assert_eq!(account.next_retry_after, Some(200));
}

#[tokio::test]
async fn field_patch_verifies_the_account_before_writing() {
    let (provider, settings, observed, worker) = server(1, |_| (200, files())).await;
    let error = patch_fields(
        &settings,
        &provider,
        "gone.json",
        "idx-a",
        &CpaCredentialFields {
            priority: Some(5),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(error.contains("已变化"));
    worker.await.unwrap();
    assert_eq!(observed.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn field_patch_sends_only_the_edited_scheduling_fields() {
    let (provider, settings, observed, worker) = server(2, |request| {
        if request.starts_with("PATCH ") {
            (200, json!({"status":"ok"}))
        } else {
            (200, files())
        }
    })
    .await;
    patch_fields(
        &settings,
        &provider,
        "a.json",
        "idx-a",
        &CpaCredentialFields {
            priority: Some(5),
            weight: None,
            request_retry: Some(0),
            note: Some(" fixture ".into()),
            excluded_models: Some(vec!["gpt-4".into()]),
        },
    )
    .await
    .unwrap();
    worker.await.unwrap();
    let requests = observed.lock().unwrap();
    let patch = requests
        .iter()
        .find(|request| request.starts_with("PATCH "))
        .expect("patch request");
    assert!(patch.starts_with("PATCH /v8/management/credentials/fields "));
    let body = patch.split("\r\n\r\n").nth(1).expect("body");
    let body: Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["name"], "a.json");
    assert_eq!(body["auth_index"], "idx-a");
    assert_eq!(body["priority"], 5);
    assert!(body.get("weight").is_none());
    assert_eq!(body["request_retry"], Value::Null);
    assert_eq!(body["note"], "fixture");
    assert_eq!(body["excluded_models"], json!(["gpt-4"]));
}

#[tokio::test]
async fn remote_file_view_returns_the_document_without_write_access() {
    let (provider, settings, observed, worker) = server(2, |request| {
        if request.starts_with("GET /v8/management/credentials/download?") {
            (200, json!({"type":"codex","access_token":"fixture-access"}))
        } else {
            (200, files())
        }
    })
    .await;
    let file = fetch_file(&settings, &provider, "a.json").await.unwrap();
    worker.await.unwrap();
    assert_eq!(file.name, "a.json");
    assert!(file.content.contains("fixture-access"));
    assert!(!file.editable);
    assert!(file.reason.is_some());
    assert_eq!(observed.lock().unwrap().len(), 2);
}

#[test]
fn gateway_settings_read_the_layout_each_management_generation_serves() {
    let v8 = json!({
        "routing": {
            "strategy": "fill-first",
            "force-model-prefix": true,
            "retry": {"request-retry": 4, "max-retry-credentials": 2, "max-retry-interval": 30}
        },
        "requests": {"proxy-url": "http://127.0.0.1:7890"},
        "quota-exceeded": {"switch-project": true, "switch-preview-model": false},
        "observability": {
            "logs": {
                "debug": true,
                "request-log": true,
                "logs-max-total-size-mb": 512,
                "error-logs-max-files": 20
            },
            "usage": {"usage-statistics-enabled": true}
        },
        "oauth": {"providers": {"aistudio": {"ws-auth": true}}},
        "api-keys": ["must-not-leak"]
    });
    let parsed = parse_gateway_settings(&v8, "v8").unwrap();
    assert_eq!(parsed.routing_strategy, "fill-first");
    assert_eq!(parsed.request_retry, Some(4));
    assert_eq!(parsed.max_retry_credentials, Some(2));
    assert_eq!(parsed.max_retry_interval, Some(30));
    assert_eq!(parsed.proxy_url, "http://127.0.0.1:7890");
    assert_eq!(parsed.switch_project, Some(true));
    assert_eq!(parsed.switch_preview_model, Some(false));
    assert_eq!(parsed.request_log, Some(true));
    assert_eq!(parsed.logs_max_total_size_mb, Some(512));
    assert_eq!(parsed.error_logs_max_files, Some(20));
    assert_eq!(parsed.usage_statistics_enabled, Some(true));
    assert_eq!(parsed.debug, Some(true));
    assert_eq!(parsed.force_model_prefix, Some(true));
    assert_eq!(parsed.ws_auth, Some(true));

    let v0 = json!({
        "routing": {"strategy": "round-robin"},
        "request-retry": 3,
        "max-retry-credentials": 0,
        "max-retry-interval": 12,
        "proxy-url": "http://127.0.0.1:1080",
        "quota-exceeded": {"switch-project": false, "switch-preview-model": true},
        "debug": false,
        "force-model-prefix": true,
        "ws-auth": false,
        "request-log": false,
        "logs-max-total-size-mb": 256,
        "error-logs-max-files": 5,
        "usage-statistics-enabled": true
    });
    let parsed = parse_gateway_settings(&v0, "v0").unwrap();
    assert_eq!(parsed.routing_strategy, "round-robin");
    assert_eq!(parsed.request_retry, Some(3));
    assert_eq!(parsed.max_retry_credentials, Some(0));
    assert_eq!(parsed.max_retry_interval, Some(12));
    assert_eq!(parsed.proxy_url, "http://127.0.0.1:1080");
    assert_eq!(parsed.switch_project, Some(false));
    assert_eq!(parsed.switch_preview_model, Some(true));
    assert_eq!(parsed.request_log, Some(false));
    assert_eq!(parsed.logs_max_total_size_mb, Some(256));
    assert_eq!(parsed.error_logs_max_files, Some(5));
    assert_eq!(parsed.usage_statistics_enabled, Some(true));
    assert_eq!(parsed.debug, Some(false));
    assert_eq!(parsed.force_model_prefix, Some(true));
    assert_eq!(parsed.ws_auth, Some(false));
}

#[tokio::test]
async fn gateway_settings_fall_back_to_v0_without_reading_credentials() {
    let (provider, _, observed, worker) = server(2, |request| {
        if request.starts_with("GET /v8/") {
            (404, json!({"error": "not_found"}))
        } else {
            (
                200,
                json!({"routing": {"strategy": "weighted-round-robin"}, "request-retry": 2}),
            )
        }
    })
    .await;
    let (settings, version) =
        gateway_settings(&Client::builder().no_proxy().build().unwrap(), &provider)
            .await
            .unwrap();
    worker.await.unwrap();
    assert_eq!(version, "v0");
    assert_eq!(settings.routing_strategy, "weighted-round-robin");
    assert_eq!(settings.request_retry, Some(2));
    assert_eq!(settings.proxy_url, "");
    assert_eq!(observed.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn gateway_setting_writes_address_one_value_per_request() {
    let (provider, _, observed, worker) = server(3, |request| {
        if request.starts_with("GET /v8/") {
            (404, json!({"error": "not_found"}))
        } else {
            (200, json!({"status": "ok"}))
        }
    })
    .await;
    update_gateway_settings(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        &CpaGatewaySettingsUpdate {
            routing_strategy: Some("fill-first".into()),
            proxy_url: Some("http://127.0.0.1:1080".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    worker.await.unwrap();
    let requests = observed.lock().unwrap();
    assert!(requests
        .iter()
        .any(|request| request.starts_with("PUT /v0/management/routing/strategy ")));
    let proxy = requests
        .iter()
        .find(|request| request.starts_with("PUT /v0/management/proxy-url "))
        .expect("proxy write");
    assert!(proxy
        .split("\r\n\r\n")
        .nth(1)
        .unwrap()
        .contains("http://127.0.0.1:1080"));
}

#[tokio::test]
async fn gateway_setting_writes_use_the_v8_tree_and_reject_unknown_strategies() {
    let (provider, _, observed, worker) = server(1, |_| {
        (
            200,
            json!({"routing": {"strategy": "round-robin", "retry": {}}}),
        )
    })
    .await;
    let error = update_gateway_settings(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        &CpaGatewaySettingsUpdate {
            routing_strategy: Some("random".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error, "CPA 调度策略无效");
    drop(worker);
    assert!(observed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn gateway_log_and_quota_switches_write_one_v0_path_per_value() {
    let (provider, _, observed, worker) = server(4, |request| {
        if request.starts_with("GET /v8/") {
            (404, json!({"error": "not_found"}))
        } else {
            (200, json!({"status": "ok"}))
        }
    })
    .await;
    update_gateway_settings(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        &CpaGatewaySettingsUpdate {
            switch_project: Some(true),
            request_log: Some(false),
            logs_max_total_size_mb: Some(300),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    worker.await.unwrap();
    let requests = observed.lock().unwrap();
    let body = |path: &str| {
        requests
            .iter()
            .find(|request| request.starts_with(path))
            .unwrap_or_else(|| panic!("missing {path}"))
            .split("\r\n\r\n")
            .nth(1)
            .unwrap()
            .to_owned()
    };
    // The legacy endpoints take the value wrapped in an object.
    assert_eq!(
        body("PUT /v0/management/quota-exceeded/switch-project "),
        r#"{"value":true}"#
    );
    assert_eq!(
        body("PUT /v0/management/request-log "),
        r#"{"value":false}"#
    );
    assert_eq!(
        body("PUT /v0/management/logs-max-total-size-mb "),
        r#"{"value":300}"#
    );
}

#[tokio::test]
async fn gateway_log_and_quota_switches_write_the_v8_tree_leaves() {
    let (provider, _, observed, worker) = server_with_headers(4, |request| {
        let tag = vec![("ETag", "\"rev-1\"".to_owned())];
        if request.starts_with("PUT ") {
            (200, json!({"status": "ok", "config-version": 8}), tag)
        } else {
            (
                200,
                json!({"routing": {"strategy": "round-robin", "retry": {}}}),
                tag,
            )
        }
    })
    .await;
    update_gateway_settings(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        &CpaGatewaySettingsUpdate {
            switch_preview_model: Some(true),
            usage_statistics_enabled: Some(true),
            error_logs_max_files: Some(7),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    worker.await.unwrap();
    let requests = observed.lock().unwrap();
    let body = |path: &str| {
        requests
            .iter()
            .find(|request| request.starts_with(path))
            .unwrap_or_else(|| panic!("missing {path}"))
            .split("\r\n\r\n")
            .nth(1)
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        body("PUT /v8/management/config/quota-exceeded/switch-preview-model "),
        "true"
    );
    assert_eq!(
        body("PUT /v8/management/config/observability/usage/usage-statistics-enabled "),
        "true"
    );
    // The v8 tree takes the bare value, not the legacy `{"value": …}` wrapper.
    assert_eq!(
        body("PUT /v8/management/config/observability/logs/error-logs-max-files "),
        "7"
    );
}

#[tokio::test]
async fn gateway_routing_and_debug_switches_write_their_v0_paths() {
    let (provider, _, observed, worker) = server(4, |request| {
        if request.starts_with("GET /v8/") {
            (404, json!({"error": "not_found"}))
        } else {
            (200, json!({"status": "ok"}))
        }
    })
    .await;
    update_gateway_settings(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        &CpaGatewaySettingsUpdate {
            debug: Some(true),
            force_model_prefix: Some(false),
            ws_auth: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    worker.await.unwrap();
    let requests = observed.lock().unwrap();
    for (path, body) in [
        ("PUT /v0/management/debug ", r#"{"value":true}"#),
        (
            "PUT /v0/management/force-model-prefix ",
            r#"{"value":false}"#,
        ),
        ("PUT /v0/management/ws-auth ", r#"{"value":true}"#),
    ] {
        let request = requests
            .iter()
            .find(|request| request.starts_with(path))
            .unwrap_or_else(|| panic!("missing {path}"));
        assert_eq!(request.split("\r\n\r\n").nth(1).unwrap(), body);
    }
}

#[tokio::test]
async fn gateway_routing_and_debug_switches_write_the_v8_tree_leaves() {
    let (provider, _, observed, worker) = server_with_headers(4, |request| {
        let tag = vec![("ETag", "\"rev-1\"".to_owned())];
        if request.starts_with("PUT ") {
            (200, json!({"status": "ok", "config-version": 8}), tag)
        } else {
            (
                200,
                json!({"routing": {"strategy": "round-robin", "retry": {}}}),
                tag,
            )
        }
    })
    .await;
    update_gateway_settings(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        &CpaGatewaySettingsUpdate {
            debug: Some(false),
            force_model_prefix: Some(true),
            ws_auth: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    worker.await.unwrap();
    let requests = observed.lock().unwrap();
    for (path, body) in [
        (
            "PUT /v8/management/config/observability/logs/debug ",
            "false",
        ),
        (
            "PUT /v8/management/config/routing/force-model-prefix ",
            "true",
        ),
        (
            "PUT /v8/management/config/oauth/providers/aistudio/ws-auth ",
            "false",
        ),
    ] {
        let request = requests
            .iter()
            .find(|request| request.starts_with(path))
            .unwrap_or_else(|| panic!("missing {path}"));
        assert_eq!(request.split("\r\n\r\n").nth(1).unwrap(), body);
    }
}

#[tokio::test]
async fn gateway_setting_writes_reject_negative_log_limits() {
    let (provider, _, observed, worker) = server(1, |_| (200, json!({"status": "ok"}))).await;
    let error = update_gateway_settings(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        &CpaGatewaySettingsUpdate {
            logs_max_total_size_mb: Some(-1),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error, "CPA 日志容量与文件数不能为负数");
    drop(worker);
    assert!(observed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn client_keys_read_the_tree_v8_serves_and_the_list_v0_returns() {
    let (provider, _, _, worker) = server(2, |request| {
        if request.starts_with("GET /v8/management/config/access/api-keys ") {
            (200, json!(["sk-a", "sk-b", "   "]))
        } else {
            (200, json!({"access": {"api-keys": ["sk-wrong"]}}))
        }
    })
    .await;
    let keys = client_keys(&Client::builder().no_proxy().build().unwrap(), &provider)
        .await
        .unwrap();
    worker.await.unwrap();
    assert_eq!(keys.keys, vec!["sk-a".to_string(), "sk-b".to_string()]);

    let (provider, _, _, worker) = server(2, |request| {
        if request.starts_with("GET /v8/") {
            (404, json!({"error": "not_found"}))
        } else {
            (200, json!({"api-keys": ["sk-v0"]}))
        }
    })
    .await;
    let keys = client_keys(&Client::builder().no_proxy().build().unwrap(), &provider)
        .await
        .unwrap();
    worker.await.unwrap();
    assert_eq!(keys.keys, vec!["sk-v0".to_string()]);
}

#[tokio::test]
async fn an_unwritten_client_key_list_reads_as_empty() {
    let (provider, _, _, worker) = server(2, |request| {
        if request.starts_with("GET /v8/management/config ") {
            (200, json!({"routing": {"strategy": "round-robin"}}))
        } else {
            (404, json!({"error": "not_found"}))
        }
    })
    .await;
    let keys = client_keys(&Client::builder().no_proxy().build().unwrap(), &provider)
        .await
        .unwrap();
    worker.await.unwrap();
    assert!(keys.keys.is_empty());
}

#[tokio::test]
async fn client_key_edits_put_the_whole_list_under_the_revision_tag() {
    let (provider, _, observed, worker) = server_with_headers(3, |request| {
        if request.starts_with("GET /v8/management/config ") {
            (
                200,
                json!({"access": {"api-keys": ["sk-old"]}}),
                vec![("ETag", "\"rev-1\"".to_string())],
            )
        } else if request.starts_with("PUT ") {
            (
                200,
                json!({"status": "ok"}),
                vec![("ETag", "\"rev-2\"".to_string())],
            )
        } else {
            (200, json!(["sk-old"]), Vec::new())
        }
    })
    .await;
    let list = update_client_keys(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        |keys| {
            keys.push("sk-new".into());
            Ok(())
        },
    )
    .await
    .unwrap();
    worker.await.unwrap();
    assert_eq!(list.keys, vec!["sk-old".to_string(), "sk-new".to_string()]);
    let requests = observed.lock().unwrap();
    let write = requests
        .iter()
        .find(|request| request.starts_with("PUT /v8/management/config/access/api-keys "))
        .expect("client key write");
    assert!(write.contains("if-match: \"rev-1\""));
    assert!(write.contains(r#"["sk-old","sk-new"]"#));
}

#[tokio::test]
async fn a_concurrent_client_key_change_is_reported_instead_of_overwritten() {
    let (provider, _, _, worker) = server_with_headers(3, |request| {
        if request.starts_with("GET /v8/management/config ") {
            (200, json!({}), vec![("ETag", "\"rev-1\"".to_string())])
        } else if request.starts_with("PUT ") {
            (412, json!({"error": "precondition_failed"}), Vec::new())
        } else {
            (200, json!(["sk-old"]), Vec::new())
        }
    })
    .await;
    let error = update_client_keys(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        |keys| {
            keys.clear();
            Ok(())
        },
    )
    .await
    .unwrap_err();
    worker.await.unwrap();
    assert_eq!(error, "网关配置已被其他会话修改，请刷新后重试");
}

#[tokio::test]
async fn plugin_config_reads_the_stored_object_and_reports_unknown_plugins() {
    let (provider, _, observed, worker) =
        server(1, |_| (200, json!({"api_key": "fixture", "enabled": true}))).await;
    let config = plugin_config(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        "oauth-login",
    )
    .await
    .unwrap();
    worker.await.unwrap();
    {
        let requests = observed.lock().unwrap();
        assert!(requests[0].starts_with("GET /v0/management/plugins/oauth-login/config "));
    }
    let parsed: Value = serde_json::from_str(&config.content).unwrap();
    assert_eq!(parsed, json!({"api_key": "fixture", "enabled": true}));

    let (provider, _, _, worker) = server(1, |_| (404, json!({"error": "plugin_not_found"}))).await;
    let error = plugin_config(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        "missing",
    )
    .await
    .unwrap_err();
    worker.await.unwrap();
    assert_eq!(error, "网关上找不到该插件");
}

#[tokio::test]
async fn plugin_switch_writes_the_flag_and_requires_confirmation() {
    let (provider, _, observed, worker) = server(1, |_| (200, json!({"status": "ok"}))).await;
    set_plugin_enabled(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        "oauth-login",
        true,
    )
    .await
    .unwrap();
    worker.await.unwrap();
    {
        let requests = observed.lock().unwrap();
        assert!(requests[0].starts_with("PATCH /v0/management/plugins/oauth-login/enabled "));
        assert!(requests[0].contains(r#"{"enabled":true}"#));
    }

    let (provider, _, _, worker) = server(1, |_| (200, json!({"status": "pending"}))).await;
    let error = set_plugin_enabled(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        "oauth-login",
        false,
    )
    .await
    .unwrap_err();
    worker.await.unwrap();
    assert_eq!(error, "CPA 未确认插件状态变更，请刷新后核对");
}

#[tokio::test]
async fn plugin_config_writes_require_a_json_object() {
    let (provider, _, observed, worker) = server(1, |_| (200, json!({"status": "ok"}))).await;
    let client = Client::builder().no_proxy().build().unwrap();
    for content in ["{", "[]", ""] {
        let error = update_plugin_config(&client, &provider, "oauth-login", content)
            .await
            .unwrap_err();
        assert!(
            matches!(
                error.as_str(),
                "插件配置不是有效的 JSON" | "插件配置必须是 JSON 对象"
            ),
            "{error}"
        );
    }
    let saved = update_plugin_config(&client, &provider, "oauth-login", r#"{"a":1}"#)
        .await
        .unwrap();
    worker.await.unwrap();
    assert_eq!(saved.content, "{\n  \"a\": 1\n}");
    {
        let requests = observed.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("PUT /v0/management/plugins/oauth-login/config "));
    }
}

#[tokio::test]
async fn plugin_ids_cannot_escape_their_route() {
    let (provider, _, _, worker) = server(0, |_| (200, json!({}))).await;
    let client = Client::builder().no_proxy().build().unwrap();
    for id in ["", "../config", "a/b", "-lead", "has space"] {
        let error = plugin_config(&client, &provider, id).await.unwrap_err();
        assert_eq!(error, "CPA 插件标识无效", "{id}");
    }
    worker.await.unwrap();
}

#[tokio::test]
async fn the_gateway_release_is_compared_against_the_reported_build() {
    let (provider, _, observed, worker) =
        server(1, |_| (200, json!({"latest-version": "v8.2.0"}))).await;
    let check = latest_release(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        "v8.1.4",
    )
    .await
    .unwrap();
    worker.await.unwrap();
    {
        let requests = observed.lock().unwrap();
        assert!(requests[0].starts_with("GET /v8/management/server/latest-version "));
    }
    assert_eq!(check.latest, "v8.2.0");
    assert_eq!(check.update_available, Some(true));

    // A gateway without the v8 route still answers on its v0 route.
    let (provider, _, observed, worker) = server(2, |request| {
        if request.starts_with("GET /v8/") {
            (404, json!({"error": "not_found"}))
        } else {
            (200, json!({"latest-version": "v8.1.4"}))
        }
    })
    .await;
    let check = latest_release(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        "v8.1.4",
    )
    .await
    .unwrap();
    worker.await.unwrap();
    {
        let requests = observed.lock().unwrap();
        assert!(requests[1].starts_with("GET /v0/management/latest-version "));
    }
    assert_eq!(check.update_available, Some(false));
}

#[tokio::test]
async fn model_rules_are_read_from_both_value_routes() {
    let (provider, _, observed, worker) = server(2, |request| {
        if request.contains("oauth-excluded-models") {
            (
                200,
                json!({"oauth-excluded-models": {"claude": ["claude-3-opus"]}}),
            )
        } else {
            (200, json!({"oauth-model-alias": null}))
        }
    })
    .await;
    let rules = model_rules(&Client::builder().no_proxy().build().unwrap(), &provider)
        .await
        .unwrap();
    worker.await.unwrap();
    {
        let requests = observed.lock().unwrap();
        assert!(requests[0].starts_with("GET /v0/management/oauth-excluded-models "));
        assert!(requests[1].starts_with("GET /v0/management/oauth-model-alias "));
    }
    let excluded: Value = serde_json::from_str(&rules.excluded_models).unwrap();
    assert_eq!(excluded, json!({"claude": ["claude-3-opus"]}));
    // An unwritten map answers null, which the panel edits as an empty object.
    let aliases: Value = serde_json::from_str(&rules.model_aliases).unwrap();
    assert_eq!(aliases, json!({}));
}

#[tokio::test]
async fn a_rule_map_the_gateway_omits_is_not_read_as_empty() {
    let (provider, _, _, worker) = server(1, |_| (200, json!({"status": "ok"}))).await;
    let error = model_rules(&Client::builder().no_proxy().build().unwrap(), &provider)
        .await
        .unwrap_err();
    worker.await.unwrap();
    assert_eq!(error, "CPA 未返回 oauth-excluded-models");
}

#[tokio::test]
async fn model_rule_writes_replace_one_map_and_return_the_confirmed_rules() {
    let (provider, _, observed, worker) = server(3, |request| {
        if request.starts_with("PUT ") {
            (200, json!({"status": "ok"}))
        } else if request.contains("oauth-excluded-models") {
            (200, json!({"oauth-excluded-models": {"claude": ["opus"]}}))
        } else {
            // CPA lowercases providers while normalizing what it stores.
            (200, json!({"oauth-model-alias": {"codex": [{"name": "gpt-5", "alias": "gpt-5-fast", "fork": true}]}}))
        }
    })
    .await;
    let rules = update_model_rules(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        CpaModelRuleKind::ModelAliases,
        r#"{"Codex":[{"name":"gpt-5","alias":"gpt-5-fast","fork":true}]}"#,
    )
    .await
    .unwrap();
    worker.await.unwrap();
    {
        let requests = observed.lock().unwrap();
        assert!(requests[0].starts_with("PUT /v0/management/oauth-model-alias "));
        assert!(requests[1].starts_with("GET /v0/management/oauth-excluded-models "));
        assert!(requests[2].starts_with("GET /v0/management/oauth-model-alias "));
        assert_eq!(requests.len(), 3);
    }
    let aliases: Value = serde_json::from_str(&rules.model_aliases).unwrap();
    assert_eq!(aliases["codex"][0]["alias"], "gpt-5-fast");
}

#[tokio::test]
async fn model_rule_writes_require_an_object_and_gateway_confirmation() {
    let (provider, _, observed, worker) = server(1, |_| (200, json!({"status": "pending"}))).await;
    let client = Client::builder().no_proxy().build().unwrap();
    for content in ["{", "[]", "null"] {
        let error = update_model_rules(
            &client,
            &provider,
            CpaModelRuleKind::ExcludedModels,
            content,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                error.as_str(),
                "模型规则不是有效的 JSON" | "模型规则必须是 JSON 对象"
            ),
            "{content}: {error}"
        );
    }
    let error = update_model_rules(&client, &provider, CpaModelRuleKind::ExcludedModels, "{}")
        .await
        .unwrap_err();
    assert_eq!(error, "CPA 未确认模型规则变更，请刷新后核对");
    worker.await.unwrap();
    // Only the confirmed-looking write reached the gateway.
    assert_eq!(observed.lock().unwrap().len(), 1);
}

#[test]
fn rule_documents_are_checked_before_they_reach_the_gateway() {
    let check = |kind: CpaModelRuleKind, text: &str| {
        check_rule_document(kind, &serde_json::from_str(text).unwrap())
    };
    assert!(check(CpaModelRuleKind::ExcludedModels, r#"{"claude":["opus"]}"#).is_ok());
    assert_eq!(
        check(CpaModelRuleKind::ExcludedModels, r#"{"claude":"opus"}"#).unwrap_err(),
        "通道 claude 的值必须是数组"
    );
    assert_eq!(
        check(CpaModelRuleKind::ExcludedModels, r#"{"claude":[1]}"#).unwrap_err(),
        "通道 claude 的排除模型必须是字符串"
    );
    assert!(check(
        CpaModelRuleKind::ModelAliases,
        r#"{"codex":[{"name":"gpt-5","alias":"fast","fork":true}]}"#
    )
    .is_ok());
    for text in [r#"{"codex":["fast"]}"#, r#"{"codex":[{"name":"gpt-5"}]}"#] {
        assert_eq!(
            check(CpaModelRuleKind::ModelAliases, text).unwrap_err(),
            "通道 codex 的每个别名都需要 name 和 alias"
        );
    }
}

#[test]
fn model_rule_kinds_keep_the_names_the_panel_sends() {
    let wire = |kind| serde_json::to_string(&kind).unwrap();
    assert_eq!(
        wire(CpaModelRuleKind::ExcludedModels),
        r#""excludedModels""#
    );
    assert_eq!(wire(CpaModelRuleKind::ModelAliases), r#""modelAliases""#);
}

#[test]
fn version_comparison_ignores_prefixes_and_build_suffixes() {
    assert_eq!(compare_versions("v8.2.0", "8.1.9"), Some(true));
    assert_eq!(compare_versions("8.1.4", "v8.1.4"), Some(false));
    assert_eq!(compare_versions("v8.1.4", "v8.1.4-dirty"), Some(false));
    assert_eq!(compare_versions("v8.10.0", "v8.9.9"), Some(true));
    assert_eq!(compare_versions("v8.2", "v8.2.0"), Some(false));
    assert_eq!(compare_versions("nightly", "v8.1.4"), None);
    assert_eq!(compare_versions("v8.2.0", ""), None);
}

fn plain_client() -> Client {
    Client::builder().no_proxy().build().unwrap()
}

fn request_body(request: &str) -> Value {
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("");
    serde_json::from_str(body).unwrap()
}

#[tokio::test]
async fn login_providers_list_the_built_ins_and_only_running_plugin_logins() {
    let (provider, settings, observed, worker) = server(2, |request| {
        if request.contains("/credentials") {
            (200, files())
        } else {
            (
                200,
                json!({"plugins_enabled":true,"plugins":[
                    {"id":"sso","enabled":true,"effective_enabled":true,"oauth_provider":"corp"},
                    {"id":"sleepy","enabled":true,"effective_enabled":false,"oauth_provider":"sleepy"},
                    {"id":"claude-copy","enabled":true,"effective_enabled":true,"oauth_provider":"claude"},
                    {"id":"quota","enabled":true,"effective_enabled":true,"oauth_provider":""}
                ]}),
            )
        }
    })
    .await;
    let providers = login::login_providers(&settings, &provider).await.unwrap();
    worker.await.unwrap();
    let ids = providers.iter().map(|p| p.id.as_str()).collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "codex",
            "claude",
            "antigravity",
            "kimi",
            "kimi-ai",
            "xai",
            "devin",
            "meta",
            "corp"
        ]
    );
    // Only the running plugin is offered: a stopped plugin has no login route
    // for the gateway to dispatch to.
    assert_eq!(providers[8].plugin.as_deref(), Some("sso"));
    assert!(providers[..8].iter().all(|p| p.plugin.is_none()));
    assert_eq!(observed.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn login_starts_on_v8_and_reports_the_generation_that_answered() {
    let (provider, _, observed, worker) = server(1, |_| {
        (
            200,
            json!({"status":"ok","url":"https://accounts.example.test/authorize","state":"abc123"}),
        )
    })
    .await;
    let start = login::start_login(&plain_client(), &provider, "codex")
        .await
        .unwrap();
    worker.await.unwrap();
    assert_eq!(start.state, "abc123");
    assert_eq!(start.flow, "browser");
    assert_eq!(start.api_version, "v8");
    assert!(start.user_code.is_none());
    let request = observed.lock().unwrap()[0].clone();
    assert!(
        request.starts_with("GET /v8/management/oauth/auth-url?"),
        "{request}"
    );
    assert!(request.contains("provider=codex"));
    assert!(request.contains("is_webui=true"));
}

#[tokio::test]
async fn a_v0_gateway_signs_in_through_its_per_provider_route() {
    let (provider, _, observed, worker) = server(2, |request| {
        if request.starts_with("GET /v8/") {
            (404, json!({"error":"not found"}))
        } else {
            (
                200,
                json!({"status":"ok","url":"https://claude.example.test/authorize","state":"s1","flow":"device","user_code":"ABCD","expires_in":600}),
            )
        }
    })
    .await;
    let start = login::start_login(&plain_client(), &provider, "claude")
        .await
        .unwrap();
    worker.await.unwrap();
    // v0 names the Claude route after the vendor key it stores.
    let requests = observed.lock().unwrap();
    assert!(
        requests[1].starts_with("GET /v0/management/anthropic-auth-url?"),
        "{}",
        requests[1]
    );
    assert!(requests[1].contains("is_webui=true"));
    assert_eq!(start.api_version, "v0");
    assert_eq!(start.flow, "device");
    assert_eq!(start.user_code.as_deref(), Some("ABCD"));
    assert_eq!(start.expires_in, Some(600));
}

#[tokio::test]
async fn a_refused_login_surfaces_the_gateways_reason() {
    let (provider, _, _, worker) = server(1, |_| {
        (500, json!({"error":"failed to start callback server"}))
    })
    .await;
    let error = login::start_login(&plain_client(), &provider, "codex")
        .await
        .unwrap_err();
    worker.await.unwrap();
    assert_eq!(error, "CPA 无法准备回调地址，请检查网关端口是否可用");
}

#[tokio::test]
async fn login_status_reads_both_generations_and_keeps_the_waiting_state() {
    let (provider, _, observed, worker) = server(1, |_| (200, json!({"status":"wait"}))).await;
    let status = login::login_status(&plain_client(), &provider, "abc", "v8")
        .await
        .unwrap();
    worker.await.unwrap();
    assert_eq!(status, CpaLoginStatus::Waiting);
    assert!(observed.lock().unwrap()[0].starts_with("GET /v8/management/oauth/status?state=abc"));

    let (provider, _, observed, worker) = server(2, |request| {
        if request.starts_with("GET /v0/") {
            (404, json!({"error":"not found"}))
        } else {
            (200, json!({"status":"ok"}))
        }
    })
    .await;
    // A session started on v0 is polled there first, and a gateway that no
    // longer serves that route is retried on v8.
    let status = login::login_status(&plain_client(), &provider, "abc", "v0")
        .await
        .unwrap();
    worker.await.unwrap();
    assert_eq!(status, CpaLoginStatus::Completed);
    assert!(observed.lock().unwrap()[1].starts_with("GET /v8/management/oauth/status?state=abc"));
}

#[tokio::test]
async fn login_status_reports_expiry_as_a_restart_prompt() {
    let (provider, _, _, worker) = server(1, |_| {
        (
            200,
            json!({"status":"error","error":"unknown or expired state"}),
        )
    })
    .await;
    assert_eq!(
        login::login_status(&plain_client(), &provider, "abc", "v8")
            .await
            .unwrap(),
        CpaLoginStatus::Failed {
            message: "授权会话已过期，请重新开始".into()
        }
    );
    worker.await.unwrap();

    let (provider, _, _, worker) = server(1, |_| {
        (400, json!({"status":"error","error":"invalid state"}))
    })
    .await;
    assert_eq!(
        login::login_status(&plain_client(), &provider, "abc", "v8")
            .await
            .unwrap_err(),
        "授权会话标识无效"
    );
    worker.await.unwrap();
}

#[tokio::test]
async fn the_callback_carries_a_redirect_url_or_a_bare_code() {
    let (provider, _, observed, worker) = server(1, |_| (200, json!({"status":"ok"}))).await;
    login::submit_login_callback(
        &plain_client(),
        &provider,
        "claude",
        "abc",
        "v8",
        " http://localhost:1455/callback?code=xyz&state=abc ",
    )
    .await
    .unwrap();
    worker.await.unwrap();
    let request = observed.lock().unwrap()[0].clone();
    assert!(
        request.starts_with("POST /v8/management/oauth/callback "),
        "{request}"
    );
    assert_eq!(
        request_body(&request),
        json!({"provider":"claude","state":"abc","redirect_url":"http://localhost:1455/callback?code=xyz&state=abc"})
    );

    let (provider, _, observed, worker) = server(1, |_| (200, json!({"status":"ok"}))).await;
    login::submit_login_callback(&plain_client(), &provider, "codex", "abc", "v8", "xyz")
        .await
        .unwrap();
    worker.await.unwrap();
    let request = observed.lock().unwrap()[0].clone();
    assert_eq!(
        request_body(&request),
        json!({"provider":"codex","state":"abc","code":"xyz"})
    );
}

#[tokio::test]
async fn a_finished_callback_is_reported_in_the_panels_words() {
    let (provider, _, _, worker) = server(1, |_| {
        (
            409,
            json!({"status":"error","error":"oauth flow is already completed"}),
        )
    })
    .await;
    assert_eq!(
        login::submit_login_callback(&plain_client(), &provider, "codex", "abc", "v8", "xyz")
            .await
            .unwrap_err(),
        "该授权已完成，可直接刷新卡片"
    );
    worker.await.unwrap();

    let (provider, _, _, worker) = server(1, |_| {
        (
            400,
            json!({"status":"error","error":"code or error is required"}),
        )
    })
    .await;
    assert_eq!(
        login::submit_login_callback(&plain_client(), &provider, "codex", "abc", "v8", "xyz")
            .await
            .unwrap_err(),
        "回调地址里没有授权码"
    );
    worker.await.unwrap();
}

#[tokio::test]
async fn cancelling_treats_an_already_finished_session_as_done() {
    let (provider, _, observed, worker) =
        server(1, |_| (200, json!({"status":"ok","cancelled":false}))).await;
    login::cancel_login(&plain_client(), &provider, "abc")
        .await
        .unwrap();
    worker.await.unwrap();
    assert!(
        observed.lock().unwrap()[0].starts_with("DELETE /v8/management/oauth/session?state=abc")
    );

    // Neither generation knows the route any more: the session is gone either
    // way, so the caller is not given an error to act on.
    let (provider, _, observed, worker) = server(2, |_| (404, json!({"error":"not found"}))).await;
    login::cancel_login(&plain_client(), &provider, "abc")
        .await
        .unwrap();
    worker.await.unwrap();
    assert_eq!(observed.lock().unwrap().len(), 2);

    let (provider, _, _, worker) = server(1, |_| (500, json!({"error":"boom"}))).await;
    assert_eq!(
        login::cancel_login(&plain_client(), &provider, "abc")
            .await
            .unwrap_err(),
        "取消 CPA 授权失败（HTTP 500）"
    );
    worker.await.unwrap();
}

/// A gateway that predates the grouped route still cancels: the v0 spelling
/// takes the same state and answers the same way.
#[tokio::test]
async fn a_gateway_without_the_v8_cancel_route_is_cancelled_on_v0() {
    let (provider, _, observed, worker) = server(2, |request| {
        if request.starts_with("DELETE /v8/") {
            (405, json!({"error":"method not allowed"}))
        } else {
            (200, json!({"status":"ok","cancelled":true}))
        }
    })
    .await;
    login::cancel_login(&plain_client(), &provider, "abc")
        .await
        .unwrap();
    worker.await.unwrap();
    let requests = observed.lock().unwrap();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert!(requests[1].starts_with("DELETE /v0/management/oauth-session?state=abc"));
}

#[tokio::test]
async fn login_identifiers_are_checked_before_they_reach_the_gateway() {
    let (provider, _, observed, worker) = server(0, |_| (200, json!({}))).await;
    let client = plain_client();
    assert_eq!(
        login::start_login(&client, &provider, "../escape")
            .await
            .unwrap_err(),
        "CPA 授权服务标识无效"
    );
    assert_eq!(
        login::login_status(&client, &provider, "a/b", "v8")
            .await
            .unwrap_err(),
        "CPA 授权会话标识无效"
    );
    assert_eq!(
        login::submit_login_callback(&client, &provider, "codex", "abc", "v8", "  ")
            .await
            .unwrap_err(),
        "请粘贴回调地址或授权码"
    );
    drop(worker);
    assert!(observed.lock().unwrap().is_empty());
}

#[test]
fn login_status_keeps_the_names_the_panel_reads() {
    let wire = |status| serde_json::to_value(&status).unwrap();
    assert_eq!(wire(CpaLoginStatus::Waiting), json!({"status":"waiting"}));
    assert_eq!(
        wire(CpaLoginStatus::Completed),
        json!({"status":"completed"})
    );
    assert_eq!(
        wire(CpaLoginStatus::Failed {
            message: "x".into()
        }),
        json!({"status":"failed","message":"x"})
    );
}

#[tokio::test]
async fn account_quota_reads_the_v8_plugin_route() {
    let (provider, _, observed, worker) = server(1, |_| {
        (
            200,
            json!({
                "subscription": {"plan": "Pro", "tierName": "Tier 2"},
                "summary": [
                    {
                        "key": "balance",
                        "label": "余额",
                        "value": 12.5,
                        "unit": "USD",
                        "format": "currency",
                        "currency": "usd"
                    },
                    {"key": "unlabelled", "value": 1}
                ],
                "groups": [{
                    "displayName": "Claude",
                    "buckets": [
                        {"window": "5h", "remainingFraction": 0.4, "resetTime": "2026-01-01T00:00:00Z"},
                        {"window": "weekly", "remaining_fraction": 1.4, "reset_time": "2026-01-02T00:00:00Z"}
                    ]
                }]
            }),
        )
    })
    .await;
    let quota = quota::account_quota(&plain_client(), &provider, "idx-1", "sso", "v8")
        .await
        .unwrap();
    worker.await.unwrap();
    assert_eq!(quota.plan, "Pro");
    assert_eq!(quota.tier, "Tier 2");
    assert_eq!(
        quota.metrics.len(),
        1,
        "a metric without a label is dropped"
    );
    assert_eq!(quota.metrics[0].value, 12.5);
    assert_eq!(quota.metrics[0].currency, "USD");
    let buckets = &quota.groups[0].buckets;
    assert_eq!(quota.groups[0].display_name, "Claude");
    assert_eq!(buckets[0].remaining_fraction, Some(0.4));
    assert_eq!(buckets[0].reset_at, Some(1_767_225_600));
    assert_eq!(
        buckets[1].remaining_fraction, None,
        "a fraction past one cannot be drawn as a bar"
    );
    assert_eq!(buckets[1].reset_at, Some(1_767_312_000));
    let request = observed.lock().unwrap()[0].clone();
    assert!(
        request.starts_with("GET /v8/management/plugins/sso/quota?"),
        "{request}"
    );
    assert!(request.contains("auth_index=idx-1"));
}

#[tokio::test]
async fn account_quota_falls_back_to_the_generic_route() {
    let (provider, _, observed, worker) = server(2, |request| {
        if request.contains("/v8/management/plugins/") {
            (404, json!({"error": "not found"}))
        } else {
            (
                200,
                json!({"summary":[{"key":"tokens","label":"Token","value":10,"format":"number","unit":"M"}]}),
            )
        }
    })
    .await;
    let quota = quota::account_quota(&plain_client(), &provider, "idx-1", "sso", "v8")
        .await
        .unwrap();
    worker.await.unwrap();
    assert_eq!(quota.metrics[0].label, "Token");
    assert_eq!(quota.metrics[0].unit, "M");
    assert!(quota.groups.is_empty());
    let requests = observed.lock().unwrap();
    assert!(
        requests[1].starts_with("POST /v0/management/quota/fetch"),
        "{}",
        requests[1]
    );
    assert_eq!(request_body(&requests[1]), json!({"auth_index": "idx-1"}));
}

#[tokio::test]
async fn a_v0_gateway_reads_the_quota_without_a_plugin() {
    let (provider, _, observed, worker) = server(1, |_| (200, json!({"summary": []}))).await;
    quota::account_quota(&plain_client(), &provider, "idx-1", "", "v0")
        .await
        .unwrap();
    worker.await.unwrap();
    assert!(
        observed.lock().unwrap()[0].starts_with("POST /v0/management/quota/fetch"),
        "the declarative probe is only served by the generic route"
    );
}

#[tokio::test]
async fn a_refused_quota_keeps_the_reason_the_gateway_gave() {
    for (code, body, expected) in [
        (
            404,
            json!({"error":"quota provider not found for plugin"}),
            "该插件不提供额度查询",
        ),
        (
            502,
            json!({"error":"failed to fetch quota: upstream 429"}),
            "CPA 读取额度失败：upstream 429",
        ),
        (
            500,
            json!({"error":"boom"}),
            "读取 CPA 授权额度失败（HTTP 500）",
        ),
    ] {
        let (provider, _, _, worker) = server(1, move |_| (code, body.clone())).await;
        let error = quota::account_quota(&plain_client(), &provider, "idx-1", "", "v0")
            .await
            .unwrap_err();
        worker.await.unwrap();
        assert_eq!(error, expected);
    }
}

#[tokio::test]
async fn quota_identifiers_are_checked_before_they_reach_the_gateway() {
    let (provider, _, observed, worker) = server(0, |_| (200, json!({}))).await;
    let client = plain_client();
    assert_eq!(
        quota::account_quota(&client, &provider, "../escape", "", "v0")
            .await
            .unwrap_err(),
        "CPA 授权索引无效"
    );
    assert_eq!(
        quota::account_quota(&client, &provider, "idx-1", "a/b", "v8")
            .await
            .unwrap_err(),
        "CPA 插件标识无效"
    );
    drop(worker);
    assert!(observed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_plugin_store_lists_entries_and_the_sources_that_failed() {
    let (provider, settings, observed, worker) = server(2, |request| {
        if request.contains("/credentials") {
            (200, files())
        } else {
            (
                200,
                json!({
                    "plugins_enabled": true,
                    "sources": [{"id": "official", "name": "官方", "url": "https://store.example.test"}],
                    "source_errors": [{"source_id": "mirror", "message": "registry timeout"}],
                    "plugins": [
                        {
                            "id": "quota",
                            "source_id": "official",
                            "source_name": "官方",
                            "name": "Quota",
                            "description": "Reads provider quota",
                            "author": "example",
                            "version": "1.2.0",
                            "repository": "example/quota",
                            "tags": ["quota", " "],
                            "auth_required": true,
                            "installed": true,
                            "installed_version": "1.1.0",
                            "update_available": true
                        },
                        {"id": "", "name": "nameless"},
                        {"id": "login", "source_id": "official"}
                    ]
                }),
            )
        }
    })
    .await;
    let store = plugin_store::store(&settings, &provider).await.unwrap();
    worker.await.unwrap();
    assert!(store.enabled);
    let ids = store
        .plugins
        .iter()
        .map(|plugin| plugin.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        ["quota", "login"],
        "an entry without an id cannot be installed"
    );
    let quota = &store.plugins[0];
    assert_eq!(quota.source_id, "official");
    assert_eq!(quota.tags, ["quota"], "blank tags are dropped");
    assert!(quota.auth_required);
    assert!(!quota.auth_configured);
    assert!(quota.update_available);
    assert_eq!(quota.installed_version, "1.1.0");
    assert_eq!(store.source_errors.len(), 1);
    assert_eq!(store.source_errors[0].source_id, "mirror");
    assert_eq!(store.source_errors[0].message, "registry timeout");
    let request = observed.lock().unwrap()[1].clone();
    assert!(
        request.starts_with("GET /v8/management/plugins/store "),
        "{request}"
    );
}

#[tokio::test]
async fn a_v0_gateway_keeps_the_dashed_store_route() {
    let (provider, settings, observed, worker) = server(3, |request| {
        if request.starts_with("GET /v8/") {
            (404, json!({"error": "not found"}))
        } else if request.contains("/auth-files") {
            (200, files())
        } else {
            (200, json!({"plugins_enabled": false, "plugins": []}))
        }
    })
    .await;
    let store = plugin_store::store(&settings, &provider).await.unwrap();
    worker.await.unwrap();
    assert!(!store.enabled);
    assert!(store.plugins.is_empty());
    let request = observed.lock().unwrap()[2].clone();
    assert!(
        request.starts_with("GET /v0/management/plugin-store "),
        "{request}"
    );
}

#[tokio::test]
async fn installing_names_the_source_and_reports_the_restart() {
    let (provider, settings, observed, worker) = server(2, |request| {
        if request.contains("/credentials") {
            (200, files())
        } else {
            (
                200,
                json!({"status":"installed","id":"quota","version":"1.2.0","restart_required":true}),
            )
        }
    })
    .await;
    let installed = plugin_store::install(&settings, &provider, "quota", "official", "1.2.0")
        .await
        .unwrap();
    worker.await.unwrap();
    assert_eq!(installed.id, "quota");
    assert_eq!(installed.version, "1.2.0");
    assert!(installed.restart_required);
    let request = observed.lock().unwrap()[1].clone();
    assert!(
        request.starts_with("POST /v8/management/plugins/store/quota/install?"),
        "{request}"
    );
    assert!(request.contains("source=official"));
    assert!(request.contains("version=1.2.0"));
    assert_eq!(request_body(&request), json!({"version": "1.2.0"}));
}

#[tokio::test]
async fn a_refused_install_keeps_the_stores_reason() {
    for (code, body, expected) in [
        (
            409,
            json!({"error":"plugin_update_requires_restart","restart_required":true}),
            "该插件正在运行，重启网关后才能更新",
        ),
        (
            502,
            json!({"error":"plugin_install_failed","message":"dial tcp: timeout"}),
            "插件安装失败：dial tcp: timeout",
        ),
        (
            429,
            json!({"error":"plugin_store_rate_limited","retry_after":30}),
            "插件商店请求过于频繁，请 30 秒后重试",
        ),
        (
            500,
            json!({"error":"boom"}),
            "安装 CPA 插件失败（HTTP 500）",
        ),
    ] {
        let (provider, settings, _, worker) = server(2, move |request| {
            if request.contains("/credentials") {
                (200, files())
            } else {
                (code, body.clone())
            }
        })
        .await;
        let error = plugin_store::install(&settings, &provider, "quota", "", "")
            .await
            .unwrap_err();
        worker.await.unwrap();
        assert_eq!(error, expected);
    }
}

#[tokio::test]
async fn store_identifiers_are_checked_before_they_reach_the_gateway() {
    let (provider, settings, observed, worker) = server(0, |_| (200, json!({}))).await;
    assert_eq!(
        plugin_store::install(&settings, &provider, "../escape", "", "")
            .await
            .unwrap_err(),
        "CPA 插件标识无效"
    );
    assert_eq!(
        plugin_store::install(&settings, &provider, "quota", "a/b", "")
            .await
            .unwrap_err(),
        "CPA 插件来源无效"
    );
    assert_eq!(
        plugin_store::install(&settings, &provider, "quota", "official", "1 2")
            .await
            .unwrap_err(),
        "CPA 插件版本号无效"
    );
    drop(worker);
    assert!(observed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn removing_a_plugin_deletes_its_file_and_configuration() {
    let (provider, _, observed, worker) = server(1, |_| {
        (
            200,
            json!({"status":"deleted","id":"quota","file_deleted":true,"configured_removed":true,"restart_required":false}),
        )
    })
    .await;
    let removal = remove_plugin(&plain_client(), &provider, "quota")
        .await
        .unwrap();
    worker.await.unwrap();
    assert!(removal.file_deleted);
    assert!(removal.configured_removed);
    assert!(
        observed.lock().unwrap()[0].starts_with("DELETE /v0/management/plugins/quota "),
        "per-plugin writes stay on the v0 routes"
    );
}

#[tokio::test]
async fn a_refused_removal_keeps_the_gateways_reason() {
    for (code, body, expected) in [
        (
            409,
            json!({"error":"plugin_delete_requires_restart","restart_required":true}),
            "该插件正在运行，重启网关后才能移除",
        ),
        (
            500,
            json!({"error":"plugin_delete_failed","message":"permission denied"}),
            "移除插件文件失败：permission denied",
        ),
        (
            500,
            json!({"error":"config_save_failed","message":"disk full","file_deleted":true}),
            "插件已删除，但网关未能保存配置：disk full",
        ),
        (404, json!({"error":"plugin_not_found"}), "网关已没有该插件"),
        (
            500,
            json!({"error":"boom"}),
            "移除 CPA 插件失败（HTTP 500）",
        ),
    ] {
        let (provider, _, _, worker) = server(1, move |_| (code, body.clone())).await;
        let error = remove_plugin(&plain_client(), &provider, "quota")
            .await
            .unwrap_err();
        worker.await.unwrap();
        assert_eq!(error, expected);
    }
}

#[tokio::test]
async fn an_unconfirmed_removal_is_not_reported_as_done() {
    let (provider, _, _, worker) = server(1, |_| (200, json!({"status":"ok"}))).await;
    let error = remove_plugin(&plain_client(), &provider, "quota")
        .await
        .unwrap_err();
    worker.await.unwrap();
    assert_eq!(error, "CPA 未确认插件移除，请刷新插件列表核对");
}

#[tokio::test]
async fn a_log_follow_read_sends_the_cursor_and_reports_a_rotation() {
    let (provider, settings, observed, worker) = server(4, |request| {
        if request.starts_with("GET /v8/management/credentials") {
            return (200, files());
        }
        if request.contains("cursor=") {
            return (
                200,
                json!({"lines":["second"],"line-count":1,"latest-timestamp":9,"next-cursor":"next","cursor-reset":true}),
            );
        }
        (
            200,
            json!({"lines":["first"],"line-count":1,"latest-timestamp":8,"next-cursor":"first"}),
        )
    })
    .await;
    let tail = request_logs(&settings, &provider, 200, "").await.unwrap();
    assert_eq!(tail.lines, vec!["first"]);
    assert_eq!(tail.next_cursor, "first");
    assert!(!tail.cursor_reset);
    let follow = request_logs(&settings, &provider, 50, "first")
        .await
        .unwrap();
    assert_eq!(follow.lines, vec!["second"]);
    assert_eq!(follow.next_cursor, "next");
    assert!(follow.cursor_reset);
    worker.await.unwrap();
    let observed = observed.lock().unwrap();
    assert!(
        observed[1].starts_with("GET /v8/management/observability/logs?limit=200 HTTP"),
        "{}",
        observed[1]
    );
    assert!(
        observed[3].contains("cursor=first") && observed[3].contains("limit=50"),
        "{}",
        observed[3]
    );
    drop(observed);
}

/// A gateway that is not writing a log is not a broken connection, and the
/// panel offers the switch that fixes it instead of a generic status.
#[tokio::test]
async fn a_disabled_log_is_explained_in_the_panels_words() {
    let (provider, settings, _, worker) = server(2, |request| {
        if request.starts_with("GET /v8/management/credentials") {
            (200, files())
        } else {
            (400, json!({"error":"logging to file disabled"}))
        }
    })
    .await;
    let error = request_logs(&settings, &provider, 200, "")
        .await
        .unwrap_err();
    worker.await.unwrap();
    assert_eq!(error, "CPA 未开启日志写入文件，请求日志不可用");
}

#[tokio::test]
async fn a_forged_log_cursor_is_refused_before_it_reaches_the_gateway() {
    let (provider, settings, observed, worker) = server(0, |_| (200, json!({}))).await;
    let error = request_logs(&settings, &provider, 200, "a/b")
        .await
        .unwrap_err();
    drop(worker);
    assert_eq!(error, "CPA 日志游标无效");
    assert!(observed.lock().unwrap().is_empty());
}

/// The gateway keys inline API-key credentials by `base_url|api_key` and groups
/// them by provider; the panel needs the parts, not the composite.
#[tokio::test]
async fn api_key_usage_splits_the_composite_and_lists_the_busiest_first() {
    let (provider, settings, observed, worker) = server(2, |request| {
        if request.starts_with("GET /v8/management/credentials") {
            (200, files())
        } else {
            (
                200,
                json!({
                    "openai": {
                        "https://api.example.test|sk-a": {
                            "success": 2,
                            "failed": 1,
                            "recent_requests": [
                                {"time": "2026-10-22T10:00:00Z", "success": 2, "failed": 1}
                            ]
                        },
                        "|sk-b": {"success": 0, "failed": 5}
                    },
                    "claude": {"https://claude.example.test|sk-c": {"success": 9, "failed": 0}}
                }),
            )
        }
    })
    .await;
    let usage = api_key_usage(&settings, &provider).await.unwrap();
    worker.await.unwrap();
    assert_eq!(usage.len(), 3);
    assert_eq!(usage[0].provider, "claude");
    assert_eq!(usage[0].key, "sk-c");
    // A key without a base URL is keyed as `|key`, so it must not swallow the
    // rest of the composite.
    assert_eq!(usage[1].key, "sk-b");
    assert_eq!(usage[1].base_url, "");
    assert_eq!(usage[1].failed, 5);
    assert_eq!(usage[2].base_url, "https://api.example.test");
    assert_eq!(usage[2].recent_requests.len(), 1);
    assert_eq!(usage[2].recent_requests[0].success, 2);
    let observed = observed.lock().unwrap();
    assert!(
        observed[1].starts_with("GET /v8/management/observability/usage/api-keys"),
        "{}",
        observed[1]
    );
    drop(observed);
}

#[tokio::test]
async fn api_key_usage_reads_the_legacy_route_and_rejects_an_unreadable_body() {
    let (provider, settings, observed, worker) = server(3, |request| {
        if request.starts_with("GET /v8/management/credentials") {
            (404, json!({"error":"fixture"}))
        } else if request.starts_with("GET /v0/management/auth-files") {
            (200, files())
        } else {
            (
                200,
                json!({"openai": {"|sk-a": {"success": 1, "failed": 0}}}),
            )
        }
    })
    .await;
    let usage = api_key_usage(&settings, &provider).await.unwrap();
    assert_eq!(usage[0].key, "sk-a");
    // 先取出再断言：guard 留在绑定里会让后面的 await 带着锁跑，新版 clippy 直接报错。
    let requests = observed.lock().unwrap().clone();
    assert!(
        requests[2].starts_with("GET /v0/management/api-key-usage"),
        "{}",
        requests[2]
    );
    worker.await.unwrap();

    let (provider, settings, _, worker) = server(2, |request| {
        if request.starts_with("GET /v8/management/credentials") {
            (200, files())
        } else {
            (200, json!(["not", "an", "object"]))
        }
    })
    .await;
    let error = api_key_usage(&settings, &provider).await.unwrap_err();
    worker.await.unwrap();
    assert_eq!(error, "CPA 调用用量格式无效");
}

/// A line in the shared tail names the request it belongs to; the drill-down
/// then reads that request's own file instead of the interleaved tail.
#[tokio::test]
async fn a_request_drill_down_reads_its_own_file_from_the_path_that_holds_it() {
    let (provider, settings, observed, worker) = raw_server(2, |request| {
        if request.starts_with("GET /v8/management/credentials") {
            return (200, files().to_string().into_bytes(), Vec::new());
        }
        (200, b"first line\nsecond line\n".to_vec(), Vec::new())
    })
    .await;
    let log = request_log(&settings, &provider, "ab12cd34").await.unwrap();
    worker.await.unwrap();
    assert_eq!(log.request_id, "ab12cd34");
    assert_eq!(log.content, "first line\nsecond line\n");
    // 先取出快照再断言：guard 一旦留在绑定里，新版 clippy 会把整个后续作用域
    // 都当成“持锁跨 await”。
    let requests = observed.lock().unwrap().clone();
    assert!(
        requests[1].starts_with("GET /v8/management/observability/logs/requests/ab12cd34"),
        "{}",
        requests[1]
    );

    // The legacy generation serves the same file under its own route.
    let (provider, settings, observed, worker) = raw_server(3, |request| {
        if request.starts_with("GET /v8/management/credentials") {
            return (404, b"{}".to_vec(), Vec::new());
        }
        if request.starts_with("GET /v0/management/auth-files") {
            return (200, files().to_string().into_bytes(), Vec::new());
        }
        (200, b"legacy body".to_vec(), Vec::new())
    })
    .await;
    let log = request_log(&settings, &provider, "ab12cd34").await.unwrap();
    worker.await.unwrap();
    assert_eq!(log.content, "legacy body");
    // 临时绑定让 guard 在断言后立刻释放；直接写 `observed.lock().unwrap()[2]` 时
    // 临时值会活到语句结束，新版 clippy 会把它当成跨 await 持锁。
    let observed = observed.lock().unwrap();
    assert!(
        observed[2].starts_with("GET /v0/management/request-log-by-id/ab12cd34"),
        "{}",
        observed[2]
    );
    drop(observed);
}

/// A request file only exists while the gateway keeps one per request, so the
/// miss names that setting instead of reading as a broken connection.
#[tokio::test]
async fn a_missing_request_log_points_at_the_setting_that_creates_it() {
    let (provider, settings, _, worker) = server(2, |request| {
        if request.starts_with("GET /v8/management/credentials") {
            (200, files())
        } else {
            (
                404,
                json!({"error":"log file not found for the given request ID"}),
            )
        }
    })
    .await;
    let error = request_log(&settings, &provider, "ab12cd34")
        .await
        .unwrap_err();
    worker.await.unwrap();
    assert_eq!(
        error,
        "CPA 未找到这次请求的日志文件；可能未开启「单独记录请求日志」，或日志已被清理"
    );
}

#[tokio::test]
async fn a_forged_request_id_is_refused_before_it_reaches_the_gateway() {
    let (provider, settings, observed, worker) = server(0, |_| (200, json!({}))).await;
    let overlong = "a".repeat(129);
    for id in ["", "ab12/cd34", "ab12\\cd34", overlong.as_str()] {
        let error = request_log(&settings, &provider, id).await.unwrap_err();
        assert_eq!(error, "CPA 请求 ID 无效", "{id}");
    }
    drop(worker);
    assert!(observed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn clearing_the_log_reports_how_many_rotated_files_went_away() {
    let (provider, settings, observed, worker) = server(2, |request| {
        if request.starts_with("GET /v8/management/credentials") {
            (200, files())
        } else {
            (
                200,
                json!({"success":true,"message":"Logs cleared successfully","removed":3}),
            )
        }
    })
    .await;
    let cleared = clear_logs(&settings, &provider).await.unwrap();
    worker.await.unwrap();
    assert_eq!(cleared.removed, 3);
    let observed = observed.lock().unwrap();
    assert!(
        observed[1].starts_with("DELETE /v8/management/observability/logs"),
        "{}",
        observed[1]
    );
}

#[tokio::test]
async fn clearing_a_disabled_log_is_explained_and_an_unconfirmed_answer_is_refused() {
    for (code, body, expected) in [
        (
            400,
            json!({"error":"logging to file disabled"}),
            "CPA 未开启日志写入文件，没有可清理的请求日志",
        ),
        (
            200,
            json!({"status":"ok"}),
            "CPA 未确认日志清理结果，请刷新后核对",
        ),
    ] {
        let (provider, settings, _, worker) = server(2, move |request| {
            if request.starts_with("GET /v8/management/credentials") {
                (200, files())
            } else {
                (code, body.clone())
            }
        })
        .await;
        let error = clear_logs(&settings, &provider).await.unwrap_err();
        worker.await.unwrap();
        assert_eq!(error, expected);
    }
}

#[tokio::test]
async fn the_log_switch_is_read_and_written_in_each_generations_shape() {
    // v8 keeps the switch in the configuration tree and guards the write with
    // the tag of the revision it was read from.
    let (provider, settings, observed, worker) = server_with_headers(4, |request| {
        if request.starts_with("GET /v8/management/config/observability") {
            return (200, json!(true), vec![("ETag", "\"tag-1\"".into())]);
        }
        if request.starts_with("PUT /v8/management/config/observability") {
            assert!(request.contains("if-match: \"tag-1\""), "{request}");
            assert_eq!(request_body(request), json!(false));
            return (
                200,
                json!({"status":"ok","config-version":8}),
                vec![("ETag", "\"tag-2\"".into())],
            );
        }
        (
            200,
            json!({"routing":{}}),
            vec![("ETag", "\"tag-1\"".into())],
        )
    })
    .await;
    let settings_value = log_settings(&settings, &provider).await.unwrap();
    assert!(settings_value.enabled);
    set_log_settings(&settings, &provider, false).await.unwrap();
    worker.await.unwrap();
    {
        let requests = observed.lock().unwrap();
        assert_eq!(requests.len(), 4, "{requests:?}");
        assert!(
            requests[1].starts_with("GET /v8/management/config/observability/logs/logging-to-file")
        );
        assert!(
            requests[3].starts_with("PUT /v8/management/config/observability/logs/logging-to-file")
        );
    }

    // v0 has a route of its own and takes `{value}`.
    let (provider, settings, observed, worker) = server(4, |request| {
        if request.starts_with("GET /v8/") {
            return (404, json!({"error":"not found"}));
        }
        if request.starts_with("PUT /v0/management/logging-to-file") {
            return (200, json!({"status":"ok"}));
        }
        if request.starts_with("GET /v0/management/logging-to-file") {
            return (200, json!({"logging-to-file":false}));
        }
        (200, files())
    })
    .await;
    let settings_value = log_settings(&settings, &provider).await.unwrap();
    assert!(!settings_value.enabled);
    set_log_settings(&settings, &provider, true).await.unwrap();
    worker.await.unwrap();
    let requests = observed.lock().unwrap();
    assert_eq!(requests.len(), 4, "{requests:?}");
    assert!(
        requests[3].starts_with("PUT /v0/management/logging-to-file "),
        "{}",
        requests[3]
    );
    assert_eq!(request_body(&requests[3]), json!({"value": true}));
}

/// The configuration document is served as YAML, not JSON, so it is read as
/// text; the revision tag it comes with is what a save has to carry.
#[tokio::test]
async fn the_gateway_configuration_is_read_verbatim_with_its_revision_tag() {
    let yaml = "routing:\n  strategy: round-robin\n";
    let (provider, settings, observed, worker) = raw_server(1, |request| {
        assert!(
            request.starts_with("GET /v8/management/config.yaml "),
            "{request}"
        );
        (
            200,
            yaml.as_bytes().to_vec(),
            vec![("ETag", "\"hash-1\"".to_owned())],
        )
    })
    .await;
    let config = gateway_config(&settings, &provider).await.unwrap();
    worker.await.unwrap();
    assert_eq!(config.content, yaml);
    assert_eq!(config.etag, "\"hash-1\"");
    assert_eq!(config.api_version, "v8");
    drop(observed);
}

/// A v0 gateway has no revision tag, and its own route is what answers once
/// the v8 one is absent.
#[tokio::test]
async fn the_gateway_configuration_falls_back_to_the_v0_route() {
    let (provider, settings, observed, worker) = raw_server(2, |request| {
        if request.starts_with("GET /v8/management/config.yaml ") {
            return (404, b"{}".to_vec(), Vec::new());
        }
        (200, b"debug: true\n".to_vec(), Vec::new())
    })
    .await;
    let config = gateway_config(&settings, &provider).await.unwrap();
    worker.await.unwrap();
    assert_eq!(config.content, "debug: true\n");
    assert_eq!(config.etag, "");
    assert_eq!(config.api_version, "v0");
    let requests = observed.lock().unwrap();
    assert!(
        requests[1].starts_with("GET /v0/management/config.yaml "),
        "{}",
        requests[1]
    );
}

/// Neither generation answering means the document is not there, which is a
/// state the panel names instead of showing an empty editor.
#[tokio::test]
async fn a_gateway_without_a_configuration_file_says_so() {
    let (provider, settings, _, worker) =
        raw_server(2, |_| (404, b"{}".to_vec(), Vec::new())).await;
    let error = gateway_config(&settings, &provider).await.unwrap_err();
    worker.await.unwrap();
    assert_eq!(
        error,
        "网关没有提供 config.yaml，请确认管理接口已启用且配置文件存在"
    );
}

/// The save is a whole-document replacement, so it has to carry the tag of the
/// revision the text came from.
#[tokio::test]
async fn saving_the_configuration_sends_the_yaml_and_its_tag() {
    let (provider, settings, observed, worker) = raw_server(1, |request| {
        assert!(
            request.starts_with("PUT /v8/management/config.yaml "),
            "{request}"
        );
        let header = request.split("\r\n\r\n").next().unwrap();
        assert!(
            header
                .lines()
                .any(|line| line.eq_ignore_ascii_case("If-Match: \"hash-1\"")),
            "{header}"
        );
        assert!(
            header
                .lines()
                .any(|line| line.eq_ignore_ascii_case("Content-Type: application/yaml")),
            "{header}"
        );
        (
            200,
            br#"{"status":"ok","config-version":8}"#.to_vec(),
            Vec::new(),
        )
    })
    .await;
    update_gateway_config(
        &settings,
        &provider,
        "debug: true\nrouting:\n  strategy: fill-first\n",
        "\"hash-1\"",
        "v8",
    )
    .await
    .unwrap();
    worker.await.unwrap();
    let requests = observed.lock().unwrap();
    assert_eq!(
        requests[0].split("\r\n\r\n").nth(1).unwrap(),
        "debug: true\nrouting:\n  strategy: fill-first\n"
    );
}

/// v0 keeps no revision tag, so its save carries only the document.
#[tokio::test]
async fn saving_a_v0_configuration_uses_its_own_route_without_a_tag() {
    let (provider, settings, observed, worker) = raw_server(1, |request| {
        assert!(
            request.starts_with("PUT /v0/management/config.yaml "),
            "{request}"
        );
        assert!(!request.to_lowercase().contains("if-match"), "{request}");
        (200, br#"{"ok":true}"#.to_vec(), Vec::new())
    })
    .await;
    update_gateway_config(&settings, &provider, "debug: false\n", "", "v0")
        .await
        .unwrap();
    worker.await.unwrap();
    drop(observed);
}

/// A refused save leaves the file untouched, so the panel repeats what the
/// gateway objected to rather than a generic failure.
#[tokio::test]
async fn a_refused_configuration_names_the_gateways_reason() {
    let calls = Arc::new(Mutex::new(0usize));
    let counter = calls.clone();
    let (provider, settings, _, worker) = raw_server(2, move |request| {
        assert!(
            request.starts_with("PUT /v8/management/config.yaml "),
            "{request}"
        );
        let mut count = counter.lock().unwrap();
        *count += 1;
        if *count == 1 {
            return (
                422,
                json!({"error": "invalid_config", "message": "retry must be >= 0"})
                    .to_string()
                    .into_bytes(),
                Vec::new(),
            );
        }
        (
            412,
            json!({"error": "precondition_failed"})
                .to_string()
                .into_bytes(),
            Vec::new(),
        )
    })
    .await;
    let error = update_gateway_config(&settings, &provider, "x: 1\n", "\"old\"", "v8")
        .await
        .unwrap_err();
    assert_eq!(error, "配置未通过网关校验：retry must be >= 0");
    let error = update_gateway_config(&settings, &provider, "x: 1\n", "\"old\"", "v8")
        .await
        .unwrap_err();
    worker.await.unwrap();
    assert_eq!(error, "网关配置已被其他会话修改，请重新读取后再保存");
}

/// v0 accepts an empty document as a configuration, so an empty save is
/// refused here instead of replacing the gateway's file with nothing.
#[tokio::test]
async fn an_empty_configuration_is_refused_before_it_reaches_the_gateway() {
    let (provider, settings, observed, worker) =
        raw_server(0, |_| (200, Vec::new(), Vec::new())).await;
    let error = update_gateway_config(&settings, &provider, "   \n", "", "v0")
        .await
        .unwrap_err();
    worker.await.unwrap();
    assert_eq!(error, "网关配置不能为空");
    assert!(observed.lock().unwrap().is_empty());
}

/// A gateway serves the catalog from the v0 route both management generations
/// mount, so one request answers it without reading the configuration first.
#[tokio::test]
async fn the_channel_catalog_is_read_from_the_shared_v0_route() {
    let (provider, _, observed, worker) = server(1, |_| {
        (
            200,
            json!({"channel": "claude", "models": [
                {"id": "claude-opus-4-1", "display_name": "Claude Opus 4.1", "type": "claude", "owned_by": "anthropic", "context_length": 200000},
                {"id": "claude-haiku-4-5", "owned_by": "anthropic"},
                {"display_name": "a definition without an id"},
                {"id": "zero-window", "context_length": 0}
            ]}),
        )
    })
    .await;
    let models = model_definitions(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        "claude",
    )
    .await
    .unwrap();
    worker.await.unwrap();
    {
        let requests = observed.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /v0/management/model-definitions/claude "));
    }
    assert_eq!(
        models.len(),
        3,
        "a definition without an id cannot be named in a rule"
    );
    assert_eq!(models[0].id, "claude-opus-4-1");
    assert_eq!(models[0].display_name, "Claude Opus 4.1");
    assert_eq!(models[0].model_type, "claude");
    assert_eq!(models[0].context_length, Some(200_000));
    assert_eq!(models[1].context_length, None);
    assert_eq!(
        models[2].context_length, None,
        "a zero window is not a window"
    );
}

/// An unknown channel is a choice the user can fix, so the gateway's bare 400
/// becomes a sentence naming the channel.
#[tokio::test]
async fn an_unknown_channel_names_itself_in_the_error() {
    let (provider, _, _, worker) = server(1, |_| {
        (400, json!({"error": "unknown channel", "channel": "nope"}))
    })
    .await;
    let error = model_definitions(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        "nope",
    )
    .await
    .unwrap_err();
    worker.await.unwrap();
    assert_eq!(error, "CPA 不认识通道「nope」，请确认通道名后再读");
}

#[tokio::test]
async fn an_invalid_channel_never_reaches_the_gateway() {
    let (provider, _, observed, worker) = server(0, |_| (200, json!({}))).await;
    let error = model_definitions(
        &Client::builder().no_proxy().build().unwrap(),
        &provider,
        "../config",
    )
    .await
    .unwrap_err();
    worker.await.unwrap();
    assert_eq!(error, "CPA 模型通道无效");
    assert!(observed.lock().unwrap().is_empty());
}
