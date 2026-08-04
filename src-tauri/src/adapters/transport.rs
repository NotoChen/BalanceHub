use crate::{
    models::{AppSettings, Provider},
    network::{self, shield},
};
use reqwest::{
    header::{COOKIE, USER_AGENT},
    Client, IntoUrl, Method, Request, RequestBuilder, StatusCode, Url,
};
use std::collections::{HashMap, HashSet};

/// Business requests use the same explicit UA as the challenge WebView.
pub(crate) const USER_AGENT_VALUE: &str = shield::WEBVIEW_USER_AGENT;
const MAX_CHALLENGE_ROUNDS: usize = 2;

/// A provider-scoped HTTP client. Raw execution stays private so every provider
/// response passes through the same challenge and replay policy.
#[derive(Clone)]
pub(crate) struct ProviderTransport {
    client: Client,
    shield_context: shield::ShieldContext,
}

pub(crate) fn build_client(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<ProviderTransport, String> {
    let proxy = network::resolve_proxy(settings, provider);
    let client = network::build_provider_client_with_proxy(proxy.clone())?;
    let shield_context = shield::ShieldContext::new(
        provider.identity.id.clone(),
        proxy.fingerprint(),
        proxy.webview_proxy_url(),
    );
    Ok(ProviderTransport {
        client,
        shield_context,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct TransportResponse {
    pub status: StatusCode,
    pub headers: reqwest::header::HeaderMap,
    pub body: String,
    pub url: Url,
}

impl ProviderTransport {
    pub(crate) fn get<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.client.get(url)
    }

    pub(crate) fn post<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.client.post(url)
    }

    pub(crate) fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder {
        self.client.request(method, url)
    }

    pub(crate) async fn send(
        &self,
        request: RequestBuilder,
        context: &str,
    ) -> Result<TransportResponse, String> {
        let mut first = request
            .build()
            .map_err(|err| format!("{context}失败: 构建请求异常: {err}"))?;
        let allows_retry = method_allows_replay(first.method());
        // Mutation bodies are never replayed, so do not duplicate them in memory.
        let retry_template = allows_retry.then(|| first.try_clone()).flatten();
        let mut applied_credentials = apply_cached_credentials(&mut first, &self.shield_context);

        let mut response = self.execute(first, context).await?;
        let mut solved = HashSet::new();
        let mut challenge_rounds = 0;

        loop {
            let Some(kind) = shield::detect(&response.headers, &response.body) else {
                if challenge_rounds > 0 || !applied_credentials.is_empty() {
                    shield::clear_challenge(&self.shield_context.provider_id);
                }
                return Ok(response);
            };

            shield::mark_challenge(&self.shield_context, kind, &response.url);
            // Only invalidate a credential that this response actually proved
            // stale. This lets concurrent first-time requests share a solver.
            if let Some(credential) = applied_credentials.get(&kind) {
                shield::invalidate_if_matches(
                    &self.shield_context,
                    &response.url,
                    kind,
                    credential,
                );
            }
            let hit =
                shield::hit_from_response(kind, &response.url, &response.headers, &response.body);

            if !allows_retry {
                // Aliyun's solver is pure local computation, so it is useful
                // to cache the result without replaying a mutating operation.
                if kind == shield::ShieldKind::AliyunWaf {
                    let _ =
                        shield::solve(&self.shield_context, &hit, shield::ChallengeMode::Silent)
                            .await?;
                }
                return Err(challenge_retry_required_message(kind));
            }

            if !solved.insert(kind) || challenge_rounds >= MAX_CHALLENGE_ROUNDS {
                return Err(shield_blocked_message(kind));
            }
            let Some(template) = retry_template.as_ref() else {
                return Err(format!(
                    "命中{}，请求正文无法安全复制，已停止自动重试。",
                    kind.label()
                ));
            };

            shield::solve(&self.shield_context, &hit, shield::ChallengeMode::Silent).await?;
            let mut retry = template
                .try_clone()
                .ok_or_else(|| format!("{context}失败: 请求正文无法复制，无法安全重试"))?;
            applied_credentials = apply_cached_credentials(&mut retry, &self.shield_context);
            response = self.execute(retry, context).await?;
            challenge_rounds += 1;
        }
    }

    pub(crate) async fn pass_challenge(&self) -> Result<(), String> {
        let state = shield::challenge_for(&self.shield_context.provider_id)
            .ok_or_else(|| "当前没有待处理的站点验证，请先让该中转站执行一次请求".to_string())?;
        shield::solve_interactively(&self.shield_context, &state).await?;
        Ok(())
    }

    async fn execute(&self, request: Request, context: &str) -> Result<TransportResponse, String> {
        let response = self
            .client
            .execute(request)
            .await
            .map_err(|err| format!("{context}失败: {err}"))?;
        let status = response.status();
        let headers = response.headers().clone();
        let url = response.url().clone();
        let body = network::read_http_text(response, context).await?;
        Ok(TransportResponse {
            status,
            headers,
            body,
            url,
        })
    }
}

fn method_allows_replay(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn apply_cached_credentials(
    request: &mut Request,
    context: &shield::ShieldContext,
) -> HashMap<shield::ShieldKind, shield::ShieldCredential> {
    let mut applied = HashMap::new();
    for (kind, credential) in shield::cached_credentials(context, request.url()) {
        let cookie_header = credential.cookie_header();
        apply_credential(request, &cookie_header, credential.user_agent.as_deref());
        applied.insert(kind, credential);
    }
    applied
}

fn apply_credential(request: &mut Request, shield_cookie: &str, user_agent: Option<&str>) {
    let headers = request.headers_mut();
    if let Some(value) = user_agent.and_then(|ua| ua.parse().ok()) {
        headers.insert(USER_AGENT, value);
    }
    let existing = headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let merged = merge_cookie_headers(&[existing.as_str(), shield_cookie]);
    if let Ok(value) = merged.parse() {
        headers.insert(COOKIE, value);
    }
}

/// Merge ordinary request cookies by name. Shield cookies are filtered before
/// reaching this function, so business authentication names cannot be added by
/// the shield cache.
pub(crate) fn merge_cookie_headers(cookie_sources: &[&str]) -> String {
    let mut pairs = std::collections::BTreeMap::new();
    for source in cookie_sources {
        for item in source
            .split(';')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let Some((name, value)) = item.split_once('=') else {
                continue;
            };
            let name = name.trim();
            let value = value.trim();
            if !name.is_empty() {
                pairs.insert(name.to_string(), format!("{name}={value}"));
            }
        }
    }
    pairs.into_values().collect::<Vec<_>>().join("; ")
}

pub(crate) fn shield_blocked_message(kind: shield::ShieldKind) -> String {
    if kind.may_need_interaction() {
        format!(
            "命中{}。请在该中转站卡片的「站点」菜单里点一次「通过站点验证」，完成后重新执行本次操作。",
            kind.label()
        )
    } else {
        format!("命中{}，自动过盾后仍未通过。", kind.label())
    }
}

fn challenge_retry_required_message(kind: shield::ShieldKind) -> String {
    if kind.may_need_interaction() {
        format!(
            "命中{}。为避免重复提交，本次操作未自动重试；请完成站点验证后重新执行。",
            kind.label()
        )
    } else {
        format!(
            "命中{}，验证凭证已准备好；为避免重复提交，请重新执行本次操作。",
            kind.label()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::mpsc::{self, Receiver},
        thread,
        time::{Duration, Instant},
    };

    #[test]
    fn method_policy_never_replays_mutations() {
        assert!(method_allows_replay(&Method::GET));
        assert!(method_allows_replay(&Method::HEAD));
        assert!(method_allows_replay(&Method::OPTIONS));
        assert!(!method_allows_replay(&Method::POST));
        assert!(!method_allows_replay(&Method::PUT));
        assert!(!method_allows_replay(&Method::PATCH));
        assert!(!method_allows_replay(&Method::DELETE));
    }

    #[test]
    fn merging_shield_cookie_does_not_drop_business_cookie() {
        assert_eq!(
            merge_cookie_headers(&["session=abc", "cf_clearance=xyz"]),
            "cf_clearance=xyz; session=abc"
        );
    }

    #[test]
    fn response_model_keeps_headers_and_url() {
        let mut headers = HeaderMap::new();
        headers.insert("cf-mitigated", HeaderValue::from_static("challenge"));
        let response = TransportResponse {
            status: StatusCode::FORBIDDEN,
            headers,
            body: "challenge".to_string(),
            url: Url::parse("https://example.test/api").unwrap(),
        };
        assert_eq!(response.headers["cf-mitigated"], "challenge");
        assert_eq!(response.url.path(), "/api");
    }

    #[tokio::test]
    async fn mutation_challenge_is_not_replayed() {
        let (url, requests, server) = mock_server(1);
        let transport = test_transport("mutation-provider");
        let request = transport
            .post(url)
            .header(COOKIE, "session=abc")
            .body("payload");

        let error = transport
            .send(request, "测试变更请求")
            .await
            .expect_err("mutation must stop after the challenge response");

        assert!(error.contains("重新执行"));
        let first = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(first.contains("POST "));
        assert!(first.contains("session=abc"));
        assert!(
            requests.try_recv().is_err(),
            "mutation request was replayed"
        );
        server.join().expect("mock server should finish");
    }

    #[tokio::test]
    async fn mutation_succeeds_on_explicit_retry_and_clears_challenge() {
        let (url, requests, server) = mock_server(2);
        let transport = test_transport("explicit-mutation-provider");

        let first_error = transport
            .send(
                transport
                    .post(url.clone())
                    .header(COOKIE, "session=abc")
                    .body("payload"),
                "测试变更请求",
            )
            .await
            .expect_err("the challenged mutation must require an explicit retry");
        assert!(first_error.contains("重新执行"));
        assert!(shield::challenge_for("explicit-mutation-provider").is_some());

        let response = transport
            .send(
                transport
                    .post(url)
                    .header(COOKIE, "session=abc")
                    .body("payload"),
                "测试变更请求",
            )
            .await
            .expect("the explicit retry should use the cached shield credential");

        assert_eq!(response.status, StatusCode::OK);
        assert!(shield::challenge_for("explicit-mutation-provider").is_none());
        let first = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        let second = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(first.contains("POST "));
        assert!(first.contains("session=abc"));
        assert!(second.contains("POST "));
        assert!(second.contains("session=abc"));
        assert!(second.contains("acw_tc=mock"));
        assert!(second.contains("acw_sc__v2="));
        assert!(requests.try_recv().is_err());
        server.join().expect("mock server should finish");
    }

    #[tokio::test]
    async fn idempotent_get_retries_with_shield_and_business_cookie() {
        let (url, requests, server) = mock_server(2);
        let transport = test_transport("get-provider");
        let request = transport
            .get(url)
            .header(COOKIE, "session=abc")
            .header(USER_AGENT, "business-agent");

        let response = transport
            .send(request, "测试读取请求")
            .await
            .expect("GET should succeed after the local shield solver");

        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.body, r#"{"ok":true}"#);
        let first = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        let second = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(first.contains("GET "));
        assert!(first.contains("session=abc"));
        assert!(second.contains("session=abc"));
        assert!(second.contains("acw_tc=mock"));
        assert!(second.contains("acw_sc__v2="));
        assert!(second.contains("business-agent"));
        server.join().expect("mock server should finish");
    }

    fn test_transport(provider_id: &str) -> ProviderTransport {
        ProviderTransport {
            client: Client::builder()
                .no_proxy()
                .build()
                .expect("test client should build"),
            shield_context: shield::ShieldContext::new(
                provider_id,
                format!("test-proxy-{provider_id}"),
                Ok(None),
            ),
        }
    }

    fn mock_server(expected_requests: usize) -> (Url, Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local mock server");
        listener
            .set_nonblocking(true)
            .expect("configure mock listener");
        let address = listener.local_addr().expect("read mock address");
        let (sender, receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            for index in 0..expected_requests {
                let mut stream = accept_with_timeout(&listener);
                let request = read_request(&mut stream);
                sender.send(request).expect("send captured request");
                let challenge = index == 0;
                write_response(stream, challenge);
            }
        });
        let url = Url::parse(&format!("http://{address}/api")).expect("build mock URL");
        (url, receiver, server)
    }

    fn accept_with_timeout(listener: &TcpListener) -> TcpStream {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .expect("configure accepted mock stream");
                    return stream;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "timed out waiting for mock request"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept mock request: {error}"),
            }
        }
    }

    fn read_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("configure mock stream");
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    bytes.extend_from_slice(&chunk[..read]);
                    if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                Err(error) => panic!("read mock request: {error}"),
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn write_response(mut stream: TcpStream, challenge: bool) {
        let body = if challenge {
            "<script>var arg1='0123456789abcdef0123456789abcdef01234567';</script>"
        } else {
            r#"{"ok":true}"#
        };
        let status = if challenge { "403 Forbidden" } else { "200 OK" };
        let cookie = if challenge {
            "Set-Cookie: acw_tc=mock; Path=/\r\n"
        } else {
            ""
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{cookie}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("write mock response");
    }
}
