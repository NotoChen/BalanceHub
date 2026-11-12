import assert from "node:assert/strict";
import test from "node:test";

import type { Provider, ProviderApiKeyOption } from "../src/stores/provider-types.ts";
import {
  providerApiKeyDisplayName,
  providerApiKeyCardName,
  providerApiKeySecondaryName,
  providerApiKeyRemark,
  providerCardTitle,
  providerCardScopeNoun,
  providerCardStatusTone,
  providerCooldownNote,
  providerModelScopeLabel,
  providerDefaultApiKeyOption,
  providerEndpointNote,
  providerQuotaScopeLabel,
  providerQuotaHeadline,
  providerQuotaTone,
  providerQuotaBarWindow,
  managedQuotaWindowRows,
  providerQuotaWindowOverflow,
  providerQuotaWindowRows,
  providerQuotaWindowWorstLevel,
  providerHasNoAvailableBalance,
  providerHasQuotaWindows,
  providerStaleSyncNote,
  providerUsesApiKeyOption,
  usedPercent,
  usedPercentLabel,
} from "../src/utils/provider-display.ts";

test("model source labels preserve account and Key scope and a model failure is visible", () => {
  assert.equal(providerModelScopeLabel("account"), "账号可用模型");
  assert.equal(providerModelScopeLabel("apiKey"), "Key 可用模型");
  assert.equal(providerModelScopeLabel(null), "可用模型");
  const value = provider("session");
  value.runtime = { enabled: true, status: "warning", errorMessage: "模型列表获取失败" };
  value.automation = { lastSyncedAt: "123" } as Provider["automation"];
  assert.equal(providerCardStatusTone(value), "warning");
});

function provider(authMode: Provider["auth"]["mode"], remark = "") {
  const displayLabel = authMode === "apiKey"
    ? remark || "Relay Site"
    : remark
      ? `Relay Site · ${remark}`
      : "Relay Site";
  return {
    displayLabel,
    identity: {
      name: "Relay Site",
      remark,
    },
    auth: {
      mode: authMode,
      apiKey: "",
      apiKeyTokenId: "",
      apiKeyOptions: [],
    },
  } as Provider;
}

function apiKeyOption(values: Partial<ProviderApiKeyOption> = {}) {
  return {
    localId: "key-local-1",
    localName: "",
    name: "",
    key: "sk-one",
    tokenId: "token-1",
    ...values,
  } as ProviderApiKeyOption;
}

test("quota progress stays between zero and one while retaining the account balance", () => {
  const value = provider("password");
  value.quota = { available: -1, used: 10001, known: true, totalKnown: true } as Provider["quota"];
  assert.equal(usedPercent(value), 1);
  assert.equal(usedPercentLabel(value), "100.0%");
  assert.equal(value.quota.available, -1);

  value.quota.available = 120;
  value.quota.used = -20;
  assert.equal(usedPercent(value), 0);
  assert.equal(usedPercentLabel(value), "0.0%");

  value.quota.available = 25;
  value.quota.used = 75;
  assert.equal(usedPercent(value), 0.75);
  assert.equal(usedPercentLabel(value), "75.0%");
  value.quota.known = false;
  assert.equal(usedPercentLabel(value), "未公开");
  value.quota.known = true;
  value.quota.unlimited = true;
  assert.equal(usedPercentLabel(value), "不限量");
});

test("account card title appends an optional provider remark", () => {
  assert.equal(providerCardTitle(provider("password", "Claude 主用")), "Relay Site · Claude 主用");
  assert.equal(providerCardTitle(provider("password")), "Relay Site");
});

test("a card calls itself what its protocol is", () => {
  const relay = provider("password");
  relay.identity.protocol = "newApi";
  assert.equal(providerCardScopeNoun(relay), "中转站");

  const gateway = provider("password");
  gateway.identity.protocol = "cliProxyApi";
  assert.equal(providerCardScopeNoun(gateway), "网关");

  const account = provider("password");
  account.identity.protocol = "oauth";
  assert.equal(providerCardScopeNoun(account), "账号");
});

test("API Key card exposes its remark as a standalone heading", () => {
  assert.equal(providerApiKeyRemark(provider("apiKey", "Codex 备用")), "Codex 备用");
  assert.equal(providerApiKeyRemark(provider("password", "账号卡片")), "");
});

test("API Key display keeps local remarks separate from remote names", () => {
  const remarked = apiKeyOption({ localName: "Claude 主用", name: "token-prod" });
  const remoteOnly = apiKeyOption({ localName: "", name: "token-backup" });
  const unnamed = apiKeyOption({ localName: "", name: "" });

  assert.equal(providerApiKeyDisplayName(remarked), "Claude 主用");
  assert.equal(providerApiKeyCardName(remarked), "Claude 主用");
  assert.equal(providerApiKeySecondaryName(remarked), "token-prod");
  assert.equal(providerApiKeyDisplayName(remoteOnly), "token-backup");
  assert.equal(providerApiKeyCardName(remoteOnly), "token-backup");
  assert.equal(providerApiKeySecondaryName(remoteOnly), "");
  assert.equal(providerApiKeyDisplayName(unnamed), "未命名 API Key");
  assert.equal(providerApiKeyCardName(unnamed), "");
  assert.equal(providerApiKeyCardName(apiKeyOption({ name: "当前 API Key" })), "");
  assert.equal(providerApiKeyCardName(apiKeyOption({ name: "当前配置 API Key" })), "");
});

test("default API Key resolution prefers the selected key value then token id fallback", () => {
  const value = provider("apiKey") as Provider;
  const first = apiKeyOption({ localId: "key-one", key: "sk-one", tokenId: "token-1" });
  const second = apiKeyOption({ localId: "key-two", key: "sk-two", tokenId: "token-2" });
  value.auth.apiKeyOptions = [first, second];
  value.auth.apiKey = "sk-one";
  value.auth.apiKeyTokenId = "token-2";

  assert.equal(providerDefaultApiKeyOption(value)?.localId, "key-one");

  value.auth.apiKey = "";
  assert.equal(providerDefaultApiKeyOption(value)?.localId, "key-two");
});

test("configured API Key remains identifiable when the local option cache is empty", () => {
  const value = provider("apiKey") as Provider;
  value.auth.apiKey = "sk-configured";
  const synthetic = apiKeyOption({
    localId: "",
    tokenId: "",
    key: "sk-configured",
    name: "当前配置 API Key",
  });

  assert.equal(providerDefaultApiKeyOption(value), undefined);
  assert.equal(providerUsesApiKeyOption(value, synthetic), true);
  assert.equal(providerUsesApiKeyOption(value, apiKeyOption({ key: "sk-other" })), false);
});

test("a relay balance shares the quota thresholds and wording of account windows", () => {
  const value = provider("apiKey");
  value.automation = { lastSyncedAt: "1700000000" } as Provider["automation"];
  value.quota = { available: 50, used: 50, known: true, totalKnown: true, scope: "token" } as Provider["quota"];
  assert.equal(providerQuotaScopeLabel(value), "API Key 可用额度");

  assert.equal(providerQuotaTone(value), "normal");
  // 75% 已用命中 OAuth 窗口的警告阈值，卡片的余额不再是另一套数字。
  value.quota = { available: 25, used: 75, known: true, totalKnown: true } as Provider["quota"];
  assert.equal(providerQuotaTone(value), "warning");
  value.quota = { available: 5, used: 95, known: true, totalKnown: true } as Provider["quota"];
  assert.equal(providerQuotaTone(value), "high");
  value.quota = { available: 0, used: 10, known: true, totalKnown: true } as Provider["quota"];
  assert.equal(providerQuotaTone(value), "empty");

  value.quota = { available: 0, used: 10, known: true, totalKnown: true, unlimited: true } as Provider["quota"];
  assert.equal(providerQuotaTone(value), "unlimited");
  // 未同步或额度不可知时只说明不确定，不假装余额正常或耗尽。
  value.quota = { available: 0, used: 0, known: false } as Provider["quota"];
  assert.equal(providerQuotaTone(value), "neutral");
  value.quota = { available: 5, used: 5, known: true, totalKnown: false } as Provider["quota"];
  assert.equal(providerQuotaTone(value), "neutral");
  value.automation = { lastSyncedAt: null } as Provider["automation"];
  value.quota = { available: 5, used: 95, known: true, totalKnown: true } as Provider["quota"];
  assert.equal(providerQuotaTone(value), "neutral");
});

test("subscription windows become one readable row of period quotas", () => {
  const now = Date.UTC(2026, 9, 22, 12, 0, 0);
  const value = provider("password");
  value.quota = {
    available: 0,
    used: 0,
    known: true,
    totalKnown: false,
    displayType: "currency",
    currencySymbol: "$",
    windows: [],
  } as Provider["quota"];

  assert.equal(providerHasQuotaWindows(value), false);
  value.quota.windows = [
    {
      label: "今日",
      used: 2.5,
      limit: 10,
      resetsAt: now / 1000 + 3 * 3600,
      history: [{ at: now / 1000 - 3600, usedPercent: 10 }],
    },
    { label: "本周", used: 30, limit: 40 },
    { label: "本月", used: 0, limit: 0 },
    { label: "多余窗口", used: 1, limit: 2 },
  ];
  // 卡片只展示三条窗口，其余窗口进入溢出摘要，而不是被悄悄丢掉。
  const rows = providerQuotaWindowRows(value, now);
  assert.deepEqual(rows.map((row) => row.label), ["今日", "本周", "本月"]);
  const overflow = providerQuotaWindowOverflow(value);
  assert.equal(overflow.count, 1);
  assert.deepEqual(overflow.labels, ["多余窗口"]);
  assert.equal(rows[0].percent, 25);
  assert.equal(rows[0].percentLabel, "25%");
  // 进度条画已用比例，和 OAuth 卡片的窗口条同向：空条表示还有余量。
  assert.equal(rows[0].level, "ok");
  assert.match(rows[0].caption, /3 小时/);
  assert.equal(rows[0].resetAt, now / 1000 + 3 * 3600);
  assert.deepEqual(rows[0].history, [
    { at: now / 1000 - 3600, usedPercent: 10 },
  ]);
  // 卡片窗口条复用 OAuth 的进度条，历史必须原样传过去才会画趋势。
  assert.deepEqual(providerQuotaBarWindow(rows[0]).history, rows[0].history);
  assert.equal(rows[1].percent, 75);
  assert.equal(rows[1].level, "warn");
  assert.equal(rows[1].caption, "");
  // 0 上限是不限量，而不是“已用尽”。
  assert.equal(rows[2].percent, null);
  assert.equal(rows[2].percentLabel, "不限");
  assert.equal(rows[2].level, "unlimited");
});

test("OAuth and CPA cards read their windows with the relay card's bar format", () => {
  const now = Date.UTC(2026, 9, 22, 12, 0, 0);
  const rows = managedQuotaWindowRows(
    [
      {
        id: "5h",
        label: "5 小时",
        model: null,
        usedPercent: 42,
        used: 42,
        limit: 100,
        unit: "%",
        resetAt: now / 1000 + 2 * 3600,
        windowSeconds: 5 * 3600,
        history: [{ at: now / 1000 - 1800, usedPercent: 30 }],
      },
      {
        id: "week",
        label: "每周",
        model: null,
        usedPercent: 78,
        used: 78,
        limit: 100,
        unit: "%",
        resetAt: null,
        windowSeconds: 7 * 86400,
        history: [],
      },
      {
        id: "month",
        label: "每月",
        model: null,
        usedPercent: 10,
        used: 10,
        limit: 100,
        unit: "%",
        resetAt: null,
        windowSeconds: 30 * 86400,
        history: [],
      },
    ],
    now,
  );
  // 卡片列出窗口的条数与中转站卡片相同：最多三条，其余由正文的摘要收起。
  assert.deepEqual(rows.map((row) => row.label), ["5 小时", "每周", "每月"]);
  assert.equal(rows[0].percent, 42);
  assert.equal(rows[0].percentLabel, "42%");
  assert.deepEqual(rows[0].history, [
    { at: now / 1000 - 1800, usedPercent: 30 },
  ]);
  assert.match(rows[0].caption, /2 小时/);
  assert.equal(rows[0].resetAt, now / 1000 + 2 * 3600);
  assert.equal(rows[1].level, "warn");
  assert.equal(rows[1].caption, "");
  // 站点没报上限时写「不限」，不画成 0% 用尽。
  const unlimited = managedQuotaWindowRows(
    [
      {
        id: "all",
        label: "全部",
        model: null,
        usedPercent: null,
        used: null,
        limit: null,
        unit: null,
        resetAt: null,
        windowSeconds: null,
        history: [],
      },
    ],
    now,
  );
  assert.equal(unlimited[0].percent, null);
  assert.equal(unlimited[0].percentLabel, "不限");
  assert.equal(unlimited[0].level, "unlimited");
});

test("a window that only reports a percentage still gets a percentage", () => {
  const now = Date.UTC(2026, 9, 22, 12, 0, 0);
  // 账号接口（Codex / Claude / Gemini）只给 used_percent，used 和 limit 都是
  // 空的。漏掉这一项，每张 OAuth 卡片都会把已知的额度写成「未提供比例」。
  const rows = managedQuotaWindowRows(
    [
      {
        id: "5h",
        label: "5 小时",
        model: null,
        usedPercent: 35,
        used: null,
        limit: null,
        unit: null,
        resetAt: now / 1000 + 3600,
        windowSeconds: 5 * 3600,
        history: [],
      },
      {
        id: "week",
        label: "每周",
        model: null,
        usedPercent: 72.5,
        used: null,
        limit: null,
        unit: null,
        resetAt: null,
        windowSeconds: 7 * 86400,
        history: [],
      },
    ],
    now,
  );
  assert.equal(rows[0].percent, 35);
  assert.equal(rows[0].percentLabel, "35%");
  assert.equal(rows[0].level, "ok");
  assert.equal(rows[1].percent, 72.5);
  assert.equal(rows[1].percentLabel, "72.5%");
  assert.equal(rows[1].level, "warn");
});

test("a subscription account with an empty wallet is judged by its windows", () => {
  const value = provider("password");
  value.automation = { lastSyncedAt: "1750000000" } as Provider["automation"];
  value.quota = {
    available: 0,
    used: 0,
    known: true,
    totalKnown: false,
    displayType: "currency",
    currencySymbol: "$",
    windows: [{ label: "今日", used: 2, limit: 10 }],
  } as Provider["quota"];

  // 钱包只装溢出额度，余额为 0 时窗口还有空间，不能读成“无余额”。
  assert.equal(providerQuotaScopeLabel(value), "钱包余额");
  assert.equal(providerHasNoAvailableBalance(value), false);
  assert.equal(providerQuotaWindowWorstLevel(value), "ok");
  assert.equal(providerQuotaTone(value), "normal");

  // 只有全部有上限的窗口都用尽才算额度用尽。
  value.quota.windows = [
    { label: "今日", used: 10, limit: 10 },
    { label: "本周", used: 35, limit: 40 },
  ];
  assert.equal(providerHasNoAvailableBalance(value), false);
  assert.equal(providerQuotaWindowWorstLevel(value), "high");
  assert.equal(providerQuotaTone(value), "high");

  value.quota.windows = [{ label: "今日", used: 10, limit: 10 }];
  assert.equal(providerHasNoAvailableBalance(value), true);

  // 主数字跟随最紧张的窗口，而不是总是 0 的钱包余额。
  assert.deepEqual(
    { label: providerQuotaHeadline(value).label, value: providerQuotaHeadline(value).value },
    { label: "今日剩余", value: "$0.00" },
  );

  // 不限量窗口永远不算用尽，也不把卡片判成低额度。
  value.quota.windows = [{ label: "今日", used: 3, limit: 0 }];
  assert.equal(providerHasNoAvailableBalance(value), false);
  assert.equal(providerQuotaTone(value), "normal");
  assert.equal(providerQuotaHeadline(value).value, "不限量");

  // 卡片只画前三条，但第四条窗口用尽同样要影响等级、主数字和“已用尽”判断。
  value.quota.windows = [
    { label: "今日", used: 2, limit: 10 },
    { label: "本周", used: 4, limit: 20 },
    { label: "本月", used: 5, limit: 30 },
    { label: "第二组 今日", used: 10, limit: 10 },
  ];
  assert.equal(providerQuotaWindowRows(value).length, 3);
  assert.equal(providerQuotaWindowOverflow(value).count, 1);
  assert.equal(providerQuotaWindowWorstLevel(value), "high");
  assert.equal(providerQuotaTone(value), "high");
  assert.equal(providerQuotaHeadline(value).label, "第二组 今日剩余");
  assert.equal(providerHasNoAvailableBalance(value), false);
});

test("the card headline shows the tightest window's remaining amount", () => {
  const value = provider("password");
  value.quota = {
    available: 12,
    used: 0,
    known: true,
    totalKnown: true,
    displayType: "currency",
    currencySymbol: "$",
    windows: [
      { label: "今日", used: 2.5, limit: 10 },
      { label: "本周", used: 30, limit: 40 },
    ],
  } as Provider["quota"];

  const headline = providerQuotaHeadline(value);
  // 本周用到 75%，比今日的 25% 更紧，因此主数字是本周的剩余额度。
  assert.equal(headline.label, "本周剩余");
  assert.equal(headline.value, "$10.00");
  // 条下的重置时间进悬停提示，具体已用 / 上限在窗口条自己的提示里。
  value.quota.windows = [
    { label: "今日", used: 2.5, limit: 10 },
    { label: "本周", used: 30, limit: 40, resetsAt: 1_700_000_000 },
  ];
  assert.equal(providerQuotaHeadline(value).value, "$10.00");
  assert.match(providerQuotaHeadline(value).title, /重置|待刷新/);

  // 没有窗口时退回钱包口径。卡上只剩一个数字，标签留空：卡型已经说明
  // 这是账号余额。
  value.quota.windows = [];
  assert.equal(providerQuotaHeadline(value).label, "");
  assert.equal(providerQuotaHeadline(value).value, "$12.00");
  assert.equal(providerQuotaScopeLabel(value), "");
});

test("an unlimited key reads as unlimited even when the site never reports a number", () => {
  const value = provider("apiKey");
  value.automation = { lastSyncedAt: "1750000000" } as Provider["automation"];
  // 通用 API Key 没有额度接口：Rust 给出「不限量」但没有数字。卡片要照这个
  // 结论写，不能把已知的「不限量」降级成「未公开」。
  value.quota = { available: 0, used: 0, known: false, totalKnown: false, unlimited: true } as Provider["quota"];
  assert.equal(providerQuotaHeadline(value).value, "不限量");
  assert.equal(providerQuotaTone(value), "unlimited");

  value.quota.unlimited = false;
  assert.equal(providerQuotaHeadline(value).value, "未公开");
  assert.equal(providerQuotaTone(value), "neutral");
});

test("a card only names the sync time once its numbers are stale", () => {
  const now = Date.UTC(2026, 9, 22, 12, 0, 0);
  assert.equal(providerStaleSyncNote(null, now), "");
  assert.equal(providerStaleSyncNote("not a time", now), "");
  // 一天以内的读数仍然可用，卡片不写同步时间。
  assert.equal(providerStaleSyncNote(String(now - 90_000), now), "");
  assert.equal(providerStaleSyncNote(String(now - 3 * 3_600_000), now), "");
  assert.equal(providerStaleSyncNote(String(now - 86_400_000), now), "");
  // 跨过一天才开始提示，秒、毫秒两种写法都认出同一个过期时刻。
  assert.equal(providerStaleSyncNote(String(now - 86_401_000), now), "数据已 1 天未更新");
  assert.equal(providerStaleSyncNote(String((now - 2 * 86_400_000) / 1000), now), "数据已 2 天未更新");
  assert.equal(providerStaleSyncNote(String(now - 2 * 86_400_000), now), "数据已 2 天未更新");
  assert.equal(providerStaleSyncNote(String(now - 5 * 86_400_000), now), "数据已 5 天未更新");
});

test("a cooling card says when automatic refresh resumes", () => {
  const now = Date.UTC(2026, 9, 22, 12, 0, 0);
  const value = provider("session");
  // 停用的卡片不自动刷新，冷却说明没有意义。
  value.runtime = { enabled: false, status: "disabled" };
  assert.equal(providerCooldownNote(value, now).text, "");

  value.runtime = {
    enabled: true,
    status: "error",
    cooldownUntil: (now + 5 * 60_000) / 1000,
    cooldownReason: "站点限流，稍后自动重试",
  } as Provider["runtime"];
  const note = providerCooldownNote(value, now);
  assert.equal(note.text, "冷却中 · 5 分钟后重试");
  assert.match(note.title, /站点限流/);
  assert.match(note.title, /手动刷新不受冷却限制/);

  // 冷却时间已过、状态尚未回写时不显示过期倒计时。
  assert.equal(providerCooldownNote(value, now + 6 * 60_000).text, "");

  // 长冷却按小时和天读，和额度窗口的重置倒计时同一读法。
  value.runtime.cooldownUntil = (now + (2 * 60 + 30) * 60_000) / 1000;
  assert.equal(providerCooldownNote(value, now).text, "冷却中 · 2 小时 30 分后重试");
  value.runtime.cooldownUntil = (now + 49 * 3_600_000) / 1000;
  assert.equal(providerCooldownNote(value, now).text, "冷却中 · 3 天后重试");
});

test("a failed-over card names the backup endpoint it is actually using", () => {
  const value = provider("session");
  // 主地址正常时这一行不出现，卡片不为一件没发生的事留位置。
  assert.deepEqual(providerEndpointNote(value), { text: "", title: "" });

  value.activeEndpoint = {
    backupUrl: "https://backup.example.com/v1",
    backupIndex: 1,
    switchedAt: 1_760_000_000,
  };
  const note = providerEndpointNote(value);
  // 卡片上只写序号和主机名，全文和切换时间放进悬停提示。
  assert.equal(note.text, "备用地址 1 · backup.example.com");
  assert.match(note.title, /主地址不可用，请求正在使用备用地址 1：https:\/\/backup\.example\.com\/v1/);
  assert.match(note.title, /切换时间：/);

  // 地址解析不了时仍写序号，不把整段原始字符串塞进卡片。
  value.activeEndpoint.backupUrl = "backup.example.com/v1";
  assert.equal(providerEndpointNote(value).text, "备用地址 1 · backup.example.com");
});
