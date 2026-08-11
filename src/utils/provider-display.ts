// 本文件只保留前端展示格式化。业务能力读取集中在 provider-actions.ts。
import type { AuthMode, Provider, ProviderProtocol, ProviderQuotaDisplay } from "../stores/providers";

export type ProviderCardTone =
  | "disabled"
  | "pending"
  | "error"
  | "warning"
  | "empty"
  | "ok"
  | "syncing";

const providerAuthModeLabels: Record<AuthMode, string> = {
  session: "Cookie",
  accessToken: "访问令牌",
  apiKey: "API Key",
  password: "账号密码",
};

export function providerAuthModeLabel(provider: Provider) {
  if (provider.identity.protocol === "sub2Api" && provider.auth.mode === "accessToken") {
    return "Access Token";
  }
  return providerAuthModeLabels[provider.auth.mode];
}

export function providerProtocolLabel(protocol: ProviderProtocol) {
  if (protocol === "sub2Api") return "Sub2API";
  if (protocol === "api") return "API";
  return "NewAPI";
}

export function providerQuotaKnown(provider: Provider) {
  return provider.quota.known !== false;
}

export function providerQuotaTotalKnown(provider: Provider) {
  return provider.quota.totalKnown !== false;
}

export function providerAuthModeDescription(provider: Provider) {
  if (provider.identity.protocol === "api") {
    return "当前使用 API Key 调用模型接口";
  }
  if (provider.identity.protocol === "sub2Api") {
    switch (provider.auth.mode) {
      case "password":
        return "当前使用账号密码登录 Sub2API，缓存 Access / Refresh Token";
      case "accessToken":
        return provider.auth.refreshToken.trim()
          ? "当前使用 Sub2API Access Token（含 Refresh Token，可自动续期）"
          : "当前使用 Sub2API Access Token（无 Refresh Token，过期需重新获取）";
      case "apiKey":
        return "当前使用 Sub2API 网关 API Key";
      case "session":
        break;
    }
  }
  switch (provider.auth.mode) {
    case "password":
      return "当前优先使用账号密码登录，并建立可复用会话";
    case "session":
      return "当前使用 Cookie 获取账号额度和账号能力";
    case "accessToken":
      return "当前使用访问令牌获取账号额度和账号能力";
    case "apiKey":
      return "当前使用 API Key 获取该 Key 的额度";
  }
}

export function formatNumberCompact(value: number, fractionDigits = 2) {
  return new Intl.NumberFormat("en-US", {
    maximumFractionDigits: fractionDigits,
    minimumFractionDigits: fractionDigits,
  }).format(value);
}

export function formatQuotaValue(value: number, quotaDisplay: ProviderQuotaDisplay) {
  const displayType = quotaDisplay.quotaDisplayType || "currency";
  if (displayType.toLowerCase() === "tokens") {
    return formatNumberCompact(value, 0);
  }
  const symbol = normalizeCurrencySymbol(displayType, quotaDisplay.currencySymbol);
  return `${symbol}${formatNumberCompact(value)}`;
}

function normalizeCurrencySymbol(displayType: string, value: string) {
  const knownSymbol = knownCurrencySymbol(displayType);
  if (knownSymbol) {
    return knownSymbol;
  }

  const symbol = value.trim();
  if (symbol && symbol !== "¤") {
    return symbol;
  }
  return "$";
}

function knownCurrencySymbol(displayType: string) {
  switch (displayType.trim().toUpperCase()) {
    case "USD":
    case "US_DOLLAR":
    case "US DOLLAR":
      return "$";
    case "CNY":
    case "RMB":
    case "CNH":
    case "YUAN":
    case "人民币":
      return "¥";
    default:
      return "";
  }
}

export function formatProviderQuota(provider: Provider, value: number) {
  return formatQuotaValue(value, {
    quotaDisplayType: provider.quota.displayType || "currency",
    currencySymbol: provider.quota.currencySymbol || "$",
  });
}

export function formatProviderSyncTime(value: string | null | undefined) {
  if (!value) return "";
  const raw = Number(value);
  const date = Number.isFinite(raw)
    ? new Date(raw < 1_000_000_000_000 ? raw * 1000 : raw)
    : new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(date);
}

export function providerQuotaUnlimited(provider: Provider) {
  return provider.quota.unlimited === true;
}

export function providerQuotaScopeLabel(provider: Provider) {
  if (provider.identity.protocol === "api") {
    return "API Key 可用额度";
  }
  return provider.quota.scope === "token" ? "API Key 可用额度" : "账号可用额度";
}

export function providerTotalQuotaLabel(provider: Provider) {
  if (!providerQuotaKnown(provider)) {
    return "未公开";
  }
  if (!providerQuotaTotalKnown(provider)) {
    return "未公开";
  }
  if (providerQuotaUnlimited(provider)) {
    return "∞";
  }
  return formatProviderQuota(provider, totalQuota(provider));
}

export function providerAvailableQuotaLabel(provider: Provider) {
  if (!providerQuotaKnown(provider)) {
    return "未公开";
  }
  if (providerQuotaUnlimited(provider)) {
    return "∞";
  }
  return formatProviderQuota(provider, provider.quota.available);
}

export function maskApiKey(value: string) {
  const text = value.trim();
  if (!text) {
    return "";
  }
  if (text.length <= 4) {
    return "•".repeat(4);
  }
  if (text.length <= 8) {
    return `${text.slice(0, 1)}••••••${text.slice(-1)}`;
  }
  if (text.length <= 14) {
    return `${text.slice(0, 2)}••••••••${text.slice(-2)}`;
  }
  return `${text.slice(0, 6)}••••••••${text.slice(-4)}`;
}

export function providerIdentityName(provider: Provider) {
  return providerIdentityDisplayName(provider) || providerIdentityUsername(provider);
}

export function providerIdentityDisplayName(provider: Provider) {
  return provider.identity.displayName?.trim() || "";
}

export function providerIdentityUsername(provider: Provider) {
  return provider.identity.username?.trim() || "";
}

export function providerIdentitySecondaryUsername(provider: Provider) {
  const displayName = providerIdentityDisplayName(provider);
  const username = providerIdentityUsername(provider);
  if (!displayName || !username || displayName.toLocaleLowerCase() === username.toLocaleLowerCase()) {
    return "";
  }
  return username;
}

export function providerIdentityId(provider: Provider) {
  if (provider.auth.mode === "apiKey") {
    return "";
  }
  return provider.identity.userId?.trim() || provider.auth.apiUser?.trim() || "";
}

export function normalizeInviteLink(value: string) {
  const text = value.trim();
  if (!text || text.includes("/register?aff=")) {
    return text;
  }
  const [base, code] = text.split("?aff=");
  if (!base || !code) {
    return text;
  }
  return `${base.replace(/\/+$/, "")}/register?aff=${code.trim()}`;
}

export function availablePercent(provider: Provider) {
  if (!providerQuotaKnown(provider) || !providerQuotaTotalKnown(provider)) {
    return 0;
  }
  if (provider.quota.unlimited === true) {
    return 1;
  }
  const total = provider.quota.available + provider.quota.used;
  return total === 0 ? 0 : provider.quota.available / total;
}

export function availablePercentLabel(provider: Provider) {
  if (!providerQuotaKnown(provider) || !providerQuotaTotalKnown(provider)) {
    return "未公开";
  }
  if (provider.quota.unlimited === true) {
    return "∞";
  }
  return `${(availablePercent(provider) * 100).toFixed(1)}%`;
}

export function totalQuota(provider: Provider) {
  return provider.quota.available + provider.quota.used;
}

export function providerHasNoAvailableBalance(provider: Provider) {
  return Boolean(provider.automation.lastSyncedAt) && providerQuotaKnown(provider) && !providerQuotaUnlimited(provider) && provider.quota.available <= 0;
}
