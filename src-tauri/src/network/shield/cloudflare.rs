//! Cloudflare 人机验证（Managed Challenge / Turnstile）求解。
//!
//! 与 AnyRouter 的阿里云盾不同：`acw_sc__v2` 是静态算法，可以在 Rust 里直接复刻；
//! Cloudflare 的挑战是轮转混淆的 JS + 浏览器指纹，没有"算出来"的可能。这里的做法
//! 是让应用自带的 WebView（真浏览器内核）过一次盾，把 `cf_clearance` 取出来交给
//! reqwest 复用。实测 clearance 在 reqwest（rustls）上被正常接受，无需伪装 TLS 指纹。
//!
//! 三级复用，越靠前越快：
//! 1. 进程内缓存（TTL 内直接命中，零开销）；
//! 2. WebView cookie 仓库（跨进程重启存活，重新收割约 600ms，无需交互）；
//! 3. 人工点选 Turnstile（仅在 CF 侧真正过期时发生）。
//!
//! `cf_clearance` 绑定 IP + User-Agent，因此 WebView 与 reqwest 必须使用同一个 UA
//! 常量——差一个字符 Cloudflare 就判废。

use super::{ShieldCredential, ShieldKind};
use reqwest::{header::HeaderMap, Url};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tokio::sync::{oneshot, Semaphore};

/// WebView 与业务请求共用的 User-Agent。
///
/// WKWebView 默认 UA 缺少 `Version/x Safari/x` 后缀，看起来就是个自动化外壳；
/// 这里补成各平台内核对应的完整浏览器 UA，避免自找升级成交互式挑战。
#[cfg(target_os = "macos")]
pub(crate) const WEBVIEW_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) ",
    "AppleWebKit/605.1.15 (KHTML, like Gecko) ",
    "Version/18.3 Safari/605.1.15"
);
#[cfg(target_os = "windows")]
pub(crate) const WEBVIEW_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ",
    "AppleWebKit/537.36 (KHTML, like Gecko) ",
    "Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0"
);
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) const WEBVIEW_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (X11; Linux x86_64) ",
    "AppleWebKit/605.1.15 (KHTML, like Gecko) ",
    "Version/18.3 Safari/605.1.15"
);

/// 静默收割的等待上限。WebView 仓库里已有 clearance 时通常几百毫秒就能拿到。
const SILENT_TIMEOUT: Duration = Duration::from_secs(8);
/// 用户主动触发时的静默确认窗口：只用于判断现有凭证是否还有效，无需等满。
const INTERACTIVE_SILENT_TIMEOUT: Duration = Duration::from_secs(3);
/// 显示窗口后等待人工点选 Turnstile 的上限。
const INTERACTIVE_TIMEOUT: Duration = Duration::from_secs(180);
const POLL_INTERVAL: Duration = Duration::from_millis(300);
const MAX_CONCURRENT_CHALLENGE_WINDOWS: usize = 2;

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// 在 setup 阶段注入 AppHandle。适配器层拿不到 AppHandle，且过盾按主机共享，
/// 与其把 AppHandle 沿十几个适配器函数签名往下传，不如按主机做成全局单例。
pub(crate) fn init(app: AppHandle) {
    let _ = APP_HANDLE.set(app);
}

pub(super) fn notify_provider_views_changed() {
    if let Some(app) = APP_HANDLE.get() {
        let _ = app.emit(crate::app_events::PROVIDERS_CHANGED_EVENT, ());
    }
}

/// 过盾时是否允许打断用户。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChallengeMode {
    /// 后台自动刷新：只做静默收割，需要人工点选时直接失败，绝不弹窗。
    Silent,
    /// 用户主动触发：静默拿不到就显示窗口，让用户点选 Turnstile。
    Interactive,
}

/// 检测响应是否是 Cloudflare 挑战页。
///
/// `cf-mitigated: challenge` 是权威信号；正文特征兜底，覆盖没有该头的旧规则。
/// 挑战页标题会被站点本地化（中文站渲染成"正在进行安全验证"），但原始 HTML 的
/// `Just a moment` 标题始终存在，本地化是页面内 JS 完成的。
pub(crate) fn matches(headers: &HeaderMap, body: &str) -> bool {
    let mitigated = headers
        .get("cf-mitigated")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("challenge"));
    if mitigated {
        return true;
    }

    let text = body.to_lowercase();
    (text.contains("just a moment") || text.contains("正在进行安全验证"))
        && (text.contains("cloudflare")
            || text.contains("cf-browser-verification")
            || text.contains("challenge-platform"))
}

/// 让应用自带的 WebView 真的过一次盾，取出白名单 Cookie。
pub(crate) async fn solve(
    url: &Url,
    mode: ChallengeMode,
    proxy_url: Option<&Url>,
) -> Result<ShieldCredential, String> {
    let _window_permit = challenge_window_gate()
        .acquire()
        .await
        .map_err(|_| "过盾窗口调度器已关闭".to_string())?;
    let app = APP_HANDLE
        .get()
        .ok_or_else(|| "过盾组件尚未初始化".to_string())?
        .clone();

    let window = open_window(&app, url, proxy_url).await?;
    let mut window_guard = ChallengeWindowGuard::new(window);
    let window = window_guard.window();
    // 用户随时可能关掉验证窗口；不盯着这个事件的话，轮询会一直读一个已销毁的
    // 窗口直到超时，把"用户放弃了"报成"验证超时"。
    let closed = Arc::new(AtomicBool::new(false));
    let closed_flag = closed.clone();
    window.on_window_event(move |event| {
        if matches!(
            event,
            WindowEvent::Destroyed | WindowEvent::CloseRequested { .. }
        ) {
            closed_flag.store(true, Ordering::Relaxed);
        }
    });

    let outcome = wait_for_clearance(window, url, mode, &closed).await;
    // Explicitly close on normal completion; the guard also closes the window
    // when the task is cancelled while waiting.
    window_guard.close();
    outcome
}

fn challenge_window_gate() -> &'static Semaphore {
    static GATE: OnceLock<Semaphore> = OnceLock::new();
    GATE.get_or_init(|| Semaphore::new(MAX_CONCURRENT_CHALLENGE_WINDOWS))
}

struct ChallengeWindowGuard {
    window: Option<WebviewWindow>,
}

impl ChallengeWindowGuard {
    fn new(window: WebviewWindow) -> Self {
        Self {
            window: Some(window),
        }
    }

    fn window(&self) -> &WebviewWindow {
        self.window.as_ref().expect("challenge window must exist")
    }

    fn close(&mut self) {
        if let Some(window) = self.window.take() {
            let _ = window.close();
        }
    }
}

impl Drop for ChallengeWindowGuard {
    fn drop(&mut self) {
        self.close();
    }
}

fn window_label(url: &Url) -> String {
    // 窗口 label 不接受主机名里的点；加序号避免上一个窗口尚未销毁时 label 冲突。
    static SEQ: OnceLock<Mutex<u64>> = OnceLock::new();
    let seq = {
        let counter = SEQ.get_or_init(|| Mutex::new(0));
        let mut guard = counter.lock().unwrap_or_else(|err| err.into_inner());
        *guard = guard.wrapping_add(1);
        *guard
    };
    let host = url
        .host_str()
        .unwrap_or("site")
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    format!("challenge-{host}-{seq}")
}

async fn open_window(
    app: &AppHandle,
    url: &Url,
    proxy_url: Option<&Url>,
) -> Result<WebviewWindow, String> {
    let label = window_label(url);
    let title = format!("正在通过 {} 的站点验证…", url.host_str().unwrap_or("站点"));
    let (sender, receiver) = oneshot::channel();
    let builder_app = app.clone();
    let builder_url = url.clone();
    let builder_proxy = proxy_url.cloned();

    // 窗口创建在 macOS 上必须发生在主线程；WebviewWindowBuilder 本身不是 Send，
    // 所以整个构造过程放进主线程闭包，只把结果（Send 的窗口句柄）传回来。
    app.run_on_main_thread(move || {
        let mut builder =
            WebviewWindowBuilder::new(&builder_app, &label, WebviewUrl::External(builder_url))
                .title(title)
                .inner_size(460.0, 620.0)
                .user_agent(WEBVIEW_USER_AGENT)
                // 先隐藏静默收割：WebView 仓库里已有 clearance 时几百毫秒就能拿到，
                // 没必要为此闪一个窗口。确认需要人工点选后再显示。
                .visible(false)
                .focused(false)
                .skip_taskbar(true);
        if let Some(proxy) = builder_proxy {
            builder = builder.proxy_url(proxy);
        }
        let result = builder
            .build()
            .map_err(|err| format!("创建过盾窗口失败: {err}"));
        // The solve future can be cancelled before the main-thread build runs.
        // In that case no guard will ever receive the window, so close it here.
        if let Err(Ok(window)) = sender.send(result) {
            let _ = window.close();
        }
    })
    .map_err(|err| format!("调度过盾窗口失败: {err}"))?;

    receiver
        .await
        .map_err(|_| "过盾窗口创建结果丢失".to_string())?
}

async fn wait_for_clearance(
    window: &WebviewWindow,
    url: &Url,
    mode: ChallengeMode,
    closed: &Arc<AtomicBool>,
) -> Result<ShieldCredential, String> {
    let started = std::time::Instant::now();
    let mut shown = false;
    // 用户主动触发时不必让他对着空屏幕干等：静默确认凭证已失效后立刻把窗口亮出来。
    let silent_budget = match mode {
        ChallengeMode::Silent => SILENT_TIMEOUT,
        ChallengeMode::Interactive => INTERACTIVE_SILENT_TIMEOUT,
    };

    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        if closed.load(Ordering::Relaxed) {
            return Err("站点验证已被取消：验证窗口被关闭".to_string());
        }

        // 页面状态才是权威信号：cookie 仓库里可能留着一张已失效但尚未被清除的
        // cf_clearance，只看"cookie 存在"会把死凭证当成过盾成功，拿去请求必然 403。
        // 只有 WebView 自己走出了挑战页，才说明这张 clearance 真的有效。
        let page = read_page_state(window).await;
        if page == Some(PageState::Cleared) {
            if let Some(cookies) = read_clearance_cookies(window, url).await? {
                return Ok(ShieldCredential::from_pairs(
                    ShieldKind::Cloudflare,
                    cookies,
                    // 必须与建窗时设置的 UA 逐字节一致：cf_clearance 绑定 IP + User-Agent。
                    Some(WEBVIEW_USER_AGENT.to_string()),
                ));
            }
        }

        let elapsed = started.elapsed();
        if elapsed >= silent_budget && !shown {
            if page == Some(PageState::Cleared) {
                // 页面正常但站点没有下发 clearance：这不是 Cloudflare 拦截，
                // 再显示窗口给用户点也无从下手。
                return Err("站点未下发 cf_clearance，可能并非 Cloudflare 拦截".to_string());
            }
            if mode == ChallengeMode::Silent {
                return Err(
                    "站点需要人工完成 Cloudflare 验证，请在中转站卡片上手动触发一次「通过站点验证」"
                        .to_string(),
                );
            }
            // 静默过不去说明需要点选 Turnstile，把窗口露出来让用户完成。
            shown = true;
            window
                .show()
                .map_err(|error| format!("显示站点验证窗口失败: {error}"))?;
            let _ = window.set_focus();
        }
        if elapsed >= INTERACTIVE_TIMEOUT {
            return Err("过盾超时：站点验证未在限定时间内完成".to_string());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageState {
    /// 仍停留在 Cloudflare 挑战页。
    Challenged,
    /// 已经是站点正常页面。
    Cleared,
}

/// 读取 WebView 当前页面是否仍是挑战页。
///
/// 页面跳转瞬间 `eval_with_callback` 的回调可能被丢弃，此时返回 None，
/// 由调用方按"未知"继续轮询，不能当成失败。
async fn read_page_state(window: &WebviewWindow) -> Option<PageState> {
    const SCRIPT: &str = "JSON.stringify({\
        title: document.title || '',\
        ready: document.readyState === 'complete',\
        widget: !!document.querySelector('#challenge-form,#challenge-running,#challenge-stage,.cf-turnstile,[id^=\"cf-chl\"]')\
    })";

    let (sender, receiver) = oneshot::channel();
    let sender = Mutex::new(Some(sender));
    window
        .eval_with_callback(SCRIPT, move |value| {
            if let Ok(mut guard) = sender.lock() {
                if let Some(sender) = guard.take() {
                    let _ = sender.send(value);
                }
            }
        })
        .ok()?;

    let raw = tokio::time::timeout(Duration::from_secs(3), receiver)
        .await
        .ok()?
        .ok()?;
    // 回调回传的是 JSON 序列化后的值，字符串本身还带一层引号。
    let inner = serde_json::from_str::<String>(&raw).unwrap_or(raw);
    let parsed = serde_json::from_str::<serde_json::Value>(&inner).ok()?;

    let title = parsed.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let widget = parsed
        .get("widget")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let ready = parsed
        .get("ready")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    classify_page_state(title, widget, ready)
}

fn classify_page_state(title: &str, widget: bool, ready: bool) -> Option<PageState> {
    if widget || is_challenge_title(title) {
        Some(PageState::Challenged)
    } else if ready {
        // API endpoints commonly render JSON with an empty document title. A
        // complete page without challenge DOM is still a valid cleared state.
        Some(PageState::Cleared)
    } else {
        None
    }
}

/// 挑战页标题会被 Cloudflare 按站点语言本地化，需要按各语言的固定文案识别。
fn is_challenge_title(title: &str) -> bool {
    let normalized = title.trim().to_lowercase();
    normalized.starts_with("just a moment")
        || normalized.starts_with("请稍候")
        || normalized.starts_with("请稍等")
        || normalized.contains("attention required")
        || normalized.contains("checking your browser")
}

/// 读取目标 URL 可用的 WebView Cookie。
///
/// 必须在主线程执行：wry 的 macOS 实现内部用嵌套 NSRunLoop 等待 `getAllCookies`
/// 回调，在后台线程上泵主 runloop 不会有任何效果，只会拿到空结果。Linux/Windows
/// 直接按 URL 读取；macOS 因 wry 的域名过滤缺陷读取全库后再由下方严格筛选。
async fn read_scoped_cookies(
    window: &WebviewWindow,
    target: &Url,
) -> Result<Vec<(String, String, String)>, String> {
    if target.host_str().is_none() {
        return Err("站点验证地址缺少主机名".to_string());
    }
    let app = APP_HANDLE
        .get()
        .ok_or_else(|| "过盾组件尚未初始化".to_string())?
        .clone();
    let (sender, receiver) = oneshot::channel();
    let window = window.clone();
    let target_url = target.clone();
    app.run_on_main_thread(move || {
        let cookies = if cfg!(target_os = "macos") {
            window.cookies()
        } else {
            window.cookies_for_url(target_url)
        };

        let result = cookies
            .map(|cookies| {
                cookies
                    .iter()
                    .map(|cookie| {
                        (
                            cookie.name().to_string(),
                            cookie.value().to_string(),
                            cookie.domain().unwrap_or_default().to_string(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .map_err(|err| format!("读取 WebView Cookie 失败: {err}"));
        let _ = sender.send(result);
    })
    .map_err(|err| format!("调度 Cookie 读取失败: {err}"))?;

    receiver
        .await
        .map_err(|_| "读取 WebView Cookie 结果丢失".to_string())?
}

/// 按主机匹配 Cookie。macOS 不能直接用 wry 的 `cookies_for_url`：它要求
/// `cookie.domain() == url.domain()` 完全相等，而 Cloudflare 下发的
/// `cf_clearance` domain 常带前导点（`.example.com`），会被整条过滤掉。
fn domain_matches(cookie_domain: &str, host: &str) -> bool {
    let cookie_domain = cookie_domain.trim_start_matches('.').to_ascii_lowercase();
    let host = host.to_ascii_lowercase();
    !cookie_domain.is_empty()
        && (host == cookie_domain || host.ends_with(&format!(".{cookie_domain}")))
}

async fn read_clearance_cookies(
    window: &WebviewWindow,
    url: &Url,
) -> Result<Option<BTreeMap<String, String>>, String> {
    let host = url.host_str().unwrap_or_default().to_string();
    let cookies = read_scoped_cookies(window, url).await?;

    let mut pairs = BTreeMap::new();
    let mut has_clearance = false;
    for (name, value, domain) in &cookies {
        if !domain_matches(domain, &host) {
            continue;
        }
        if name == "cf_clearance" && !value.trim().is_empty() {
            has_clearance = true;
        }
        if super::cookie_name_allowed(ShieldKind::Cloudflare, name) {
            pairs.insert(name.clone(), value.clone());
        }
    }

    Ok(has_clearance.then_some(pairs))
}

#[cfg(test)]
mod tests {
    use super::{classify_page_state, domain_matches, is_challenge_title, matches, PageState};
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn recognizes_localized_challenge_titles() {
        assert!(is_challenge_title("Just a moment..."));
        assert!(is_challenge_title("请稍候…"));
        assert!(!is_challenge_title(""));
        assert!(!is_challenge_title("BalanceHub 中转站"));
    }

    #[test]
    fn recognizes_completed_json_pages_without_a_title() {
        assert_eq!(
            classify_page_state("", false, true),
            Some(PageState::Cleared)
        );
        assert_eq!(classify_page_state("", false, false), None);
        assert_eq!(
            classify_page_state("", true, true),
            Some(PageState::Challenged)
        );
    }

    #[test]
    fn matches_cookie_domain_with_leading_dot() {
        // Cloudflare 下发的 cf_clearance 通常带前导点，必须能匹配上。
        assert!(domain_matches(".muyuan.do", "muyuan.do"));
        assert!(domain_matches(".muyuan.do", "api.muyuan.do"));
        assert!(!domain_matches(".other.com", "muyuan.do"));
        assert!(!domain_matches("", "muyuan.do"));
        // 后缀相同但不是子域，不能误判。
        assert!(!domain_matches(".uyuan.do", "muyuan.do"));
    }

    #[test]
    fn detects_challenge_by_header_or_localized_body() {
        let mut headers = HeaderMap::new();
        headers.insert("cf-mitigated", HeaderValue::from_static("challenge"));
        assert!(matches(&headers, ""));

        assert!(matches(
            &HeaderMap::new(),
            "正在进行安全验证 由 Cloudflare 提供的性能和安全服务"
        ));
        assert!(!matches(&HeaderMap::new(), r#"{"success":true}"#));
    }
}
