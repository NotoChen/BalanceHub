// Read-only challenge detection shared by check-in and login. A widget stays
// under the user's control; its fresh response is the only completion signal.
export function pageNeedsVerification() {
  const title = window.__balancehubOriginalTitle ?? document.title;
  if (window._cf_chl_opt || document.querySelector('script[src*="/cdn-cgi/challenge-platform/"]')
    || ["Just a moment...", "正在验证…"].includes(title)) return true;
  const widget = document.querySelector('iframe[src*="challenges.cloudflare.com"], .cf-turnstile, .g-recaptcha, .h-captcha');
  if (!widget || !widget.getClientRects().length) return false;
  return ![...document.querySelectorAll('[name="cf-turnstile-response"], [name="g-recaptcha-response"], [name="h-captcha-response"]')]
    .some((input) => typeof input.value === "string" && input.value.trim());
}

// This function runs in the page. Every callback belongs to one widget instance,
// so a late callback from a previous verification cannot supply a new token.
export async function renderTurnstile({ siteKey, providerName, siteHost, windowTitle }) {
  if (window.__balancehubWidgetId !== undefined && typeof window.turnstile?.remove === "function") {
    window.turnstile.remove(window.__balancehubWidgetId);
  }
  const state = { token: "", error: "", interactive: false, expired: false, timedOut: false, unsupported: false };
  window.__balancehubVerification = state;
  document.title = windowTitle;
  document.documentElement.lang = "zh-CN";
  document.body.replaceChildren();
  document.body.style.cssText = "margin:0;font:14px/1.5 system-ui;background:#fafafa;color:#202124";
  const panel = document.createElement("main");
  panel.id = "balancehub-verification";
  panel.style.cssText = "box-sizing:border-box;width:max-content;min-width:332px;max-width:420px;padding:16px;margin:0 auto";
  const title = document.createElement("h1");
  title.style.cssText = "margin:0 0 4px;font:600 14px/20px system-ui;overflow-wrap:anywhere";
  title.textContent = `${providerName} · 签到验证`;
  const site = document.createElement("p");
  site.className = "verification-origin";
  site.style.cssText = "margin:0 0 12px;font:12px/18px system-ui;color:#666;overflow-wrap:anywhere";
  site.textContent = siteHost;
  const hint = document.createElement("p");
  hint.className = "verification-hint";
  hint.style.cssText = "margin:10px 0 0;font:12px/18px system-ui;color:#666";
  hint.textContent = "完成后自动继续签到，关闭窗口可取消。";
  const container = document.createElement("div");
  container.id = "balancehub-turnstile";
  container.style.cssText = "min-width:300px;min-height:65px";
  panel.append(title, site, container, hint);
  document.body.append(panel);
  if (!window.turnstile) {
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("verification_script_timeout")), 20_000);
      const script = document.createElement("script");
      script.src = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
      script.onload = () => { clearTimeout(timer); resolve(); };
      script.onerror = () => { clearTimeout(timer); reject(new Error("verification_script_failed")); };
      document.head.append(script);
    });
  }
  window.__balancehubWidgetId = window.turnstile.render(container, {
    sitekey: siteKey,
    retry: "never",
    "refresh-expired": "manual",
    "refresh-timeout": "manual",
    callback: (token) => { state.token = token; state.error = ""; },
    "before-interactive-callback": () => {
      state.interactive = true;
      hint.textContent = "请手动完成上方验证，完成后自动继续签到。";
    },
    "after-interactive-callback": () => { state.interactive = false; },
    "expired-callback": () => { state.token = ""; state.expired = true; },
    "timeout-callback": () => { state.token = ""; state.timedOut = true; },
    "unsupported-callback": () => { state.token = ""; state.unsupported = true; },
    "error-callback": (code) => { state.token = ""; state.error = String(code); return true; },
  });
}

export function verificationFailure(state) {
  if (!state) return "验证码组件未能初始化，请重试";
  if (state.unsupported || state.error === "110500") return "当前浏览器不受验证码服务支持，请换用其他浏览器或在站点原页面完成验证（110500）";
  if (state.expired) return "验证码已过期，请重新验证";
  if (state.timedOut || state.error === "110600" || state.error === "110620") return "人工验证超时，请重新验证";
  if (!state.error) return null;
  const code = /^[a-z0-9_-]{1,40}$/i.test(state.error) ? state.error : "unknown";
  if (["110100", "110110"].includes(code)) return `站点的验证码配置无效，请联系站点管理员（${code}）`;
  if (code === "110200") return `当前域名未获验证码服务授权，请联系站点管理员（${code}）`;
  if (code === "200500") return `验证码页面加载失败，请检查网络或代理（${code}）`;
  return `验证码验证失败（${code}），请重试；持续失败时请在站点原页面完成验证`;
}
