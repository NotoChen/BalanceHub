import assert from "node:assert/strict";
import test from "node:test";

import type { Provider, SiteAnnouncement } from "../src/stores/provider-types.ts";
import {
  groupSiteAnnouncements,
  latestSiteAnnouncementTimestamp,
  plainSiteAnnouncementText,
  providerAnnouncementSourceSignature,
  siteAnnouncementExplicitTitle,
  siteAnnouncementLevelKey,
  siteAnnouncementLevelLabel,
  siteAnnouncementTimestamp,
} from "../src/utils/site-announcements.ts";

test("announcement timestamps accept seconds, milliseconds and RFC3339", () => {
  assert.equal(siteAnnouncementTimestamp("1"), 1_000);
  assert.equal(siteAnnouncementTimestamp("1700000000000"), 1_700_000_000_000);
  assert.equal(
    siteAnnouncementTimestamp("2026-08-17T10:00:00Z"),
    Date.parse("2026-08-17T10:00:00Z"),
  );
  assert.equal(siteAnnouncementTimestamp("invalid"), 0);
});

test("announcement sorting prefers updated time over published time", () => {
  const item = {
    updatedAt: "200",
    publishedAt: "300",
  } as SiteAnnouncement;
  assert.equal(latestSiteAnnouncementTimestamp(item), 200_000);
});

test("announcement source signature ignores status-only provider updates", () => {
  const provider = {
    identity: {
      id: "provider-1",
      protocol: "newApi",
      baseUrl: "https://relay.example.com",
    },
    auth: { mode: "password" },
    runtime: { enabled: true, status: "ok" },
  } as Provider;
  const first = providerAnnouncementSourceSignature([provider]);
  const second = providerAnnouncementSourceSignature([
    {
      ...provider,
      runtime: { ...provider.runtime, status: "syncing" },
    },
  ]);
  assert.equal(first, second);
});

test("announcement source signature is shared by accounts on the same site", () => {
  const provider = {
    identity: {
      id: "provider-1",
      protocol: "sub2Api",
      baseUrl: "https://relay.example.com",
      username: "account-a",
      userId: "7",
      displayName: "Account A",
    },
    auth: {
      mode: "password",
      loginUsername: "account-a@example.com",
      apiUser: "7",
    },
    runtime: { enabled: true, status: "ok" },
  } as Provider;
  const first = providerAnnouncementSourceSignature([provider]);
  const second = providerAnnouncementSourceSignature([
    {
      ...provider,
      identity: { ...provider.identity, username: "account-b", userId: "8" },
      auth: { ...provider.auth, loginUsername: "account-b@example.com", apiUser: "8" },
    },
  ]);
  assert.equal(first, second);
});

test("announcement source signature normalizes paths and ignores API-key-only providers", () => {
  const first = providerAnnouncementSourceSignature([
    {
      identity: { id: "a", protocol: "newApi", baseUrl: "https://relay.example.com/api" },
      auth: { mode: "password" },
      runtime: { enabled: true },
    } as Provider,
    {
      identity: { id: "key", protocol: "api", baseUrl: "https://key.example.com" },
      auth: { mode: "apiKey" },
      runtime: { enabled: true },
    } as Provider,
  ]);
  const second = providerAnnouncementSourceSignature([
    {
      identity: { id: "b", protocol: "newApi", baseUrl: "https://relay.example.com" },
      auth: { mode: "password" },
      runtime: { enabled: true },
    } as Provider,
  ]);
  assert.equal(first, second);
});

test("announcement titles are stripped of site HTML and inline Markdown", () => {
  const item = {
    title: "<strong>系统维护 &amp; 升级</strong>",
    titleIsDerived: false,
    content: "<p>今晚 **23:00** 开始</p>",
  } as SiteAnnouncement;

  assert.equal(siteAnnouncementExplicitTitle(item), "系统维护 & 升级");
  assert.equal(plainSiteAnnouncementText("版本 &#x35; 已发布"), "版本 5 已发布");
});

test("title-only announcements still render their own title", () => {
  const item = { title: "仅标题公告", titleIsDerived: false, content: "" } as SiteAnnouncement;
  assert.equal(siteAnnouncementExplicitTitle(item), "仅标题公告");
});

test("explicit titles stay separate from the rendered body", () => {
  const titled = {
    title: " 维护通知 ",
    titleIsDerived: false,
    content: "今晚 23:00 开始",
  } as SiteAnnouncement;
  assert.equal(siteAnnouncementExplicitTitle(titled), "维护通知");

  // NewAPI 控制台公告没有单独标题，正文第一行就是标题，只渲染一次。
  const untitled = { title: "", titleIsDerived: true, content: "第一行公告\n第二行说明" } as SiteAnnouncement;
  assert.equal(siteAnnouncementExplicitTitle(untitled), "");
});

test("titles extracted from the body are not rendered a second time", () => {
  // 后端从正文第一行提取标题，只用于本地识别；界面再渲染一次就与正文重复。
  const derived = {
    title: "误封禁部分用户，处理中",
    titleIsDerived: true,
    content: "误封禁部分用户，处理中",
  } as SiteAnnouncement;
  assert.equal(siteAnnouncementExplicitTitle(derived), "");
});

test("only known announcement levels get a label and a styled key", () => {
  assert.equal(siteAnnouncementLevelLabel("warning"), "注意");
  assert.equal(siteAnnouncementLevelLabel(" Error "), "故障");
  assert.equal(siteAnnouncementLevelKey("ongoing"), "ongoing");
  // default 和未知取值按普通公告显示，不编造级别。
  assert.equal(siteAnnouncementLevelLabel("default"), "");
  assert.equal(siteAnnouncementLevelLabel(""), "");
  assert.equal(siteAnnouncementLevelKey("maintenance"), "");
});

test("timeline groups announcements by site and keeps recency order", () => {
  const item = (
    providerId: string,
    id: string,
    publishedAt: string,
    readAt: string | null = null,
  ) =>
    ({
      providerId,
      providerName: `${providerId}.example.com`,
      providerProtocol: "newApi",
      source: "console",
      id,
      fingerprint: `${providerId}:${id}`,
      title: id,
      titleIsDerived: false,
      content: id,
      level: "",
      extra: "",
      publishedAt,
      updatedAt: null,
      readAt,
      canMarkRead: false,
    }) as SiteAnnouncement;
  const items = [
    item("a", "a1", "2026-10-20T10:00:00Z"),
    item("a", "a2", "2026-10-18T10:00:00Z", "2026-10-18T11:00:00Z"),
    item("b", "b1", "2026-10-19T10:00:00Z"),
  ];

  const groups = groupSiteAnnouncements(items, (entry) => Boolean(entry.readAt));

  assert.deepEqual(
    groups.map((group) => [group.providerId, group.items.map((entry) => entry.id)]),
    [
      ["a", ["a1", "a2"]],
      ["b", ["b1"]],
    ],
  );
  assert.deepEqual(
    groups.map((group) => group.unreadCount),
    [1, 1],
  );
  // 站点之间按各自最新一条公告排序，不按站点名。
  assert.equal(groupSiteAnnouncements([...items].reverse(), () => false)[0].providerId, "a");
});

test("one site keeps its two announcement sources in separate sections", () => {
  const item = (id: string, source: SiteAnnouncement["source"], readAt: string | null = null) =>
    ({
      providerId: "provider-1",
      providerName: "relay.example.com",
      providerProtocol: "newApi",
      source,
      id,
      fingerprint: `provider-1:${source}:${id}`,
      title: id,
      titleIsDerived: false,
      content: id,
      level: "",
      extra: "",
      publishedAt: "2026-10-20T10:00:00Z",
      updatedAt: null,
      readAt,
      canMarkRead: source === "console",
    }) as SiteAnnouncement;

  const groups = groupSiteAnnouncements(
    [item("console-1", "console"), item("notice", "siteNotice")],
    (entry) => Boolean(entry.readAt),
  );

  assert.equal(groups.length, 1);
  // 站点总公告长期有效，排在控制台公告之前，而不是按时间混在一起。
  assert.deepEqual(
    groups[0].sections.map((section) => [section.source, section.label]),
    [
      ["siteNotice", "站点公告"],
      ["console", "控制台公告"],
    ],
  );
  assert.deepEqual(
    groups[0].sections.map((section) => section.items.map((entry) => entry.id)),
    [["notice"], ["console-1"]],
  );
  assert.equal(groups[0].unreadCount, 2);
});
