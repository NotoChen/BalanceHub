import type { Provider, SiteAnnouncement } from "../stores/provider-types.ts";

const HTML_BLOCK_TAG_PATTERN =
  /<\/?(?:address|article|aside|blockquote|br|center|dd|details|dialog|div|dl|dt|fieldset|figcaption|figure|footer|form|h[1-6]|header|hr|li|main|nav|ol|p|pre|section|table|tbody|td|tfoot|th|thead|tr|ul)\b[^>]*>/gi;
const HTML_TAG_PATTERN = /<[^>]+>/g;
const MARKDOWN_LINK_PATTERN = /\[([^\]]+)]\(([^)]+)\)/g;
const MARKDOWN_HEADING_PATTERN = /^#{1,6}\s+/gm;
const MARKDOWN_INLINE_TOKEN_PATTERN = /(\*\*|__|~~|`)/g;
const ANNOUNCEMENT_LEVEL_LABELS: Record<string, string> = {
  ongoing: "进行中",
  success: "已完成",
  warning: "注意",
  error: "故障",
};

/** 只认识协议约定的公告级别；default 和未知取值按普通公告显示。 */
export function siteAnnouncementLevelKey(level: string) {
  const key = level.trim().toLowerCase();
  return key in ANNOUNCEMENT_LEVEL_LABELS ? key : "";
}

export function siteAnnouncementLevelLabel(level: string) {
  return ANNOUNCEMENT_LEVEL_LABELS[siteAnnouncementLevelKey(level)] ?? "";
}

export function siteAnnouncementTimestamp(value: string | null) {
  if (!value) return 0;
  const numeric = Number(value);
  if (Number.isFinite(numeric) && numeric > 0) {
    return numeric < 1_000_000_000_000 ? numeric * 1_000 : numeric;
  }
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

export function latestSiteAnnouncementTimestamp(item: SiteAnnouncement) {
  return siteAnnouncementTimestamp(item.updatedAt) || siteAnnouncementTimestamp(item.publishedAt);
}

export interface SiteAnnouncementSection {
  source: SiteAnnouncement["source"];
  label: string;
  items: SiteAnnouncement[];
  unreadCount: number;
}

export interface SiteAnnouncementGroup {
  providerId: string;
  providerName: string;
  providerProtocol: SiteAnnouncement["providerProtocol"];
  /** 同一站点内部按来源分开：站点总公告、控制台公告、订阅公告不混在一起。 */
  sections: SiteAnnouncementSection[];
  items: SiteAnnouncement[];
  unreadCount: number;
  latestTimestamp: number;
}

const SOURCE_LABELS: Record<SiteAnnouncement["source"], string> = {
  siteNotice: "站点公告",
  console: "控制台公告",
  announcement: "公告",
};

/// 站点总公告是长期有效的置顶内容，排在控制台公告之前。
const SOURCE_ORDER: SiteAnnouncement["source"][] = ["siteNotice", "console", "announcement"];

export function siteAnnouncementSourceLabel(source: SiteAnnouncement["source"]) {
  return SOURCE_LABELS[source] ?? "公告";
}

function sourceRank(source: SiteAnnouncement["source"]) {
  const index = SOURCE_ORDER.indexOf(source);
  return index === -1 ? SOURCE_ORDER.length : index;
}

/**
 * 线性时间线按站点分组：同一来源的公告排在一起，站点之间按各自最新一条公告
 * 的发布时间排序，组内保持传进来的时间倒序。
 */
export function groupSiteAnnouncements(
  items: SiteAnnouncement[],
  isRead: (item: SiteAnnouncement) => boolean,
): SiteAnnouncementGroup[] {
  const groups = new Map<string, SiteAnnouncementGroup>();
  for (const item of items) {
    const existing = groups.get(item.providerId);
    if (existing) {
      existing.items.push(item);
      continue;
    }
    groups.set(item.providerId, {
      providerId: item.providerId,
      providerName: item.providerName,
      providerProtocol: item.providerProtocol,
      sections: [],
      items: [item],
      unreadCount: 0,
      latestTimestamp: 0,
    });
  }
  for (const group of groups.values()) {
    group.unreadCount = group.items.filter((item) => !isRead(item)).length;
    group.latestTimestamp = group.items.reduce(
      (latest, item) => Math.max(latest, latestSiteAnnouncementTimestamp(item)),
      0,
    );
    group.sections = buildSections(group.items, isRead);
  }
  return Array.from(groups.values()).sort(
    (left, right) =>
      right.latestTimestamp - left.latestTimestamp ||
      left.providerName.localeCompare(right.providerName, "zh-CN"),
  );
}

function buildSections(
  items: SiteAnnouncement[],
  isRead: (item: SiteAnnouncement) => boolean,
): SiteAnnouncementSection[] {
  const buckets = new Map<SiteAnnouncement["source"], SiteAnnouncement[]>();
  for (const item of items) {
    const bucket = buckets.get(item.source);
    if (bucket) {
      bucket.push(item);
    } else {
      buckets.set(item.source, [item]);
    }
  }
  return Array.from(buckets.entries())
    .sort(([left], [right]) => sourceRank(left) - sourceRank(right))
    .map(([source, entries]) => ({
      source,
      label: siteAnnouncementSourceLabel(source),
      items: entries,
      unreadCount: entries.filter((entry) => !isRead(entry)).length,
    }));
}

export function formatSiteAnnouncementDateTime(value: string | null) {
  const timestamp = siteAnnouncementTimestamp(value);
  if (!timestamp) return value || "时间未提供";
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(new Date(timestamp));
}

/**
 * 公告标题里由协议真正给出的那一份。NewAPI 控制台公告多数没有单独标题
 * （正文第一行就是标题），后端会从正文提取标题供本地识别用，但界面上
 * 再渲染一次就和正文重复，所以只有协议明确给了标题才单独显示。
 */
export function siteAnnouncementExplicitTitle(item: SiteAnnouncement) {
  if (item.titleIsDerived) return "";
  return singleLineSiteAnnouncementText(item.title);
}

export function plainSiteAnnouncementText(value: string) {
  return value
    .replace(/\r\n?/g, "\n")
    .replace(HTML_BLOCK_TAG_PATTERN, "\n")
    .replace(HTML_TAG_PATTERN, "")
    .replace(MARKDOWN_LINK_PATTERN, "$1 ($2)")
    .replace(MARKDOWN_HEADING_PATTERN, "")
    .replace(MARKDOWN_INLINE_TOKEN_PATTERN, "")
    .replace(/&(?:nbsp|amp|lt|gt|quot|apos|#39|#\d+|#x[\da-f]+);/gi, decodeHtmlEntity)
    .split("\n")
    .map((line) => line.replace(/[\t ]+/g, " ").trim())
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function singleLineSiteAnnouncementText(value: string) {
  return plainSiteAnnouncementText(value).replace(/\s+/g, " ").trim();
}

function decodeHtmlEntity(entity: string) {
  const normalized = entity.toLowerCase();
  const named: Record<string, string> = {
    "&nbsp;": " ",
    "&amp;": "&",
    "&lt;": "<",
    "&gt;": ">",
    "&quot;": '"',
    "&apos;": "'",
    "&#39;": "'",
  };
  if (named[normalized]) return named[normalized];

  const radix = normalized.startsWith("&#x") ? 16 : 10;
  const digits = normalized.slice(radix === 16 ? 3 : 2, -1);
  const codePoint = Number.parseInt(digits, radix);
  if (!Number.isInteger(codePoint) || codePoint <= 0 || codePoint > 0x10ffff) {
    return entity;
  }
  try {
    return String.fromCodePoint(codePoint);
  } catch {
    return entity;
  }
}

export function providerAnnouncementSourceSignature(providers: Provider[]) {
  const sources = new Set<string>();
  for (const provider of providers) {
    if (!provider.runtime.enabled || provider.identity.protocol === "api") continue;
    sources.add(
      `${provider.identity.protocol}:${announcementSiteOrigin(provider.identity.baseUrl)}`,
    );
  }
  return Array.from(sources).sort().join("|");
}

function announcementSiteOrigin(value: string) {
  const trimmed = value.trim();
  try {
    const url = new URL(trimmed);
    return url.origin.toLowerCase();
  } catch {
    return trimmed.replace(/\/+$/, "").toLowerCase();
  }
}
