// 注意：本文件的额度/货币/邀请链接/anyrouter 判定等格式化逻辑，与 Rust 端
// （src-tauri/src/tray.rs、providers/newapi_site.rs、models.rs）存在同源镜像实现。
// 修改任一侧的格式化规则时，请同步另一侧，避免两处显示不一致。
import type { Provider, ProviderQuotaDisplay } from "../stores/providers";

export type ProviderCardTone = "disabled" | "error" | "warning" | "empty" | "ok" | "syncing";

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

export function providerQuotaUnlimited(provider: Provider) {
  return provider.quota.unlimited === true;
}

export function providerQuotaScopeLabel(provider: Provider) {
  if (provider.quota.scope === "token") return "令牌额度";
  if (providerQuotaUnlimited(provider)) return "账号额度";
  return availablePercentLabel(provider);
}

export function providerTotalQuotaLabel(provider: Provider) {
  if (providerQuotaUnlimited(provider)) {
    return "∞";
  }
  return formatProviderQuota(provider, totalQuota(provider));
}

export function providerAvailableQuotaLabel(provider: Provider) {
  if (providerQuotaUnlimited(provider)) {
    return "∞";
  }
  return formatProviderQuota(provider, provider.quota.available);
}

export function maskApiKey(value: string) {
  const text = value.trim();
  if (text.length <= 14) {
    return text;
  }
  return `${text.slice(0, 8)}****${text.slice(-6)}`;
}

export function isAnyRouterProvider(provider: Provider) {
  return provider.identity.baseUrl.toLowerCase().includes("anyrouter");
}

export function providerCheckInUser(provider: Provider) {
  const apiUser = provider.auth.apiUser.trim();
  return apiUser || (isAnyRouterProvider(provider) ? provider.identity.id : "");
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

export function providerIdentityId(provider: Provider) {
  return provider.identity.userId?.trim() || provider.auth.apiUser?.trim() || "";
}

export function supportsCheckIn(provider: Provider) {
  const capabilities = provider.capabilities;
  if (capabilities?.checkInKnown) {
    return capabilities.checkInSupported;
  }
  if (isAnyRouterProvider(provider)) {
    return Boolean(provider.auth.sessionCookie.trim());
  }
  return (
    (provider.auth.mode === "accessToken" && Boolean(provider.auth.accessToken.trim() && provider.auth.apiUser.trim())) ||
    (provider.auth.mode === "session" && Boolean(provider.auth.sessionCookie.trim() && provider.auth.apiUser.trim()))
  );
}

export function supportsApiKeyManagement(provider: Provider) {
  const capabilities = provider.capabilities;
  if (capabilities?.apiKeyManagementKnown) {
    return capabilities.apiKeyManagementSupported;
  }
  return Boolean(provider.auth.apiUser.trim() && (provider.auth.accessToken.trim() || provider.auth.sessionCookie.trim()));
}

export function supportsInvitation(provider: Provider) {
  const capabilities = provider.capabilities;
  if (capabilities?.invitationKnown) {
    return capabilities.invitationSupported;
  }
  return Boolean(
    provider.capabilities.inviteLink?.trim() ||
      (provider.auth.apiUser.trim() && (provider.auth.accessToken.trim() || provider.auth.sessionCookie.trim())),
  );
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

export function parseStoredDate(value: string | null) {
  if (!value) {
    return null;
  }

  const numericTimestamp = Number(value);
  if (Number.isFinite(numericTimestamp)) {
    return new Date(numericTimestamp > 1_000_000_000_000 ? numericTimestamp : numericTimestamp * 1000);
  }

  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

export function isSameLocalDay(a: Date, b: Date) {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

export function providerCheckedInToday(provider: Provider) {
  if (!supportsCheckIn(provider)) {
    return false;
  }

  const checkedAt = parseStoredDate(provider.automation.lastCheckedInAt);
  if (!checkedAt || !isSameLocalDay(checkedAt, new Date())) {
    return false;
  }

  const checkedUser = provider.automation.lastCheckInUser.trim();
  if (!checkedUser) {
    return true;
  }

  return checkedUser === providerCheckInUser(provider);
}

export function quotaPercent(provider: Provider) {
  if (provider.quota.unlimited === true) {
    return 1;
  }
  const total = provider.quota.available + provider.quota.used;
  return total === 0 ? 0 : provider.quota.used / total;
}

export function availablePercent(provider: Provider) {
  if (provider.quota.unlimited === true) {
    return 1;
  }
  const total = provider.quota.available + provider.quota.used;
  return total === 0 ? 0 : provider.quota.available / total;
}

export function availablePercentLabel(provider: Provider) {
  if (provider.quota.unlimited === true) {
    return "∞";
  }
  return `${(availablePercent(provider) * 100).toFixed(1)}%`;
}

export function totalQuota(provider: Provider) {
  return provider.quota.available + provider.quota.used;
}

export function providerNeedsCheckIn(provider: Provider) {
  return supportsCheckIn(provider) && !providerCheckedInToday(provider);
}

export function providerHasNoAvailableBalance(provider: Provider) {
  return Boolean(provider.automation.lastSyncedAt) && !providerQuotaUnlimited(provider) && provider.quota.available <= 0;
}
