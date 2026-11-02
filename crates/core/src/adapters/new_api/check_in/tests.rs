use super::*;
use crate::models::{AppSettings, ProviderInput, ProviderProxyMode};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

const NOT_CHECKED: &str = r#"{"success":true,"data":{"checked_in_today":false}}"#;
const CHECKED: &str = r#"{"success":true,"data":{"checked_in_today":true}}"#;
const SUCCESS: &str = r#"{"success":true,"message":"签到成功"}"#;

struct Reply {
    body: &'static str,
    headers: &'static str,
}
fn json(body: &'static str) -> Option<Reply> {
    Some(Reply { body, headers: "" })
}

fn serve(replies: Vec<Option<Reply>>) -> (Provider, Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, requests) = mpsc::channel();
    let server = thread::spawn(move || {
        for reply in replies {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "missing expected check-in request"
                        );
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("mock accept: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..read]);
                if let Some(end) = bytes.windows(4).position(|item| item == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")
                                .and_then(|length| length.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            sender.send(String::from_utf8(bytes).unwrap()).unwrap();
            if let Some(reply) = reply {
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}", reply.headers, reply.body.len(), reply.body).unwrap();
            }
        }
    });
    let mut input = ProviderInput::default();
    input.identity.base_url = format!("http://{address}/relay");
    input.auth.mode = AuthMode::Session;
    input.auth.session_cookie = "session=fixture-cookie".into();
    input.auth.api_user = "42".into();
    input.proxy.mode = ProviderProxyMode::NoProxy;
    (
        Provider::from_input(input, format!("fixture-{address}")),
        requests,
        server,
    )
}

async fn run(provider: &Provider) -> Result<(Provider, ProviderCheckInResult), String> {
    let client =
        crate::adapters::transport::build_client(&AppSettings::default(), provider).await?;
    check_in_provider(&client, provider).await
}

#[tokio::test]
async fn modern_rotation_is_committed_before_a_later_business_failure() {
    let (mut provider, requests, server) = serve(vec![
        Some(Reply {
            body: r#"{"success":true,"data":{"access_token":"fixture-jwt-new","access_expires_at":4102444800,"user":{"id":42},"session":{"sid":"fixture-sid"}}}"#,
            headers:
                "Set-Cookie: new_api_refresh=fixture-rotated; Path=/api/user/auth; HttpOnly\r\n",
        }),
        json(r#"{"success":false,"message":"fixture business failure"}"#),
    ]);
    provider.auth.session_cookie.clear();
    provider.auth.new_api_session = Some(crate::models::NewApiSession {
        refresh_cookie: "fixture-old".into(),
        access_token: "fixture-expired".into(),
        access_expires_at: Some(1),
        session_id: "fixture-sid".into(),
    });
    let prepared =
        crate::adapters::new_api::prepare_authentication(&AppSettings::default(), &provider)
            .await
            .unwrap();
    prepared.value.unwrap();
    prepared.credentials.apply(&mut provider);
    assert!(run(&provider).await.is_err());
    assert_eq!(
        provider
            .auth
            .new_api_session
            .as_ref()
            .unwrap()
            .refresh_cookie,
        "fixture-rotated"
    );
    server.join().unwrap();
    let requests = requests.try_iter().collect::<Vec<_>>();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /relay/api/user/auth/refresh "));
    assert!(requests[0].contains("new_api_refresh=fixture-old"));
    assert!(requests[1]
        .to_ascii_lowercase()
        .contains("authorization: bearer fixture-jwt-new"));
    assert!(!requests[1].contains("new_api_refresh"));
}

#[tokio::test]
async fn modern_browser_session_refreshes_quota_without_legacy_cookie() {
    let (mut provider, requests, server) = serve(vec![
        json(
            r#"{"success":true,"data":{"system_name":"Fixture","quota_per_unit":500000,"display_type":"USD"}}"#,
        ),
        json(
            r#"{"success":true,"data":{"id":42,"username":"fixture","quota":25000000,"used_quota":5000000}}"#,
        ),
    ]);
    provider.auth.session_cookie.clear();
    provider.auth.new_api_session = Some(crate::models::NewApiSession {
        refresh_cookie: "fixture-refresh".into(),
        access_token: "fixture-dashboard-jwt".into(),
        access_expires_at: Some(4102444800),
        session_id: "fixture-sid".into(),
    });
    let client = crate::adapters::transport::build_client(&AppSettings::default(), &provider)
        .await
        .unwrap();
    let refreshed = crate::adapters::new_api::quota::refresh_provider(&client, &provider).await;
    server.join().unwrap();
    assert_eq!(refreshed.quota.available, 50.0);
    assert_eq!(refreshed.quota.used, 10.0);
    assert!(refreshed.runtime.error_message.is_none());
    assert_eq!(
        refreshed.auth.new_api_session,
        provider.auth.new_api_session
    );
    let requests = requests.try_iter().collect::<Vec<_>>();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("GET /relay/api/user/self "));
    assert!(requests[1]
        .to_ascii_lowercase()
        .contains("authorization: bearer fixture-dashboard-jwt"));
    assert!(!requests[1].contains("fixture-refresh"));
}

#[tokio::test]
async fn standard_check_in_submits_once_and_reads_back_today() {
    let (provider, requests, server) = serve(vec![json(NOT_CHECKED), json(SUCCESS), json(CHECKED)]);
    let (_, result) = run(&provider).await.unwrap();
    server.join().unwrap();
    let requests = requests.try_iter().collect::<Vec<_>>();
    assert!(result.ok);
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("GET /relay/api/user/checkin?month="));
    assert!(requests[1].starts_with("POST /relay/api/user/checkin "));
    assert!(requests[2].starts_with("GET /relay/api/user/checkin?month="));
}

#[tokio::test]
async fn check_in_always_turnstile_yields_before_post_even_with_shield_disabled() {
    let (mut provider, requests, server) = serve(vec![json(NOT_CHECKED)]);
    provider.automation.turnstile_mode = ProviderTurnstileMode::Always;
    provider.automation.auto_shield = false;
    let (_, result) = run(&provider).await.unwrap();
    server.join().unwrap();
    assert_eq!(
        result.verification_required,
        Some(ProviderCheckInVerification::Turnstile)
    );
    assert!(requests
        .try_iter()
        .all(|request| request.starts_with("GET ")));
}

#[tokio::test]
async fn session_check_in_works_on_unknown_host_without_api_user_or_bearer_token() {
    let (mut provider, requests, server) = serve(vec![json(SUCCESS)]);
    provider.automation.check_in_method = ProviderCheckInMethod::SessionSignIn;
    provider.auth.session_cookie = "fixture-cookie".into();
    provider.auth.api_user.clear();
    provider.auth.access_token = "unused-fixture-token".into();
    let (_, result) = run(&provider).await.unwrap();
    server.join().unwrap();
    let request = requests.recv().unwrap();
    assert!(result.ok);
    assert!(request.starts_with("POST /relay/api/user/sign_in "));
    assert!(request.contains("session=fixture-cookie"));
    assert!(!request.to_ascii_lowercase().contains("new-api-user"));
    assert!(!request.contains("unused-fixture-token"));
}

#[tokio::test]
async fn fresh_login_check_in_ignores_cached_session_and_confirms_account() {
    let (mut provider, requests, server) = serve(vec![
        Some(Reply {
            body: r#"{"success":true,"data":{"id":42}}"#,
            headers: "Set-Cookie: session=fresh-fixture; Path=/\r\n",
        }),
        json(r#"{"success":true,"data":{"id":42}}"#),
    ]);
    provider.automation.check_in_method = ProviderCheckInMethod::FreshLogin;
    provider.auth.mode = AuthMode::Password;
    provider.auth.login_username = "fixture".into();
    provider.auth.login_password = "fixture-password".into();
    let (authenticated, result) = run(&provider).await.unwrap();
    server.join().unwrap();
    let requests = requests.try_iter().collect::<Vec<_>>();
    assert!(result.ok);
    assert!(result.message.contains("奖励以站点记录为准"));
    assert!(authenticated.auth.session_cookie.contains("fresh-fixture"));
    assert!(requests[0].starts_with("POST /relay/api/user/login "));
    assert!(!requests[0].contains("fixture-cookie"));
    assert!(requests[1].starts_with("GET /relay/api/user/self "));
    assert!(requests[1].contains("session=fresh-fixture"));
}

#[tokio::test]
async fn uncertain_check_in_post_is_never_repeated() {
    let (provider, requests, server) = serve(vec![json(NOT_CHECKED), None, json(NOT_CHECKED)]);
    let (_, result) = run(&provider).await.unwrap();
    server.join().unwrap();
    assert!(!result.ok);
    assert!(result.unconfirmed);
    assert_eq!(
        requests
            .try_iter()
            .filter(|request| request.starts_with("POST "))
            .count(),
        1
    );
}

#[tokio::test]
async fn check_in_cloudflare_handoff_respects_protection_switch() {
    for enabled in [true, false] {
        let (mut provider, requests, server) = serve(vec![Some(Reply {
            body: "<!doctype html><script>window._cf_chl_opt={}</script>",
            headers: "cf-mitigated: challenge\r\n",
        })]);
        provider.automation.auto_shield = enabled;
        let result = run(&provider).await;
        server.join().unwrap();
        assert_eq!(requests.try_iter().count(), 1);
        if enabled {
            assert_eq!(
                result.unwrap().1.verification_required,
                Some(ProviderCheckInVerification::Cloudflare)
            );
        } else {
            assert!(result.unwrap_err().contains("关闭自动处理站点防护"));
        }
    }
}

#[tokio::test]
async fn password_check_in_handoff_retains_login_stage_despite_cached_session() {
    let (mut provider, requests, server) = serve(vec![Some(Reply {
        body: "<!doctype html><script>window._cf_chl_opt={}</script>",
        headers: "cf-mitigated: challenge\r\n",
    })]);
    provider.auth.mode = AuthMode::Password;
    provider.auth.login_username = "fixture".into();
    provider.auth.login_password = "fixture-password".into();
    let (_, result) = run(&provider).await.unwrap();
    server.join().unwrap();
    assert!(requests
        .recv()
        .unwrap()
        .starts_with("POST /relay/api/user/login "));
    assert_eq!(
        result.verification_required,
        Some(ProviderCheckInVerification::Cloudflare)
    );
    assert!(result.verification_requires_login);
    assert!(serde_json::to_value(result)
        .unwrap()
        .get("verificationRequiresLogin")
        .is_none());
}
