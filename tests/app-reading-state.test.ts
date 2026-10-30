import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { effectScope, reactive, ref } from "vue";
import { useLatestRequest } from "../src/composables/useLatestRequest.ts";
import { useRequestLogs } from "../src/composables/useRequestLogs.ts";
import { useUsageSummary } from "../src/composables/useUsageSummary.ts";
import { useCheckInRecords } from "../src/composables/useCheckInRecords.ts";
import { useProviderConnectionTest } from "../src/composables/useProviderConnectionTest.ts";
import { useUsageTrendChart } from "../src/composables/useUsageTrendChart.ts";
import { emptyDraft } from "../src/utils/provider-input.ts";
import type { Provider, ProviderRequestLogsResult, ProviderUsageSummary, ProviderUsageOverview, ProviderCheckInRecordsResult, ProviderConnectionTestResult } from "../src/stores/provider-types.ts";

function scoped<T>(t: TestContext, create: () => T) {
  const scope = effectScope();
  t.after(() => scope.stop());
  return scope.run(create)!;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function settle() { for (let index = 0; index < 12; index++) await Promise.resolve(); }
const provider = (id: string) => ({ identity: { id } }) as Provider;
const quotaDisplay = { quotaDisplayType: "currency", currencySymbol: "$" };
const logs = (id: string, page = 0): ProviderRequestLogsResult => ({ providerId: id, providerName: id, page, pageSize: 20, total: 1, quotaDisplay, stats: { quota: 0, rpm: 0, tpm: 0 }, logs: [], message: "" });
const usage = (id: string, used = 0.005): ProviderUsageSummary => ({ providerId: id, providerName: id, quotaDisplay, points: [{ date: "2026-09-26", used, requestCount: 1, tokenUsed: 2 }], modelStats: [], modelPoints: [], modelHealth: [], requestCount: 1 });
const record = (month: string): ProviderCheckInRecordsResult => ({ providerId: "a", month, records: [], quotaDisplay, message: "" });

test("closing a view releases its busy state before the IPC settles and old work cannot finish a new request", async (t) => {
  const old = deferred<string>();
  const fresh = deferred<string>();
  const published: string[] = [];
  const request = scoped(t, () => useLatestRequest({ timeoutMessage: "读取超时" }));
  const first = request.run(() => old.promise, (value) => published.push(value));
  await settle();
  request.invalidate();
  assert.equal(request.loading.value, false);
  const second = request.run(() => fresh.promise, (value) => published.push(value));
  old.resolve("old");
  await first;
  assert.equal(request.loading.value, true);
  assert.deepEqual(published, []);
  fresh.resolve("fresh");
  await second;
  assert.deepEqual(published, ["fresh"]);
  assert.equal(request.loading.value, false);
});

test("failure and timeout release reads, and a late response cannot erase the failure", async (t) => {
  const old = deferred<string>();
  const published: string[] = [];
  const request = scoped(t, () => useLatestRequest({ timeoutMessage: "读取超时", timeoutMs: 5 }));
  await request.run(() => old.promise, (value) => published.push(value));
  assert.equal(request.loading.value, false);
  assert.equal(request.error.value, "读取超时");
  old.resolve("late");
  await settle();
  assert.deepEqual(published, []);
  await request.run(async () => { throw new Error("离线"); }, () => assert.fail());
  assert.equal(request.loading.value, false);
  assert.equal(request.error.value, "离线");
  await request.run(async () => "recovered", (value) => published.push(value));
  assert.equal(request.error.value, "");
  assert.deepEqual(published, ["recovered"]);
});

test("request log pages and errors stay with their provider and requested page", async (t) => {
  const old = deferred<ProviderRequestLogsResult>();
  const fresh = deferred<ProviderRequestLogsResult>();
  const pages = deferred<ProviderRequestLogsResult>();
  const panel = scoped(t, () => useRequestLogs({ providers: ref([provider("a"), provider("b")]), loadLogs: (id, query) => id === "a" ? old.promise : query.page === 0 ? fresh.promise : pages.promise }));
  panel.openRequestLogs(provider("a"));
  panel.openRequestLogs(provider("b"));
  fresh.resolve(logs("b"));
  await settle();
  old.reject(new Error("old failure"));
  await settle();
  assert.equal(panel.requestLogsResult.value?.providerId, "b");
  assert.equal(panel.requestLogsError.value, "");
  panel.setRequestLogsPage(1);
  assert.equal(panel.requestLogsResult.value, null, "a new page must not display the previous page as its result");
  panel.requestLogsVisible.value = false;
  assert.equal(panel.requestLogsLoading.value, false);
  pages.resolve(logs("b", 1));
  await settle();
  assert.equal(panel.requestLogsResult.value, null);
});

test("changing usage periods cannot label a late 24-hour response as seven-day data", async (t) => {
  const old = deferred<ProviderUsageSummary>();
  const fresh = deferred<ProviderUsageSummary>();
  const panel = scoped(t, () => useUsageSummary({
    loadUsage: (_id, period) => period === "24h" ? old.promise : fresh.promise,
    loadOverview: async () => assert.fail("卡片入口不应读取全部站点汇总"),
  }));
  panel.openUsage(provider("a"));
  panel.usagePeriod.value = "7d";
  fresh.resolve(usage("a", 7));
  await settle();
  old.resolve(usage("a", 24));
  await settle();
  assert.equal(panel.usageSummary.value?.points[0].used, 7);
  assert.equal(panel.usageLoading.value, false);
});

test("the toolbar overview reads every site once and a late card read cannot replace it", async (t) => {
  const overview = deferred<ProviderUsageOverview>();
  const cardRead = deferred<ProviderUsageSummary>();
  const periods: string[] = [];
  const panel = scoped(t, () => useUsageSummary({
    loadUsage: () => cardRead.promise,
    loadOverview: (period) => { periods.push(period); return overview.promise; },
  }));
  panel.openUsageSummary();
  await settle();
  assert.equal(panel.usageProvider.value, null, "汇总视图不绑定单个中转站");
  assert.equal(panel.usageVisible.value, true);
  assert.ok(periods.length >= 1 && periods.every((period) => period === "24h"), `读取了 ${JSON.stringify(periods)}`);
  overview.resolve({ requestCount: 12, modelStats: [], modelHealth: [], errors: [{ providerId: "a", providerName: "A", message: "离线" }] });
  await settle();
  assert.equal(panel.usageOverview.value?.requestCount, 12);
  // 打开另一张卡片的用量：汇总结果和卡片结果不能互相顶替。
  panel.openUsage(provider("a"));
  assert.equal(panel.usageOverview.value, null);
  cardRead.resolve(usage("a", 3));
  await settle();
  assert.equal(panel.usageSummary.value?.points[0].used, 3);
  assert.equal(panel.usageOverview.value, null);
  assert.equal(panel.usageLoading.value, false);
});

test("returning to a cached check-in month releases another month's loading state and explicit refresh still reads", async (t) => {
  const pending = deferred<ProviderCheckInRecordsResult>();
  let calls = 0;
  const panel = scoped(t, () => useCheckInRecords({ providers: ref([provider("a")]), loadRecords: async (_id, month) => { calls++; return month === "2020-01" ? pending.promise : record(month); } }));
  panel.openCheckInRecords(provider("a"));
  await settle();
  assert.equal(calls, 1, "opening starts one read");
  const month = panel.checkInRecordsMonth.value;
  panel.checkInRecordsMonth.value = "2020-01";
  await settle();
  assert.equal(panel.checkInRecordsLoading.value, true);
  panel.checkInRecordsMonth.value = month;
  assert.equal(panel.checkInRecordsLoading.value, false);
  assert.equal(panel.checkInRecordsResult.value?.month, month);
  pending.resolve(record("2020-01"));
  await settle();
  await panel.loadCheckInRecords({ force: true });
  assert.equal(calls, 3);
  assert.equal(panel.checkInRecordsResult.value?.month, month);
});

test("connection tests are single-flight and editing credentials invalidates the old result immediately", async (t) => {
  const pending = deferred<ProviderConnectionTestResult>();
  const draft = reactive(emptyDraft());
  draft.identity.baseUrl = "https://example.invalid";
  const result = ref<ProviderConnectionTestResult | null>(null);
  let calls = 0;
  const panel = scoped(t, () => useProviderConnectionTest({ draftProvider: draft, drawerVisible: ref(true), editorSession: ref(1), editingProviderId: ref(null), connectionTestResult: result, testProviderConnection: () => { calls++; return pending.promise; } }));
  const first = panel.testConnection();
  await panel.testConnection();
  assert.equal(calls, 1);
  draft.auth.loginUsername = "another-account";
  assert.equal(panel.testingConnection.value, false);
  pending.resolve({ ok: true, message: "通过", available: 1, used: 0, quotaDisplay, steps: [] });
  await first;
  assert.equal(result.value, null);
});

test("small usage amounts retain chart contrast and zero-only data does not invent a peak", () => {
  const summary = usage("a");
  summary.modelStats = [{ modelName: "example", used: 0.005, requestCount: 1, tokenUsed: 2 }];
  const chart = useUsageTrendChart({ summary, period: "30d" });
  assert.equal(chart.maxUsageValue.value, 0.005);
  assert.equal(chart.usageMaxModelQuota.value, 0.005);
  assert.equal(chart.usageChartPoints.value[0].y, 26);
  assert.equal(chart.usageAverageRpm.value, 1 / (30 * 24 * 60));
  const empty = useUsageTrendChart({ summary: usage("a", 0), period: "24h" });
  assert.equal(empty.usagePeakPoint.value, null);
  assert.ok(Number.isFinite(empty.usageChartPoints.value[0].y));
});

test("response quality lists the least reliable routes first and drops unsampled models", () => {
  const summary = usage("a");
  summary.modelHealth = [
    { modelName: "steady", successRate: 99.8, avgLatencyMs: 900, avgTps: 42 },
    { modelName: "flaky", successRate: 88.5, avgLatencyMs: 2100, avgTps: 12 },
    { modelName: "unsampled", successRate: null, avgLatencyMs: null, avgTps: null },
    { modelName: "no-tps", successRate: 97, avgLatencyMs: null, avgTps: null },
  ];
  summary.modelStats = [{ modelName: "flaky", used: 1, requestCount: 7, tokenUsed: 90 }];
  const chart = useUsageTrendChart({ summary, period: "24h" });
  assert.deepEqual(
    chart.usageModelHealth.value.map((item) => item.modelName),
    ["flaky", "no-tps", "steady"],
  );
  // 请求次数按模型名从用量统计里对齐，用来提示采样覆盖面。
  assert.equal(chart.usageModelHealth.value[0].requestCount, 7);
  assert.equal(chart.usageModelHealth.value[1].requestCount, 0);
});

test("the granularity note follows the buckets the site actually returned", () => {
  const hourly = usage("a");
  hourly.points = [
    { date: "10-23 08:00", used: 1, requestCount: 1, tokenUsed: 1 },
    { date: "10-23 09:00", used: 2, requestCount: 1, tokenUsed: 1 },
  ];
  assert.equal(useUsageTrendChart({ summary: hourly, period: "24h" }).usagePeriodGranularity.value, "按小时汇总");
  const daily = usage("a");
  daily.points = [{ date: "10-23", used: 1, requestCount: 1, tokenUsed: 1 }];
  // NewAPI 的 24 小时视图只有日桶，不能标成小时数据。
  assert.equal(useUsageTrendChart({ summary: daily, period: "24h" }).usagePeriodGranularity.value, "按天汇总");
  assert.equal(useUsageTrendChart({ summary: hourly, period: "7d" }).usagePeriodGranularity.value, "按天汇总");
});
