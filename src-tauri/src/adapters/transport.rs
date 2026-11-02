use crate::{
    limits,
    models::{
        provider_domain::endpoints::{
            apply_query_override, match_request, render_path, ProviderEndpointOverride,
        },
        AppSettings, Provider, ProviderProtocol,
    },
    network::{self, shield},
    util::unix_secs,
};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, COOKIE, ORIGIN, REFERER},
    Client, IntoUrl, Method, Request, RequestBuilder, StatusCode, Url,
};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    sync::{Arc, Mutex as StdMutex, OnceLock},
};
use tokio::sync::Mutex as AsyncMutex;

/// Provider requests use a desktop browser UA appropriate for the current
/// platform. A macOS Safari UA sent by Linux/Windows clients is rejected by
/// some WAFs and can also select the wrong response variant.
#[cfg(target_os = "macos")]
pub(crate) const USER_AGENT_VALUE: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3 Safari/605.1.15";
#[cfg(target_os = "windows")]
pub(crate) const USER_AGENT_VALUE: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub(crate) const USER_AGENT_VALUE: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const MAX_SHIELD_ROUNDS: usize = 2;
/// 主地址失败后保持备用的时长。到期后新的操作重新从主地址开始，避免主地址
/// 恢复后因为一张过期的失败记录永远停留在备用地址上。
const PRIMARY_RETRY_SECS: u64 = 10 * 60;

/// A provider-scoped HTTP client. Raw execution stays private so every provider
/// response passes through the same WAF detection and bounded retry policy.
#[derive(Clone)]
pub(crate) struct ProviderTransport {
    client: Client,
    auto_shield: bool,
    shield_context: shield::ShieldContext,
    shield_state: Arc<AsyncMutex<ShieldAttemptState>>,
    /// 主地址与备用地址的有序候选；回退只在这一层发生，适配器仍按填写的主
    /// 地址拼 URL。
    endpoints: EndpointFailover,
    /// 用户在卡片高级配置里声明的额外请求头，统一在发送前覆盖写入。
    headers: HeaderMap,
    /// 协议目录的默认请求约定与卡片按接口的改写。
    protocol: ProviderProtocol,
    endpoint_overrides: Arc<[ProviderEndpointOverride]>,
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

/// 主地址与备用地址的有序候选。请求 URL 以主地址为前缀时，连接失败或 5xx
/// 会按顺序改用下一个地址重发；落在其他来源的请求（OAuth 厂商接口、公告
/// 原文链接、下载地址）不参与回退。
#[derive(Debug, Clone, Default)]
struct EndpointFailover {
    provider_id: String,
    /// 规范化后的请求前缀，`[0]` 是主地址，其余按用户填写的顺序排列。
    bases: Arc<[String]>,
}

/// 某个中转站最近一次回退到的备用地址。只活在进程内：它是网络状况的读数，
/// 不属于用户配置，重启后重新探测即可。
#[derive(Debug, Clone)]
struct EndpointHealth {
    base: String,
    switched_at: u64,
}

fn endpoint_health() -> &'static StdMutex<HashMap<String, EndpointHealth>> {
    static HEALTH: OnceLock<StdMutex<HashMap<String, EndpointHealth>>> = OnceLock::new();
    HEALTH.get_or_init(Default::default)
}

/// 卡片展示用的备用地址读数：当前生效的是第几个备用地址、地址原文和切换时间。
pub(crate) struct ActiveBackup {
    pub(crate) backup_url: String,
    pub(crate) backup_index: usize,
    pub(crate) switched_at: u64,
}

/// 最近一次实际生效的备用地址。主地址正常时没有记录，卡片因此不会多写一句。
pub(crate) fn active_backup(provider: &Provider) -> Option<ActiveBackup> {
    let health = endpoint_health().lock().ok()?;
    let entry = health.get(&provider.identity.id)?;
    let (position, raw) = provider
        .identity
        .backup_urls
        .iter()
        .enumerate()
        .find(|(_, raw)| normalized_endpoint(raw).as_deref() == Some(entry.base.as_str()))?;
    Some(ActiveBackup {
        backup_url: raw.clone(),
        backup_index: position + 1,
        switched_at: entry.switched_at,
    })
}

/// 请求前缀与适配器拼接 URL 的写法保持一致：去掉首尾空白和结尾斜杠，再按
/// URL 规范统一大小写与默认路径，避免把同一个地址当成两个候选。
fn normalized_endpoint(raw: &str) -> Option<String> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    let url = Url::parse(text).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.username() != "" || url.password().is_some()
    {
        return None;
    }
    let normalized = url.as_str().trim_end_matches('/');
    (!normalized.is_empty()).then(|| normalized.to_string())
}

impl EndpointFailover {
    fn new(provider: &Provider) -> Self {
        let mut bases = Vec::with_capacity(provider.identity.backup_urls.len() + 1);
        if let Some(primary) = normalized_endpoint(&provider.identity.base_url) {
            bases.push(primary);
        }
        for raw in &provider.identity.backup_urls {
            let Some(base) = normalized_endpoint(raw) else {
                continue;
            };
            // 主地址为空时第一个备用地址就是唯一候选；重复项只保留一次。
            if bases.contains(&base) {
                continue;
            }
            bases.push(base);
        }
        Self {
            provider_id: provider.identity.id.clone(),
            bases: bases.into(),
        }
    }

    fn has_candidates(&self) -> bool {
        self.bases.len() > 1
    }

    /// 请求地址相对主地址的后半段；落在主地址之外时返回 `None`。
    fn relative<'a>(&self, url: &'a Url) -> Option<&'a str> {
        let primary = self.bases.first()?;
        let rest = url.as_str().strip_prefix(primary.as_str())?;
        (rest.is_empty() || rest.starts_with('/')).then_some(rest)
    }

    /// 本次操作首先尝试的候选。记录超过 `PRIMARY_RETRY_SECS` 后重新从主地址
    /// 开始，让恢复的主地址有机会被重新用上。
    fn preferred(&self) -> usize {
        let Ok(health) = endpoint_health().lock() else {
            return 0;
        };
        let Some(entry) = health.get(&self.provider_id) else {
            return 0;
        };
        if unix_secs().saturating_sub(entry.switched_at) > PRIMARY_RETRY_SECS {
            return 0;
        }
        self.bases
            .iter()
            .position(|base| base == &entry.base)
            .unwrap_or(0)
    }

    /// 尝试顺序：从当前生效的候选开始；备用地址生效期间不再回到主地址——
    /// 它刚失败过，同一轮操作里再试只是多等一次超时。
    fn order(&self) -> Vec<usize> {
        let preferred = self.preferred().min(self.bases.len().saturating_sub(1));
        if preferred == 0 {
            return (0..self.bases.len()).collect();
        }
        (preferred..self.bases.len()).chain(1..preferred).collect()
    }

    fn record(&self, index: usize) {
        if !self.has_candidates() {
            return;
        }
        let Ok(mut health) = endpoint_health().lock() else {
            return;
        };
        if index == 0 {
            health.remove(&self.provider_id);
            return;
        }
        let base = self.bases[index].clone();
        // 一直使用同一个备用地址时不刷新切换时间：提示的是「什么时候切的」。
        if health
            .get(&self.provider_id)
            .is_some_and(|entry| entry.base == base)
        {
            return;
        }
        health.insert(
            self.provider_id.clone(),
            EndpointHealth {
                base,
                switched_at: unix_secs(),
            },
        );
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
    let client = network::build_provider_client_with_proxy(settings, proxy.clone())?;
    let shield_context =
        shield::ShieldContext::new(provider.identity.id.clone(), proxy.fingerprint());
    Ok(ProviderTransport {
        client,
        auto_shield: provider.automation.auto_shield,
        shield_context,
        shield_state: Arc::new(AsyncMutex::new(ShieldAttemptState::default())),
        endpoints: EndpointFailover::new(provider),
        headers: provider_header_map(provider),
        protocol: provider.identity.protocol,
        endpoint_overrides: provider.request.endpoints.clone().into(),
    })
}

fn provider_header_map(provider: &Provider) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for header in provider.request.headers.iter().filter(|h| h.is_usable()) {
        let name = header.name.trim();
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(header.value.trim()),
        ) {
            headers.insert(name, value);
        }
    }
    headers
}

#[derive(Debug, Clone)]
pub(crate) struct TransportResponse {
    pub status: StatusCode,
    pub headers: reqwest::header::HeaderMap,
    pub body: String,
    pub url: Url,
}

impl ProviderTransport {
    /// OAuth account APIs must not replay rotating credentials or start automatic
    /// WAF interactions. The client still shares the provider proxy resolution.
    pub(crate) fn non_replaying_client(&self) -> &Client {
        &self.client
    }
    pub(crate) fn get<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.client.get(url)
    }

    pub(crate) fn post<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.client.post(url)
    }

    pub(crate) fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder {
        self.client.request(method, url)
    }

    /// 按候选顺序发送请求：主地址连接失败、过盾没有通过或返回 5xx 时改用下一个
    /// 地址重发。非幂等的变更请求同样会回退——备用地址是同一个站点的另一个域名，
    /// 这几种失败都发生在业务代码运行之前；与过盾重放同一套边界：请求正文必须
    /// 可复制，且每个地址最多尝试一次。
    pub(crate) async fn send(
        &self,
        request: RequestBuilder,
        context: &str,
    ) -> Result<TransportResponse, String> {
        let mut first = request
            .build()
            .map_err(|err| format!("{context}失败: 构建请求异常: {err}"))?;
        self.apply_headers(&mut first);
        self.apply_endpoint_override(&mut first);
        // 失败时把实际发出的方法和路径带回界面，用户可以直接对照接口目录排查。
        let context = format!("{context}（{} {}）", first.method(), first.url().path());
        if self.endpoints.relative(first.url()).is_none() {
            return self.send_once(first, &context).await;
        }
        let order = self.endpoints.order();
        let primary = self.endpoints.bases[0].clone();
        // 每次重发之前 `first` 的地址都停在上一个候选上，重写前缀必须从当前
        // 地址出发，不能一直按主地址拼——否则第二个备用地址永远轮不到。
        let mut current = primary;
        let mut response = None;
        let mut error = None;
        for (position, index) in order.iter().enumerate() {
            let target = self.endpoints.bases[*index].clone();
            if target != current {
                if !rebase_request(&mut first, &current, &target) {
                    break;
                }
                current = target;
            }
            let retry = if position + 1 < order.len() {
                first.try_clone()
            } else {
                None
            };
            match self.send_once(first, &context).await {
                Ok(candidate) if !endpoint_failed(&candidate) => {
                    self.endpoints.record(*index);
                    return Ok(candidate);
                }
                Ok(candidate) => {
                    // 所有地址都返回 5xx 时把最高优先级的那个响应交回业务代码：
                    // 状态码和正文比一句“连接失败”更能说明问题。
                    response.get_or_insert(candidate);
                }
                Err(candidate) => {
                    error.get_or_insert(candidate);
                }
            }
            match retry {
                Some(retry) => first = retry,
                None => break,
            }
        }
        // `relative` 已经保证了半段路径可拼接；这里只剩“所有候选都失败”。
        response
            .ok_or_else(|| error.unwrap_or_else(|| format!("{context}失败: 没有可用的中转站地址")))
    }

    /// 自定义请求头覆盖写在构建好的请求上：适配器自己设置的同名头（例如
    /// User-Agent）以用户声明为准，且不会出现重复头。
    fn apply_headers(&self, request: &mut Request) {
        for (name, value) in &self.headers {
            request.headers_mut().insert(name, value.clone());
        }
    }

    /// 按接口改写路径、查询参数和请求头。匹配使用相对主地址的路径；改写后
    /// 的地址仍是同一站点的请求，因此继续参与备用地址回退和过盾。
    fn apply_endpoint_override(&self, request: &mut Request) {
        if self.endpoint_overrides.is_empty() {
            return;
        }
        let Some(relative) = self.endpoints.relative(request.url()).map(|rest| {
            rest.split(['?', '#'])
                .next()
                .unwrap_or_default()
                .to_string()
        }) else {
            return;
        };
        let url = request.url().clone();
        let query = url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        let Some(matched) =
            match_request(self.protocol, request.method().as_str(), &relative, &query)
        else {
            return;
        };
        let Some(over) = self
            .endpoint_overrides
            .iter()
            .find(|over| over.operation == matched.spec.id)
        else {
            return;
        };
        let mut next = url.clone();
        let path = over.path.trim();
        if !path.is_empty() {
            let rendered = render_path(path, &matched.params);
            let target = self
                .endpoints
                .bases
                .first()
                .and_then(|base| Url::parse(&format!("{base}{rendered}")).ok());
            if let Some(parsed) = target {
                // 默认查询参数属于接口本身，改路径时保留；要调整请用参数表。
                next = parsed;
                next.set_query(url.query());
            }
        }
        apply_query_override(&mut next, &over.query, &matched.params);
        if next.as_str() != url.as_str() {
            *request.url_mut() = next;
        }
        for header in over.headers.iter().filter(|header| header.is_usable()) {
            let name = header.name.trim();
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(header.value.trim()),
            ) {
                request.headers_mut().insert(name, value);
            }
        }
    }

    /// 针对已确定的地址完成一次请求（含过盾与过盾重试）。
    async fn send_once(
        &self,
        mut first: Request,
        context: &str,
    ) -> Result<TransportResponse, String> {
        if !self.auto_shield {
            strip_shield_cookies(&mut first);
            return self.execute(first, context).await;
        }
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
            .map_err(|err| format_request_error(context, &err))?;
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

/// 把请求从主地址换到候选地址：URL 只替换前缀，`Origin` / `Referer` 这些由
/// 主地址拼出来的请求头一起改，备用地址收到的仍是自洽的一组来源信息。
fn rebase_request(request: &mut Request, from: &str, to: &str) -> bool {
    let Some(relative) = request.url().as_str().strip_prefix(from) else {
        return false;
    };
    let Ok(url) = Url::parse(&format!("{to}{relative}")) else {
        return false;
    };
    *request.url_mut() = url;
    for name in [ORIGIN, REFERER] {
        let Some(value) = request
            .headers()
            .get(&name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix(from))
            .map(str::to_string)
        else {
            continue;
        };
        if let Ok(rewritten) = format!("{to}{value}").parse() {
            request.headers_mut().insert(name, rewritten);
        }
    }
    true
}

/// 5xx 说明地址背后的服务不可用。WAF 挑战页也可能带 5xx，但那是过盾流程的
/// 输入，不是地址故障，交给 `send_once` 处理。
fn endpoint_failed(response: &TransportResponse) -> bool {
    response.status.is_server_error() && shield::detect(&response.headers, &response.body).is_none()
}

fn format_request_error(context: &str, error: &reqwest::Error) -> String {
    let mut causes = Vec::new();
    let mut source = error.source();
    while let Some(cause) = source {
        let detail = cause.to_string();
        if !detail.is_empty() && causes.last() != Some(&detail) {
            causes.push(detail);
        }
        source = cause.source();
    }

    if causes.is_empty() {
        format!("{context}失败: {error}")
    } else {
        format!("{context}失败: {error}; 底层原因: {}", causes.join(" -> "))
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

fn strip_shield_cookies(request: &mut Request) {
    let Some(cookie) = request
        .headers()
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
    else {
        return;
    };
    let cookie = cookie
        .split(';')
        .filter(|pair| {
            let name = pair.trim().split('=').next().unwrap_or_default();
            !matches!(name, "cf_clearance" | "__cf_bm" | "acw_tc" | "acw_sc__v2")
                && !name.starts_with("cf_chl_")
        })
        .collect::<Vec<_>>()
        .join(";");
    request.headers_mut().remove(COOKIE);
    if !cookie.trim().is_empty() {
        if let Ok(value) = cookie.trim().parse() {
            request.headers_mut().insert(COOKIE, value);
        }
    }
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
    use crate::models::{provider_domain::endpoints::ProviderEndpointQuery, ProviderHeader};
    use reqwest::header::{HeaderMap, USER_AGENT};
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::mpsc::{self, Receiver},
        thread,
        time::{Duration, Instant},
    };

    #[tokio::test]
    async fn check_in_disabled_shield_does_not_inject_cached_cookies_or_solve() {
        let (url, requests, server) = mock_server(3);
        let mut transport = test_transport("disabled-shield-provider");
        transport
            .send(transport.get(url.clone()), "fixture")
            .await
            .unwrap();
        transport.auto_shield = false;
        transport
            .send(
                transport
                    .get(url)
                    .header(COOKIE, "session=fixture; cf_clearance=old-fixture"),
                "fixture",
            )
            .await
            .unwrap();
        let _first = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        let retried = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        let disabled = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(retried.contains("acw_sc__v2="));
        assert!(!disabled.contains("acw_sc__v2="));
        assert!(!disabled.contains("cf_clearance="));
        assert!(disabled.contains("session=fixture"));
        server.join().unwrap();

        let (url, requests, server) = mock_server(1);
        transport.send(transport.get(url), "fixture").await.unwrap();
        assert!(!requests
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .contains("acw_sc__v2="));
        assert!(requests.try_recv().is_err());
        server.join().unwrap();
    }

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

    #[tokio::test]
    async fn a_dead_primary_falls_back_to_the_backup_and_reports_it() {
        let (backup, requests, server) = static_server("200 OK", r#"{"ok":true}"#, 2);
        let backup_base = base_of(&backup);
        let provider = failover_provider(
            "failover-dead-primary",
            "http://127.0.0.1:1",
            &[backup_base.as_str()],
        );
        let transport = failover_transport(&provider);

        let response = transport
            .send(
                transport.get("http://127.0.0.1:1/api/user/self"),
                "测试读取",
            )
            .await
            .expect("备用地址应该接住主地址连不上的请求");
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.url.host_str(), backup.host_str());
        let active = active_backup(&provider).expect("卡片应读到当前生效的备用地址");
        assert_eq!(active.backup_index, 1);
        assert_eq!(active.backup_url, backup_base);

        // 备用地址生效期间，同一张卡的下一次请求直接从备用地址开始。
        transport
            .send(
                transport.get("http://127.0.0.1:1/api/user/self"),
                "测试读取",
            )
            .await
            .expect("备用地址继续可用");
        let first = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        let second = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(first.contains("GET /api/user/self "));
        assert!(second.contains("GET /api/user/self "));
        server.join().expect("mock server should finish");
    }

    #[tokio::test]
    async fn the_next_backup_is_tried_after_the_previous_one_fails() {
        let (live_backup, live_requests, live_server) =
            static_server("200 OK", r#"{"ok":true}"#, 1);
        let live_base = base_of(&live_backup);
        // 第一个备用地址同样连不上：第二个备用地址必须还有机会。每个候选的
        // 地址前缀各不相同，重写 URL 只能从上一个候选出发。
        let provider = failover_provider(
            "failover-second-backup",
            "http://127.0.0.1:1",
            &["http://127.0.0.1:2", live_base.as_str()],
        );
        let transport = failover_transport(&provider);

        let response = transport
            .send(
                transport.get("http://127.0.0.1:1/api/user/self"),
                "测试读取",
            )
            .await
            .expect("第二个备用地址应该接住请求");
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.url.host_str(), live_backup.host_str());
        let active = active_backup(&provider).expect("卡片应读到当前生效的备用地址");
        assert_eq!(active.backup_index, 2);
        assert_eq!(active.backup_url, live_base);
        assert!(live_requests
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .contains("GET /api/user/self "));
        live_server.join().expect("mock server should finish");
    }

    #[tokio::test]
    async fn server_errors_fall_back_and_rewrite_the_origin_headers() {
        let (primary, primary_requests, primary_server) = static_server("500 Internal", "{}", 1);
        let (backup, backup_requests, backup_server) = static_server("200 OK", r#"{"ok":true}"#, 1);
        let primary_base = base_of(&primary);
        let backup_base = base_of(&backup);
        let provider = failover_provider(
            "failover-server-error",
            primary_base.as_str(),
            &[backup_base.as_str()],
        );
        let transport = failover_transport(&provider);

        let response = transport
            .send(
                transport
                    .get(format!("{primary_base}/api/user/login"))
                    .header(ORIGIN, primary_base.as_str())
                    .header(REFERER, format!("{primary_base}/")),
                "测试登录",
            )
            .await
            .expect("5xx 的地址应让位给下一个候选");
        assert_eq!(response.status, StatusCode::OK);
        assert!(primary_requests
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .contains("GET "));
        // 备用地址收到的来源信息必须指向它自己，而不是主地址。
        let rebound = backup_requests
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .to_lowercase();
        assert!(rebound.contains(&format!("origin: {backup_base}")));
        assert!(rebound.contains(&format!("referer: {backup_base}/")));
        assert_eq!(
            active_backup(&provider).map(|active| active.backup_index),
            Some(1)
        );
        primary_server.join().expect("primary server should finish");
        backup_server.join().expect("backup server should finish");
    }

    #[tokio::test]
    async fn a_healthy_primary_never_touches_the_backup() {
        let (primary, _primary_requests, primary_server) =
            static_server("200 OK", r#"{"ok":true}"#, 1);
        let (backup, backup_requests, backup_server) = static_server("200 OK", "{}", 0);
        let primary_base = base_of(&primary);
        let provider = failover_provider(
            "failover-healthy-primary",
            primary_base.as_str(),
            &[base_of(&backup).as_str()],
        );
        let transport = failover_transport(&provider);

        let response = transport
            .send(
                transport.get(format!("{primary_base}/api/status")),
                "测试读取",
            )
            .await
            .unwrap();
        assert_eq!(response.url.host_str(), primary.host_str());
        assert!(active_backup(&provider).is_none());
        assert!(backup_requests.try_recv().is_err());
        primary_server.join().expect("primary server should finish");
        backup_server.join().expect("backup server should finish");
    }

    #[tokio::test]
    async fn client_errors_stay_on_the_address_that_answered() {
        let (primary, _primary_requests, primary_server) =
            static_server("404 Not Found", r#"{"error":"missing"}"#, 1);
        let (backup, backup_requests, backup_server) = static_server("200 OK", "{}", 0);
        let primary_base = base_of(&primary);
        let provider = failover_provider(
            "failover-client-error",
            primary_base.as_str(),
            &[base_of(&backup).as_str()],
        );
        let transport = failover_transport(&provider);

        let response = transport
            .send(
                transport.get(format!("{primary_base}/api/unknown")),
                "测试读取",
            )
            .await
            .unwrap();
        assert_eq!(response.status, StatusCode::NOT_FOUND);
        assert!(active_backup(&provider).is_none());
        assert!(backup_requests.try_recv().is_err());
        primary_server.join().expect("primary server should finish");
        backup_server.join().expect("backup server should finish");
    }

    #[tokio::test]
    async fn an_expired_switch_retries_the_primary_first() {
        let (primary, _primary_requests, primary_server) =
            static_server("200 OK", r#"{"ok":true}"#, 1);
        let (backup, backup_requests, backup_server) = static_server("200 OK", "{}", 0);
        let primary_base = base_of(&primary);
        let backup_base = base_of(&backup);
        let provider = failover_provider(
            "failover-expired-switch",
            primary_base.as_str(),
            &[backup_base.as_str()],
        );
        // 很久以前的切换记录已经过期，主地址应该重新获得一次机会。
        endpoint_health().lock().unwrap().insert(
            provider.identity.id.clone(),
            EndpointHealth {
                base: backup_base,
                switched_at: 0,
            },
        );
        let transport = failover_transport(&provider);

        let response = transport
            .send(
                transport.get(format!("{primary_base}/api/status")),
                "测试读取",
            )
            .await
            .unwrap();
        assert_eq!(response.url.host_str(), primary.host_str());
        assert!(active_backup(&provider).is_none());
        assert!(backup_requests.try_recv().is_err());
        primary_server.join().expect("primary server should finish");
        backup_server.join().expect("backup server should finish");
    }

    #[tokio::test]
    async fn endpoint_overrides_rewrite_the_matched_request() {
        let (base, requests, server) = static_server("200 OK", r#"{"ok":true}"#, 1);
        let base_url = base_of(&base);
        let mut provider = failover_provider("endpoint-override", base_url.as_str(), &[]);
        provider.request.endpoints = vec![ProviderEndpointOverride {
            operation: "check_in".into(),
            path: "/api/user/sign_in".into(),
            query: vec![ProviderEndpointQuery {
                name: "turnstile".into(),
                value: "1".into(),
            }],
            headers: vec![ProviderHeader {
                name: "X-Shield".into(),
                value: "1".into(),
            }],
        }];
        let transport = failover_transport(&provider);

        // 卡片上还是默认签到路径；目录把它匹配为 check_in，改写到站点实际接口。
        transport
            .send(
                transport.post(format!("{base_url}/api/user/checkin?month=2026-10")),
                "签到",
            )
            .await
            .unwrap();
        let request = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(request.starts_with("POST /api/user/sign_in?"), "{request}");
        // 默认查询参数保留，新增参数追加在后面。
        assert!(request.contains("month=2026-10"), "{request}");
        assert!(request.contains("turnstile=1"), "{request}");
        assert!(
            request.to_ascii_lowercase().contains("x-shield: 1"),
            "{request}"
        );
        server.join().expect("mock server should finish");
    }

    #[tokio::test]
    async fn endpoint_overrides_leave_other_requests_alone() {
        let (base, requests, server) = static_server("200 OK", r#"{"ok":true}"#, 1);
        let base_url = base_of(&base);
        let mut provider = failover_provider("endpoint-untouched", base_url.as_str(), &[]);
        provider.request.endpoints = vec![ProviderEndpointOverride {
            operation: "check_in".into(),
            path: "/api/user/sign_in".into(),
            query: Vec::new(),
            headers: Vec::new(),
        }];
        let transport = failover_transport(&provider);

        // 没有匹配到改写条目时，路径、参数和请求头都保持原样。
        transport
            .send(
                transport.get(format!("{base_url}/api/status?fixture=1")),
                "站点状态",
            )
            .await
            .unwrap();
        let request = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            request.starts_with("GET /api/status?fixture=1"),
            "{request}"
        );
        assert!(
            !request.to_ascii_lowercase().contains("x-shield"),
            "{request}"
        );
        server.join().expect("mock server should finish");
    }

    #[tokio::test]
    async fn request_errors_name_the_method_and_path() {
        let provider = failover_provider("error-context", "http://127.0.0.1:1", &[]);
        let transport = failover_transport(&provider);

        // 连接失败时错误要带上实际发出的方法与路径，方便对照接口目录排查。
        let error = transport
            .send(transport.get("http://127.0.0.1:1/api/status"), "站点状态")
            .await
            .unwrap_err();
        assert!(error.contains("站点状态"), "{error}");
        assert!(error.contains("GET /api/status"), "{error}");
    }

    #[tokio::test]
    async fn requests_outside_the_provider_base_do_not_fall_back() {
        let (other, _other_requests, other_server) = static_server("200 OK", r#"{"ok":true}"#, 1);
        let (backup, backup_requests, backup_server) = static_server("200 OK", "{}", 0);
        let provider = failover_provider(
            "failover-foreign-request",
            "http://127.0.0.1:1",
            &[base_of(&backup).as_str()],
        );
        let transport = failover_transport(&provider);

        let response = transport
            .send(
                transport.get(format!("{}/feed.xml", base_of(&other))),
                "测试读取",
            )
            .await
            .unwrap();
        assert_eq!(response.url.host_str(), other.host_str());
        assert!(backup_requests.try_recv().is_err());
        other_server.join().expect("mock server should finish");
        backup_server.join().expect("backup server should finish");
    }

    fn base_of(url: &Url) -> String {
        url.as_str().trim_end_matches('/').to_string()
    }

    fn failover_transport(provider: &Provider) -> ProviderTransport {
        build_client_blocking(&AppSettings::default(), provider).expect("transport should build")
    }

    fn failover_provider(id: &str, base_url: &str, backups: &[&str]) -> Provider {
        let mut input = crate::models::ProviderInput::default();
        input.identity.base_url = base_url.to_string();
        input.identity.backup_urls = backups.iter().map(|url| url.to_string()).collect();
        input.auth.mode = crate::models::AuthMode::ApiKey;
        input.auth.api_key = "sk-fixture-key".into();
        input.automation.auto_shield = false;
        input.proxy.mode = crate::models::ProviderProxyMode::NoProxy;
        Provider::from_input(input, id.to_string())
    }

    fn static_server(
        status: &'static str,
        body: &'static str,
        expected_requests: usize,
    ) -> (Url, Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local mock server");
        listener
            .set_nonblocking(true)
            .expect("configure mock listener");
        let address = listener.local_addr().expect("read mock address");
        let (sender, receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            for _ in 0..expected_requests {
                let mut stream = accept_with_timeout(&listener);
                let request = read_request(&mut stream);
                sender.send(request).expect("send captured request");
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write mock response");
            }
        });
        let url = Url::parse(&format!("http://{address}")).expect("build mock URL");
        (url, receiver, server)
    }

    fn test_transport(provider_id: &str) -> ProviderTransport {
        ProviderTransport {
            auto_shield: true,
            client: Client::builder()
                .no_proxy()
                .build()
                .expect("test client should build"),
            shield_context: shield::ShieldContext::new(
                provider_id,
                format!("test-proxy-{provider_id}"),
            ),
            shield_state: Arc::new(AsyncMutex::new(ShieldAttemptState::default())),
            endpoints: EndpointFailover::default(),
            headers: HeaderMap::new(),
            protocol: ProviderProtocol::NewApi,
            endpoint_overrides: Arc::from([]),
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
