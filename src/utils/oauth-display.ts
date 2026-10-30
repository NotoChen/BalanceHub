import type {
  AccountQuotaSample,
  AccountQuotaWindow,
  OAuthVendor,
} from "../stores/oauth-types";
import type { Provider } from "../stores/provider-types";

export function managedModelCount(provider: Provider): number | null {
  const { availableModels, availableModelsState } = provider.capabilities;
  return availableModels.length || availableModelsState?.updatedAt != null
    ? availableModels.length
    : null;
}
export const oauthVendors: { value: OAuthVendor; label: string }[] = [
  { value: "codex", label: "Codex" },
  { value: "claude", label: "Claude" },
  { value: "gemini", label: "Gemini CLI" },
  { value: "grok", label: "Grok" },
];
export function oauthVendorLabel(vendor: OAuthVendor) {
  return oauthVendors.find((entry) => entry.value === vendor)?.label || vendor;
}
/// Vendors report the tier as a lowercase machine value (`plus`, `pro`,
/// `go`). Only the tiers with a settled Chinese name are translated; product
/// tier names such as SuperGrok stay as the vendor spells them.
const planNames: Record<string, string> = {
  free: "免费",
  go: "Go",
  plus: "Plus",
  pro: "Pro",
  max: "Max",
  team: "团队版",
  business: "商业版",
  enterprise: "企业版",
  edu: "教育版",
};
export function planLabel(plan: string | null | undefined) {
  const value = plan?.trim() || "";
  return planNames[value.toLowerCase()] || value;
}
const numberFormat = new Intl.NumberFormat("zh-CN", {
  maximumFractionDigits: 2,
});
export function quotaNumber(value: number | null | undefined) {
  return value != null && Number.isFinite(value)
    ? numberFormat.format(value)
    : "—";
}
const percentFormat = new Intl.NumberFormat("zh-CN", {
  maximumFractionDigits: 1,
});
/// Quota meters are read at a glance, so a used percentage never carries the
/// second decimal that the general-purpose number formatter keeps.
export function quotaPercent(value: number | null | undefined) {
  return value != null && Number.isFinite(value)
    ? percentFormat.format(value)
    : "—";
}
/// 账号侧时间戳只在数据放旧时才显示，可能跨年显示；年份和时区让用户能
/// 把它和本机时间对上，而不是猜“09/28”是今年还是去年。
export function accountTimestamp(value: number | null | undefined) {
  if (!value) return "未提供";
  const date = new Date(value * 1000);
  const time = date.toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
  const zone = new Intl.DateTimeFormat("zh-CN", { timeZoneName: "shortOffset" })
    .formatToParts(date)
    .find((part) => part.type === "timeZoneName")
    ?.value.replace(/^GMT/, "UTC");
  return zone ? `${time} ${zone}` : time;
}
export function quotaResetLabel(
  value: number | null | undefined,
  now = Date.now(),
) {
  if (!value) return "";
  const minutes = Math.ceil((value * 1000 - now) / 60_000);
  /// 重置时刻已经过去时，这一行说的是「这个窗口刚刚到点」，不是一份待领的
  /// 额度；写成「待刷新额度」会让人以为还有额度没发下来。
  if (minutes <= 0) return "已到重置时间";
  if (minutes < 60) return `${minutes} 分钟后重置`;
  if (minutes < 1440)
    return `${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分后重置`;
  return `${Math.ceil(minutes / 1440)} 天后重置`;
}
/// The percentage a window is at. The redirect fills `usedPercent` on every
/// refresh, so the used/limit branch only serves documents written before that
/// normalization existed.
export function quotaWindowPercent(
  window: AccountQuotaWindow,
): number | null {
  if (window.usedPercent != null && Number.isFinite(window.usedPercent))
    return window.usedPercent;
  return window.limit && window.used != null
    ? (window.used / window.limit) * 100
    : null;
}
/// 窗口没有可画的比例，是因为站点根本没给上限：Grok 这类账号的额度窗口只
/// 有名字和重置时间。有上限但缺使用量时仍然算「有上限」，卡片写「未提供
/// 比例」而不是把一条有上限的额度说成不限量。
export function quotaWindowUnlimited(window: AccountQuotaWindow): boolean {
  if (window.usedPercent != null && Number.isFinite(window.usedPercent))
    return false;
  return (window.limit ?? 0) <= 0;
}
/// Windows that are nearly used up; the card names them instead of leaving the
/// user to read every meter.
export function quotaAlertWindows(
  windows: AccountQuotaWindow[],
): AccountQuotaWindow[] {
  return windows.filter((window) => (quotaWindowPercent(window) ?? 0) >= 90);
}
/// The window that refills first, so the card can say when pressure drops.
/// Reset times already in the past are waiting for a refresh, not a countdown.
export function nextQuotaReset(
  windows: AccountQuotaWindow[],
  now = Date.now(),
): AccountQuotaWindow | null {
  const upcoming = windows
    .filter((window) => (window.resetAt ?? 0) * 1000 > now)
    .sort((a, b) => (a.resetAt ?? 0) - (b.resetAt ?? 0));
  return upcoming[0] || null;
}
/// Detail views separate the account-wide windows from ones scoped to a model,
/// because the model windows answer a different question.
export function splitQuotaWindows(windows: AccountQuotaWindow[]): {
  shared: AccountQuotaWindow[];
  scoped: AccountQuotaWindow[];
} {
  const shared: AccountQuotaWindow[] = [];
  const scoped: AccountQuotaWindow[] = [];
  for (const window of windows) {
    (window.model ? scoped : shared).push(window);
  }
  return { shared, scoped };
}
/// Access tokens are renewed in the background; the panel tells the user how
/// long the credential has left rather than making them subtract timestamps.
export function tokenExpiryLabel(
  value: number | null | undefined,
  now = Date.now(),
): string {
  if (!value) return "";
  const minutes = Math.ceil((value * 1000 - now) / 60_000);
  if (minutes <= 0) return "已过期";
  if (minutes < 60) return `${minutes} 分钟后到期`;
  if (minutes < 1440) return `${Math.floor(minutes / 60)} 小时后到期`;
  return `${Math.ceil(minutes / 1440)} 天后到期`;
}
/// A token inside its last day is the point at which a paused renewal becomes
/// an outage, so the panel marks it instead of only printing the date.
export function tokenExpirySoon(
  value: number | null | undefined,
  now = Date.now(),
): boolean {
  if (!value) return false;
  return value * 1000 - now <= 86_400_000;
}
/// What the sparkline covers, said in words for its tooltip.
export function quotaTrendSummary(samples: AccountQuotaSample[]): string {
  if (samples.length < 2) return "";
  const values = samples.map((sample) => sample.usedPercent);
  const change = values[values.length - 1] - values[0];
  const sign = change > 0 ? "+" : "";
  return `近 ${samples.length} 次刷新 · 最低 ${quotaPercent(
    Math.min(...values),
  )}% · 最高 ${quotaPercent(Math.max(...values))}% · 累计 ${sign}${quotaPercent(
    change,
  )}%`;
}
/// Readings closer together than this say too little about a rate: two samples
/// a minute apart would extrapolate a refresh interval into a trend.
const MIN_BURN_SPAN_SECONDS = 600;
/// Percentage points consumed per hour between the first and last reading.
/// A flat or refilling window has no burn rate, and a window that only has one
/// reading cannot have one yet.
export function quotaBurnRate(
  samples: AccountQuotaSample[],
): number | null {
  if (samples.length < 2) return null;
  const first = samples[0];
  const last = samples[samples.length - 1];
  const hours = (last.at - first.at) / 3600;
  if (!Number.isFinite(hours) || hours * 3600 < MIN_BURN_SPAN_SECONDS) return null;
  const rate = (last.usedPercent - first.usedPercent) / hours;
  return Number.isFinite(rate) && rate > 0 ? rate : null;
}
/// When a window reaches 100% at the observed rate. It is an estimate from
/// BalanceHub's own readings, not something the provider promised, so the UI
/// always words it as 约.
export function quotaExhaustsAt(
  window: AccountQuotaWindow,
): number | null {
  const percent = quotaWindowPercent(window);
  const rate = quotaBurnRate(window.history);
  if (percent == null || rate == null || percent >= 100) return null;
  const last = window.history[window.history.length - 1];
  return Math.round(last.at + ((100 - percent) / rate) * 3600);
}
/// A window worth warning about: it runs out sooner than its own reset. A
/// window with no reset time is judged against one day, which is the shortest
/// reset any provider here reports.
export function quotaExhaustionLabel(
  window: AccountQuotaWindow,
  now = Date.now(),
): string {
  const exhaustsAt = quotaExhaustsAt(window);
  if (exhaustsAt == null) return "";
  const limit = window.resetAt || Math.floor(now / 1000) + 86_400;
  if (exhaustsAt >= limit) return "";
  return `按当前速度约 ${durationLabel(exhaustsAt - Math.floor(now / 1000))}后耗尽`;
}
/// The most urgent predicted exhaustion across all windows, so the panel warns
/// once with the window that is actually going to run out.
export function firstQuotaExhaustion(
  windows: AccountQuotaWindow[],
  now = Date.now(),
): { window: AccountQuotaWindow; label: string } | null {
  let worst: { window: AccountQuotaWindow; label: string; at: number } | null =
    null;
  for (const window of windows) {
    const label = quotaExhaustionLabel(window, now);
    if (!label) continue;
    const at = quotaExhaustsAt(window) ?? 0;
    if (!worst || at < worst.at) worst = { window, label, at };
  }
  return worst ? { window: worst.window, label: worst.label } : null;
}
/// Shared duration wording for countdowns that are measured, not scheduled.
export function durationLabel(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "即将";
  const minutes = Math.ceil(seconds / 60);
  if (minutes < 60) return `${minutes} 分钟`;
  if (minutes < 1440) return `${Math.floor(minutes / 60)} 小时`;
  return `${Math.ceil(minutes / 1440)} 天`;
}
