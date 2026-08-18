import type { Provider, SiteAnnouncement } from "../stores/provider-types.ts";

const HTML_BLOCK_TAG_PATTERN =
  /<\/?(?:address|article|aside|blockquote|br|center|dd|details|dialog|div|dl|dt|fieldset|figcaption|figure|footer|form|h[1-6]|header|hr|li|main|nav|ol|p|pre|section|table|tbody|td|tfoot|th|thead|tr|ul)\b[^>]*>/gi;
const HTML_TAG_PATTERN = /<[^>]+>/g;
const MARKDOWN_LINK_PATTERN = /\[([^\]]+)]\(([^)]+)\)/g;
const MARKDOWN_HEADING_PATTERN = /^#{1,6}\s+/gm;
const MARKDOWN_INLINE_TOKEN_PATTERN = /(\*\*|__|~~|`)/g;

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

export function siteAnnouncementDisplayTitle(item: SiteAnnouncement) {
  const explicitTitle = singleLineSiteAnnouncementText(item.title);
  if (explicitTitle) return explicitTitle;

  const firstLine = siteAnnouncementDisplayContent(item.content)
    .split("\n")
    .map((line) => line.trim())
    .find(Boolean);
  return firstLine || "站点公告";
}

export function siteAnnouncementDisplayContent(value: string) {
  return plainSiteAnnouncementText(value) || "公告未提供正文";
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
