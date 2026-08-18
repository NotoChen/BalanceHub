import assert from "node:assert/strict";
import test from "node:test";

import type { Provider, SiteAnnouncement } from "../src/stores/provider-types.ts";
import {
  latestSiteAnnouncementTimestamp,
  plainSiteAnnouncementText,
  providerAnnouncementSourceSignature,
  siteAnnouncementDisplayContent,
  siteAnnouncementDisplayTitle,
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

test("announcement display text strips site HTML and common Markdown safely", () => {
  const item = {
    title: "<strong>系统维护 &amp; 升级</strong>",
    content: "<p>今晚 **23:00** 开始</p><p>查看 [状态页](https://status.example.com)</p>",
  } as SiteAnnouncement;

  assert.equal(siteAnnouncementDisplayTitle(item), "系统维护 & 升级");
  assert.equal(
    siteAnnouncementDisplayContent(item.content),
    "今晚 23:00 开始\n\n查看 状态页 (https://status.example.com)",
  );
  assert.equal(plainSiteAnnouncementText("版本 &#x35; 已发布"), "版本 5 已发布");
});

test("title-only announcements keep a readable detail fallback", () => {
  const item = { title: "仅标题公告", content: "" } as SiteAnnouncement;
  assert.equal(siteAnnouncementDisplayTitle(item), "仅标题公告");
  assert.equal(siteAnnouncementDisplayContent(item.content), "公告未提供正文");
});
