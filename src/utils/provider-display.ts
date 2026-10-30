// 本文件只保留前端展示格式化。业务能力读取集中在 provider-actions.ts。
import type {
  AuthMode,
  Provider,
  ProviderApiKeyOption,
  ProviderModelScope,
  ProviderProtocol,
  ProviderQuotaDisplay,
} from "../stores/providers";
import type { AccountQuotaWindow } from "../stores/oauth-types";
import { providerNeedsCheckIn } from "./provider-actions.ts";
import { quotaPercent, quotaResetLabel } from "./oauth-display.ts";
import { type QuotaLevel, quotaLevel } from "./quota-level.ts";

export type ProviderCardTone =
  | "disabled"
  | "pending"
  | "error"
  | "warning"
  | "empty"
  | "ok"
  | "syncing";

/// 四类卡片共用的状态词表：同一个色调在任何卡型上都写同一句话。只有 warning
/// 的语义按卡型细化（中转站卡是「待签到」），其余走同一份。
const providerCardToneLabels: Record<Exclude<ProviderCardTone, "warning">, string> = {
  ok: "正常",
  pending: "待同步",
  syncing: "同步中",
  empty: "无余额",
  error: "异常",
  disabled: "已停用",
};

export function providerCardStatusLabel(
  tone: ProviderCardTone,
  warningLabel = "需关注",
) {
  return tone === "warning" ? warningLabel : providerCardToneLabels[tone];
}

/// 状态标悬停时只说卡片本身没写出来的事：异常消息。正常、已停用这些词
/// 本来就写在标里，再重复一遍只是噪音。四类卡片共用这一条。
export function providerCardStatusTitle(provider: Provider): string {
  return provider.runtime.errorMessage?.trim() || "";
}

/** 底栏、菜单和提示里对这张卡片的称呼；同一句话在四类卡片上保持同一读法。 */
export function providerCardScopeNoun(provider: Provider): string {
  if (provider.actions.cpaGateway) return "网关";
  if (provider.actions.oauthAccount) return "账号";
  return "中转站";
}

/** 卡片与筛选共用的状态；签到、探测等临时进度由卡片单独显示。 */
export function providerCardStatusTone(provider: Provider): Exclude<ProviderCardTone, "syncing"> {
  if (!provider.runtime.enabled) return "disabled";
  if (provider.runtime.status === "warning" && !provider.automation.lastSyncedAt) return "pending";
  if (provider.runtime.status === "error") return "error";
  if (provider.runtime.status === "warning" && provider.runtime.errorMessage) return "warning";
  if (providerNeedsCheckIn(provider)) return "warning";
  if (providerHasNoAvailableBalance(provider)) return "empty";
  return "ok";
}

const providerAuthModeLabels: Record<AuthMode, string> = {
  session: "Cookie",
  accessToken: "访问令牌",
  apiKey: "API Key",
  password: "账号密码",
};

export function providerModelScopeLabel(scope: ProviderModelScope | null | undefined): string {
  if (scope === "account") return "账号可用模型";
  if (scope === "apiKey") return "Key 可用模型";
  return "可用模型";
}

export function providerAuthModeLabel(provider: Provider) {
  return provider.authModeLabel?.trim() || providerAuthModeLabels[provider.auth.mode];
}

export function providerProtocolLabel(protocol: ProviderProtocol | Provider): string {
  if (typeof protocol !== "string") {
    return protocol.protocolLabel?.trim() || providerProtocolLabel(protocol.identity.protocol);
  }
  if (protocol === "oauth") return "OAuth 账号";
  if (protocol === "cliProxyApi") return "CPA";
  if (protocol === "sub2Api") return "Sub2API";
  if (protocol === "api") return "通用 API Key";
  return "NewAPI";
}

/** Returns the transport scheme separately from the provider protocol. */
export function providerTransportProtocol(baseUrl: string): string {
  const match = baseUrl.trim().match(/^([a-z][a-z0-9+.-]*):\/\//i);
  return match?.[1]?.toUpperCase() || "";
}

function decodedDisplaySecret(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function sensitiveUrlParameterValues(parameters: string): string[] {
  return parameters.split("&").flatMap((parameter) => {
    const separator = parameter.indexOf("=");
    if (separator < 0) return [];
    const name = decodedDisplaySecret(parameter.slice(0, separator).replace(/\+/g, " "))
      .replace(/[^a-z0-9]/gi, "");
    if (!/(?:key|token|secret|password|passwd|pwd|cookie|session(?:id)?|credentials?|authorization|auth|signature|sig|code)$/i.test(name)) {
      return [];
    }
    const value = parameter.slice(separator + 1);
    return [value, decodedDisplaySecret(value.replace(/\+/g, " "))];
  });
}

function providerUrlDisplaySecrets(baseUrl: string): string[] {
  try {
    const url = new URL(baseUrl.trim());
    const query = url.search.slice(1);
    const fragment = url.hash.slice(1);
    const secrets = [url.username, url.password, query, fragment, ...sensitiveUrlParameterValues(query)];
    // Fragments can carry an opaque token or OAuth parameters, including after a route.
    const fragmentParameters = fragment.includes("=") ? fragment : decodedDisplaySecret(fragment);
    if (fragmentParameters.includes("=")) {
      secrets.push(...sensitiveUrlParameterValues(fragmentParameters.slice(fragmentParameters.indexOf("?") + 1)));
    }
    return secrets;
  } catch {
    return [];
  }
}

function displaySecretVariants(value: string): string[] {
  const raw = value.trim();
  if (!raw) return [];
  const decoded = decodedDisplaySecret(raw);
  return [raw, decoded].flatMap((secret) => {
    try {
      const encoded = encodeURIComponent(secret);
      return [secret, encoded, encoded.replace(/%20/g, "+")];
    } catch {
      return [secret];
    }
  });
}

/** Removes configured and URL-derived secrets from labels without changing provider data. */
export function providerSafeDisplayText(provider: Provider, value: string): string {
  const secrets = [
    provider.auth.apiKey,
    provider.auth.accessToken,
    provider.auth.sessionCookie,
    provider.auth.loginPassword,
    provider.auth.refreshToken,
    ...provider.auth.apiKeyOptions.map((option) => option.key),
    ...providerUrlDisplaySecrets(provider.identity.baseUrl),
  ];
  const patterns = [...new Set(secrets.flatMap(displaySecretVariants))]
    .sort((left, right) => right.length - left.length)
    .map((secret) => secret.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
      // Percent-escape hex casing is insignificant; credential character casing is not.
      .replace(/%[0-9a-f]{2}/gi, (octet) => octet.replace(/[a-f]/gi, (letter) => `[${letter.toLowerCase()}${letter.toUpperCase()}]`)));
  return patterns.length ? value.replace(new RegExp(patterns.join("|"), "g"), "••••") : value;
}

/** Keeps a distinguishing HTTP(S) site path, never URL authentication or raw invalid input. */
export function providerSiteAddressLabel(provider: Provider): string {
  const value = provider.identity.baseUrl.trim();
  if (!value) return "地址未配置";

  try {
    const url = new URL(value);
    if (url.protocol !== "http:" && url.protocol !== "https:") return "地址格式异常";
    const path = url.pathname.split("/").map((segment) => {
      try {
        const decoded = decodeURIComponent(segment);
        return providerSafeDisplayText(provider, decoded) === decoded ? segment : "••••";
      } catch {
        return "••••";
      }
    }).join("/").replace(/\/+$/, "");
    return providerSafeDisplayText(provider, `${url.origin}${path}`);
  } catch {
    return "地址格式异常";
  }
}

export function providerQuotaKnown(provider: Provider) {
  return provider.quota.known !== false;
}

export function providerQuotaTotalKnown(provider: Provider) {
  return provider.quota.totalKnown !== false;
}

export function providerAuthModeDescription(provider: Provider) {
  const declared = provider.authModeDescription?.trim();
  if (declared) {
    if (provider.auth.mode === "accessToken") {
      return provider.auth.refreshToken.trim()
        ? `${declared}（包含刷新令牌，可自动续期）`
        : `${declared}（没有刷新令牌）`;
    }
    return declared;
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

export function formatQuotaValue(value: number, quotaDisplay: ProviderQuotaDisplay, precision?: number) {
  if (!Number.isFinite(value)) return "—";
  const displayType = quotaDisplay.quotaDisplayType || "currency";
  const formatted = precision === undefined
    ? formatNumberCompact(value, displayType.toLowerCase() === "tokens" ? 0 : 2)
    : new Intl.NumberFormat("en-US", { maximumFractionDigits: precision }).format(value);
  if (displayType.toLowerCase() === "tokens") {
    return formatted;
  }
  const symbol = normalizeCurrencySymbol(displayType, quotaDisplay.currencySymbol);
  return `${symbol}${formatted}`;
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

/// 同步时间只在数据已经放旧时才展示，此刻可能跨年、跨时区对着看；只写
/// “09/28 03:06:40” 既不知道是哪一年，也不知道是本地还是服务器时间，所以带上
/// 年份并显式标注本地时区。
export function formatProviderSyncTime(
  value: string | number | null | undefined,
) {
  const date = providerSyncInstant(value);
  if (!date) return "";
  const time = new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(date);
  const zone = timeZoneLabel(date);
  return zone ? `${time} ${zone}` : time;
}

function timeZoneLabel(date: Date) {
  const part = new Intl.DateTimeFormat("zh-CN", { timeZoneName: "shortOffset" })
    .formatToParts(date)
    .find((item) => item.type === "timeZoneName");
  return part?.value.replace(/^GMT/, "UTC") ?? "";
}

/// Sync timestamps are written by several adapters and reach the UI with mixed
/// units, so one parser decides which scale a value is on.
export function providerSyncInstant(
  value: string | number | null | undefined,
): Date | null {
  if (!value) return null;
  const raw = Number(value);
  const date = Number.isFinite(raw)
    ? new Date(raw < 1_000_000_000_000 ? raw * 1000 : raw)
    : new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

/// 卡片上的同步时间只在数据过期时出现。数据新鲜时「3 小时前」不回答任何
/// 问题，却会在每张卡上重复一遍；超过一天没有更新才写成一句警示，准确的
/// 时刻留在悬停提示和详情面板里。一天以内的相对时长在扫读网格时没有区别。
const STALE_SYNC_SECONDS = 86_400;

export function providerStaleSyncNote(
  value: string | number | null | undefined,
  now = Date.now(),
): string {
  const date = providerSyncInstant(value);
  if (!date) return "";
  const seconds = Math.floor((now - date.getTime()) / 1000);
  if (seconds <= STALE_SYNC_SECONDS) return "";
  return `数据已 ${Math.floor(seconds / STALE_SYNC_SECONDS)} 天未更新`;
}

export function providerQuotaUnlimited(provider: Provider) {
  return provider.quota.unlimited === true;
}

/// 一张卡片读的额度窗口：OAuth / CPA 卡来自账号订阅，中转站卡来自 Rust 收口
/// 的三条订阅窗口。条本身共用 AccountQuotaBar。
export interface ProviderQuotaWindowRow {
  label: string;
  /** 已用比例，决定进度条长度和额度等级；不限量窗口为 null。 */
  percent: number | null;
  /** 条右端的读法；不限量窗口写「不限」。 */
  percentLabel: string;
  /** 窗口行右侧的重置倒计时；没有重置时间时为空。 */
  caption: string;
  /** unix 秒；没有重置时间时为 null。 */
  resetAt: number | null;
  level: QuotaLevel | "unlimited";
  /** 站点报的使用量 / 上限；只报比例时两者为空。窗口条拿它区分「不限量」
     和「有上限但缺数据」。 */
  used: number | null;
  limit: number | null;
}

/// 一张卡的窗口行：窗口名、重置倒计时和已用比例同一行，条长就是已用比例，
/// 颜色随等级变深。中转站卡的订阅窗口和 OAuth / CPA 卡的额度窗口都从这里出，
/// 卡片只决定最多取几条。
function quotaWindowRow(
  window: {
    label: string;
    /// 站点报的数量额度；只报比例时为 null。
    used?: number | null;
    limit?: number | null;
    /// 站点直接给出的已用比例；有的接口只报比例，没有使用量/额度。
    usedPercent?: number | null;
    resetsAt?: number | null;
  },
  now: number,
): ProviderQuotaWindowRow {
  const limit = Number.isFinite(window.limit) ? Number(window.limit) : 0;
  const used = Number.isFinite(window.used) ? Math.max(0, Number(window.used)) : 0;
  const reported =
    window.usedPercent != null && Number.isFinite(window.usedPercent)
      ? window.usedPercent
      : null;
  const percent =
    reported != null
      ? Math.min(100, Math.max(0, reported))
      : limit > 0
        ? Math.min(100, Math.max(0, (used / limit) * 100))
        : null;
  const resetsAt = window.resetsAt ?? null;
  return {
    label: window.label,
    percent,
    percentLabel: percent == null ? "不限" : `${quotaPercent(percent)}%`,
    caption: resetsAt ? quotaResetLabel(resetsAt, now) : "",
    resetAt: resetsAt,
    level: percent == null ? "unlimited" : quotaLevel(percent),
    used: Number.isFinite(window.used) ? used : null,
    limit: limit > 0 ? limit : null,
  };
}

/// 订阅制中转站的额度在周期窗口里，钱包只是溢出账户。Rust 侧已收口到三条，
/// 卡片高度固定，这里再兜底一次，前端不指望上游永远只给三条。
export function providerQuotaWindowRows(
  provider: Provider,
  now = Date.now(),
): ProviderQuotaWindowRow[] {
  return (provider.quota.windows ?? [])
    .slice(0, 3)
    .map((window) => quotaWindowRow(window, now));
}

/// 卡片正文把窗口行喂给 AccountQuotaBar：四类卡片共用同一支条，适配层也只能
/// 有一份。窗口条用不到的字段按面板结构占位，不在这里新造读法。
export function providerQuotaBarWindow(
  row: ProviderQuotaWindowRow,
): AccountQuotaWindow {
  return {
    id: row.label,
    label: row.label,
    model: null,
    usedPercent: row.percent,
    used: row.used,
    limit: row.limit,
    unit: null,
    resetAt: row.resetAt,
    windowSeconds: null,
    history: [],
  };
}

/// 账号卡与网关卡列出窗口的条数与中转站卡相同：最多三条，Rust 已按已用
/// 比例排好序，三条之外的摘要放在卡片底部。条数由额度区块决定，不由卡型决定。
export function managedQuotaWindowRows(
  windows: readonly AccountQuotaWindow[],
  now = Date.now(),
): ProviderQuotaWindowRow[] {
  return windows.slice(0, 3).map((window) =>
    quotaWindowRow(
      {
        label: window.label,
        used: window.used,
        limit: window.limit,
        // 账号接口多数只给比例，`used` / `limit` 为空；漏掉这一项会让每张
        // OAuth 卡片都写成「未提供比例」。
        usedPercent: window.usedPercent,
        resetsAt: window.resetAt,
      },
      now,
    ),
  );
}

/// 供卡片判断是否用窗口行替换钱包比例条：有窗口时窗口才是额度来源。
export function providerHasQuotaWindows(provider: Provider): boolean {
  return (provider.quota.windows?.length ?? 0) > 0;
}

/// 不限量的窗口没有可比比例，不参与“是否用尽”的判断。窗口数量在 Rust 侧
/// 已收口到三条，因此从行数据取比例不会再漏掉被卡片裁掉的窗口。
function limitedWindowRows(provider: Provider): ProviderQuotaWindowRow[] {
  return providerQuotaWindowRows(provider).filter((row) => row.percent != null);
}

/// 窗口压力决定订阅制账号的额度等级：没有窗口时返回 null，交给钱包余额判断。
export function providerQuotaWindowWorstLevel(provider: Provider): QuotaLevel | null {
  if (!providerHasQuotaWindows(provider)) return null;
  const rows = limitedWindowRows(provider);
  // 窗口全部不限量：额度不会用尽，按健康处理。
  if (!rows.length) return "ok";
  return rows.reduce<QuotaLevel>((worst, row) => {
    if (row.level === "high") return "high";
    if (row.level === "warn" && worst === "ok") return "warn";
    return worst;
  }, "ok");
}

/// 卡片上只有一个余额数字时不需要标签：卡型已经说明这是账号余额，写一遍
/// 「账号可用额度」只是占掉一行。只有数字需要和别的数字区分时才写——Key
/// 维度的额度，以及被订阅窗口顶替之后剩下的钱包余额。
export function providerQuotaScopeLabel(provider: Provider) {
  if (provider.quota.scope === "token") return "API Key 可用额度";
  // 订阅制账号这里的数字是钱包溢出额度，主额度在窗口里，标签要避免读成“总可用”。
  return providerHasQuotaWindows(provider) ? "钱包余额" : "";
}

export interface ProviderQuotaHeadline {
  label: string;
  value: string;
  title: string;
}

/// 卡片的主数字代表“还能用多少”。订阅制账号的约束在周期窗口里，钱包余额只是
/// 溢出账户，因此主数字换成最紧张窗口的剩余额度，钱包余额留在详情面板。
export function providerQuotaHeadline(
  provider: Provider,
  now = Date.now(),
): ProviderQuotaHeadline {
  if (!providerHasQuotaWindows(provider)) {
    return {
      label: providerQuotaScopeLabel(provider),
      value: providerAvailableQuotaLabel(provider),
      title: "",
    };
  }
  const rows = providerQuotaWindowRows(provider, now);
  const limited = rows.filter((row) => row.percent != null);
  // 窗口全部不限量：没有“剩余”可言，直接说明额度不受限。
  if (!limited.length) {
    return { label: "订阅额度", value: "不限量", title: "订阅窗口不限量" };
  }
  // 压力最大的窗口决定今天还能不能继续用，主数字跟随它的剩余额度。
  const tightest = limited.reduce((worst, row) =>
    (row.percent ?? 0) >= (worst.percent ?? 0) ? row : worst,
  );
  const window = (provider.quota.windows ?? []).find(
    (item) => item.label === tightest.label,
  );
  const limit = Number.isFinite(window?.limit) ? (window?.limit ?? 0) : 0;
  const used = Number.isFinite(window?.used) ? Math.max(0, window?.used ?? 0) : 0;
  return {
    label: `${tightest.label}剩余`,
    value: formatProviderQuota(provider, Math.max(0, limit - used)),
    title: tightest.caption,
  };
}

/// 不限量是 Rust 明确给出的结论，不是“还没读到”。通用 API Key 往往没有额度
/// 接口（known = false）但额度不受限，卡片要写「不限量」，不能降级成「未公开」。
export function providerAvailableQuotaLabel(provider: Provider) {
  if (providerQuotaUnlimited(provider)) {
    return "不限量";
  }
  if (!providerQuotaKnown(provider)) {
    return "未公开";
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

export function providerCardTitle(provider: Provider) {
  return providerDisplayLabel(provider);
}

export function providerDisplayLabel(provider: Provider) {
  return provider.displayLabel?.trim() || provider.identity.name.trim();
}

export function providerApiKeyRemark(provider: Provider) {
  if (provider.auth.mode !== "apiKey") return "";
  return provider.identity.remark?.trim() || "";
}

export function providerApiKeyLocalRemark(option: ProviderApiKeyOption) {
  return option.localName?.trim() || "";
}

export function providerApiKeyRemoteName(option: ProviderApiKeyOption) {
  return option.name?.trim() || "";
}

export function providerApiKeyCardName(option: ProviderApiKeyOption) {
  const localRemark = providerApiKeyLocalRemark(option);
  if (localRemark) return localRemark;

  const remoteName = providerApiKeyRemoteName(option);
  return remoteName === "当前 API Key" || remoteName === "当前配置 API Key"
    ? ""
    : remoteName;
}

export function providerApiKeyDisplayName(option: ProviderApiKeyOption) {
  return providerApiKeyLocalRemark(option) || providerApiKeyRemoteName(option) || "未命名 API Key";
}

export function providerApiKeySecondaryName(option: ProviderApiKeyOption) {
  const localRemark = providerApiKeyLocalRemark(option);
  const remoteName = providerApiKeyRemoteName(option);
  if (
    !localRemark
    || !remoteName
    || localRemark.toLocaleLowerCase() === remoteName.toLocaleLowerCase()
  ) {
    return "";
  }
  return remoteName;
}

export function providerDefaultApiKeyOption(provider: Provider) {
  const key = provider.auth.apiKey?.trim() || "";
  if (key) {
    const option = provider.auth.apiKeyOptions.find((candidate) => candidate.key?.trim() === key);
    if (option) return option;
  }
  const tokenId = provider.auth.apiKeyTokenId?.trim() || "";
  return tokenId
    ? provider.auth.apiKeyOptions.find((candidate) => candidate.tokenId?.trim() === tokenId)
    : undefined;
}

export function providerUsesApiKeyOption(
  provider: Provider,
  option: ProviderApiKeyOption,
) {
  const current = providerDefaultApiKeyOption(provider);
  if (!current) {
    const configured = provider.auth.apiKey?.trim() || "";
    return Boolean(configured && option.key?.trim() && configured === option.key.trim());
  }
  const currentLocalId = current.localId?.trim() || "";
  const optionLocalId = option.localId?.trim() || "";
  if (currentLocalId && optionLocalId) return currentLocalId === optionLocalId;

  const currentTokenId = current.tokenId?.trim() || "";
  const optionTokenId = option.tokenId?.trim() || "";
  if (currentTokenId && optionTokenId) return currentTokenId === optionTokenId;

  const currentKey = current.key?.trim() || "";
  const optionKey = option.key?.trim() || "";
  return Boolean(currentKey && optionKey && currentKey === optionKey);
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

/// 钱包卡的比例条画已用份额，和订阅窗口条、OAuth 窗口条同向：条越长压力越大，
/// 一排卡片之间可以直接比较。
export function usedPercent(provider: Provider) {
  if (!providerQuotaKnown(provider) || !providerQuotaTotalKnown(provider)) {
    return 0;
  }
  if (provider.quota.unlimited === true) {
    return 0;
  }
  const total = provider.quota.available + provider.quota.used;
  return total <= 0 ? 0 : Math.max(0, Math.min(1, provider.quota.used / total));
}

export function usedPercentLabel(provider: Provider) {
  if (!providerQuotaKnown(provider) || !providerQuotaTotalKnown(provider)) {
    return "未公开";
  }
  if (provider.quota.unlimited === true) {
    return "不限量";
  }
  return `${(usedPercent(provider) * 100).toFixed(1)}%`;
}

export function providerHasNoAvailableBalance(provider: Provider) {
  if (!provider.automation.lastSyncedAt || !providerQuotaKnown(provider) || providerQuotaUnlimited(provider)) {
    return false;
  }
  // 订阅制账号的钱包只装溢出额度：余额为 0 不等于用尽，窗口全部用尽才算。
  if (providerHasQuotaWindows(provider)) {
    const windows = limitedWindowRows(provider);
    // 不限量窗口能一直用，不参与“用尽”的判断。
    return windows.length > 0 && windows.every((row) => (row.percent ?? 0) >= 100);
  }
  return provider.quota.available <= 0;
}

export type ProviderQuotaTone =
  | "unlimited"
  | "neutral"
  | "normal"
  | "warning"
  | "high"
  | "empty";

/// A relay card judges its balance with the same thresholds as an OAuth quota
/// window, so "nearly gone" reads the same way across both card families.
export function providerQuotaTone(provider: Provider): ProviderQuotaTone {
  if (providerQuotaUnlimited(provider)) return "unlimited";
  if (!provider.automation.lastSyncedAt) return "neutral";
  // 有窗口上限时由窗口决定额度压力，钱包余额在这类账号里是溢出账户。
  const windowLevel = providerQuotaWindowWorstLevel(provider);
  if (windowLevel) {
    switch (windowLevel) {
      case "high":
        return "high";
      case "warn":
        return "warning";
      case "unknown":
        return "neutral";
      default:
        return "normal";
    }
  }
  if (!providerQuotaKnown(provider) || !providerQuotaTotalKnown(provider)) return "neutral";
  if (provider.quota.available <= 0) return "empty";
  // 余额条和进度条共用“已用比例”，压力等级也按同一个比例判定。
  switch (quotaLevel(Math.round(usedPercent(provider) * 1e8) / 1e6)) {
    case "high":
      return "high";
    case "warn":
      return "warning";
    case "unknown":
      return "neutral";
    default:
      return "normal";
  }
}
