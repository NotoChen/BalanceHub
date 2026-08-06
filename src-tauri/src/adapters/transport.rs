use crate::{
    limits,
    models::{AppSettings, Provider},
    network::{self, shield},
};
use reqwest::{header::COOKIE, Client, IntoUrl, Method, Request, RequestBuilder, StatusCode, Url};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::Mutex as AsyncMutex;

/// Provider requests use a stable desktop browser UA for protocol compatibility.
pub(crate) const USER_AGENT_VALUE: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3 Safari/605.1.15";
const MAX_SHIELD_ROUNDS: usize = 2;

/// A provider-scoped HTTP client. Raw execution stays private so every provider
/// response passes through the same WAF detection and bounded retry policy.
#[derive(Clone)]
pub(crate) struct ProviderTransport {
    client: Client,
    shield_context: shield::ShieldContext,
    shield_state: Arc<AsyncMutex<ShieldAttemptState>>,
}

#[derive(Debug, Default)]
struct ShieldAttemptState {
    attempted: HashSet<ShieldAttemptKey>,
    blocked: HashMap<ShieldAttemptKey, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ShieldAttemptKey {
    origin: String,
    kind: shield::ShieldKind,
}

impl ShieldAttemptKey {
    fn new(url: &Url, kind: shield::ShieldKind) -> Self {
        Self {
            origin: url.origin().ascii_serialization(),
            kind,
        }
    }
}

pub(crate) async fn build_client(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<ProviderTransport, String> {
    let settings = settings.clone();
    let provider = provider.clone();
    tauri::async_runtime::spawn_blocking(move || build_client_blocking(&settings, &provider))
        .await
        .map_err(|err| format!("初始化中转站网络客户端任务异常: {err}"))?
}

fn build_client_blocking(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<ProviderTransport, String> {
    let proxy = network::resolve_proxy(settings, provider);
    let client = network::build_provider_client_with_proxy(proxy.clone())?;
    let shield_context =
        shield::ShieldContext::new(provider.identity.id.clone(), proxy.fingerprint());
    Ok(ProviderTransport {
        client,
        shield_context,
        shield_state: Arc::new(AsyncMutex::new(ShieldAttemptState::default())),
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
        if let Some(error) = self.blocked_shield_error(first.url()).await {
            return Err(error);
        }
        // A positively identified WAF page means the reverse proxy intercepted
        // the request before business code ran. Replaying a bounded request
        // once is therefore safe for reads and mutations.
        let retry_template = replay_template(&first);
        let mut applied_credentials = apply_cached_credentials(&mut first, &self.shield_context);

        let mut response = self.execute(first, context).await?;
        let mut solved = HashSet::new();
        let mut shield_rounds = 0;

        loop {
            let Some(kind) = shield::detect(&response.headers, &response.body) else {
                return Ok(response);
            };

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

            if !solved.insert(kind) || shield_rounds >= MAX_SHIELD_ROUNDS {
                let error = shield_blocked_message(kind);
                self.block_shield(&response.url, kind, error.clone()).await;
                return Err(error);
            }
            let Some(template) = retry_template.as_ref() else {
                let error = format!(
                    "命中{}，请求正文无法安全复制，已停止自动重试。",
                    kind.label()
                );
                self.block_shield(&response.url, kind, error.clone()).await;
                return Err(error);
            };

            self.solve_shield_once(&hit).await?;

            let mut retry = template
                .try_clone()
                .ok_or_else(|| format!("{context}失败: 请求正文无法复制，无法安全重试"))?;
            applied_credentials = apply_cached_credentials(&mut retry, &self.shield_context);
            response = self.execute(retry, context).await?;
            shield_rounds += 1;
        }
    }

    async fn solve_shield_once(&self, hit: &shield::ShieldHit) -> Result<(), String> {
        let key = ShieldAttemptKey::new(&hit.url, hit.kind);
        // Hold this guard while the deterministic solver runs. Other requests
        // for this provider reuse its cached result and never duplicate work.
        let mut state = self.shield_state.lock().await;
        if let Some(error) = state.blocked.get(&key) {
            return Err(error.clone());
        }
        if state.attempted.contains(&key) {
            if shield::cached_credentials(&self.shield_context, &hit.url)
                .into_iter()
                .any(|(cached_kind, _)| cached_kind == hit.kind)
            {
                return Ok(());
            }
            let error = shield_blocked_message(hit.kind);
            state.blocked.insert(key, error.clone());
            return Err(error);
        }
        state.attempted.insert(key.clone());

        match shield::solve(&self.shield_context, hit).await {
            Ok(_) => Ok(()),
            Err(error) => {
                state.blocked.insert(key, error.clone());
                Err(error)
            }
        }
    }

    async fn block_shield(&self, url: &Url, kind: shield::ShieldKind, error: String) {
        let key = ShieldAttemptKey::new(url, kind);
        self.shield_state.lock().await.blocked.insert(key, error);
    }

    async fn blocked_shield_error(&self, url: &Url) -> Option<String> {
        let state = self.shield_state.lock().await;
        state.blocked.iter().find_map(|(key, error)| {
            (key.origin == url.origin().ascii_serialization()).then(|| error.clone())
        })
    }

    pub(crate) async fn shield_blocked_for(&self, origin: &str) -> Option<String> {
        let url = Url::parse(origin).ok()?;
        self.blocked_shield_error(&url).await
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

/// Do not eagerly duplicate an unbounded or streaming request body just because
/// a reverse proxy might ask for a WAF challenge. All current business requests
/// are small JSON/form bodies; a future upload must fail with a clear retry
/// message instead of multiplying its memory footprint.
fn replay_template(request: &Request) -> Option<Request> {
    let body_is_bounded = request.body().is_none_or(|body| {
        body.as_bytes()
            .is_some_and(|bytes| bytes.len() <= limits::MAX_HTTP_REPLAY_BODY_BYTES)
    });
    body_is_bounded.then(|| request.try_clone()).flatten()
}

fn apply_cached_credentials(
    request: &mut Request,
    context: &shield::ShieldContext,
) -> HashMap<shield::ShieldKind, shield::ShieldCredential> {
    let mut applied = HashMap::new();
    for (kind, credential) in shield::cached_credentials(context, request.url()) {
        let cookie_header = credential.cookie_header();
        apply_credential(request, &cookie_header);
        applied.insert(kind, credential);
    }
    applied
}

fn apply_credential(request: &mut Request, shield_cookie: &str) {
    let headers = request.headers_mut();
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

fn shield_blocked_message(kind: shield::ShieldKind) -> String {
    format!("命中{}，自动过盾后仍未通过。", kind.label())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, USER_AGENT};
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::mpsc::{self, Receiver},
        thread,
        time::{Duration, Instant},
    };

    #[test]
    fn merging_shield_cookie_does_not_drop_business_cookie() {
        assert_eq!(
            merge_cookie_headers(&["session=abc", "acw_sc__v2=xyz"]),
            "acw_sc__v2=xyz; session=abc"
        );
    }

    #[test]
    fn response_model_keeps_headers_and_url() {
        let headers = HeaderMap::new();
        let response = TransportResponse {
            status: StatusCode::FORBIDDEN,
            headers,
            body: "waf challenge".to_string(),
            url: Url::parse("https://example.test/api").unwrap(),
        };
        assert!(response.headers.is_empty());
        assert_eq!(response.url.path(), "/api");
    }

    #[tokio::test]
    async fn mutation_shield_is_solved_and_replayed_automatically() {
        let (url, requests, server) = mock_server(2);
        let transport = test_transport("mutation-provider");
        let request = transport
            .post(url)
            .header(COOKIE, "session=abc")
            .body("payload");

        let response = transport
            .send(request, "测试变更请求")
            .await
            .expect("the shielded mutation should finish in one operation");

        assert_eq!(response.status, StatusCode::OK);
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
    async fn automatic_mutation_retry_uses_cached_shield() {
        let (url, requests, server) = mock_server(2);
        let transport = test_transport("explicit-mutation-provider");

        let response = transport
            .send(
                transport
                    .post(url)
                    .header(COOKIE, "session=abc")
                    .body("payload"),
                "测试变更请求",
            )
            .await
            .expect("the automatic retry should use the solved shield credential");

        assert_eq!(response.status, StatusCode::OK);
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

    #[tokio::test]
    async fn rejected_shield_is_not_retried_by_the_next_request() {
        let (url, requests, server) = repeating_shield_server();
        let transport = test_transport("rejected-shield-provider");

        let first_error = transport
            .send(transport.get(url.clone()), "第一次读取")
            .await
            .expect_err("a permanently challenged response must fail");
        assert!(first_error.contains("自动过盾后仍未通过"));

        let second_error = transport
            .send(transport.get(url), "第二次读取")
            .await
            .expect_err("the operation-level shield breaker must stay active");
        assert_eq!(second_error, first_error);
        let first_request = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        let retry_request = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(first_request.contains("GET "));
        assert!(retry_request.contains("GET "));
        assert!(retry_request.contains("acw_tc=mock"));
        assert!(retry_request.contains("acw_sc__v2="));
        assert!(requests.try_recv().is_err());
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
            ),
            shield_state: Arc::new(AsyncMutex::new(ShieldAttemptState::default())),
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
                let shield = index == 0;
                write_response(stream, shield);
            }
        });
        let url = Url::parse(&format!("http://{address}/api")).expect("build mock URL");
        (url, receiver, server)
    }

    fn repeating_shield_server() -> (Url, Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local mock server");
        listener
            .set_nonblocking(true)
            .expect("configure mock listener");
        let address = listener.local_addr().expect("read mock address");
        let (sender, receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let mut stream = accept_with_timeout(&listener);
                let request = read_request(&mut stream);
                sender.send(request).expect("send captured request");
                write_response(stream, true);
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

    fn write_response(mut stream: TcpStream, shield: bool) {
        let body = if shield {
            "<script>var arg1='0123456789abcdef0123456789abcdef01234567';</script>"
        } else {
            r#"{"ok":true}"#
        };
        let status = if shield { "403 Forbidden" } else { "200 OK" };
        let cookie = if shield {
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
