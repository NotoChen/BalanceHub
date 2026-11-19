import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { effectScope } from "vue";
import type { ModelMetadata } from "../src/api/model-catalog.ts";
import type {
  Provider,
  ProviderGroupPricing,
} from "../src/stores/provider-types.ts";
import {
  buildModelComparisonRows,
  modelComparisonProviders,
  providerModelWireName,
  providerOffersModel,
} from "../src/utils/model-comparison.ts";
import { loadSource } from "./helpers/load-source.ts";

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (error: Error) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function metadata(overrides: Partial<ModelMetadata> = {}): ModelMetadata {
  return {
    id: "gpt-5",
    name: "gpt-5",
    provider: "openai",
    contextWindow: null,
    outputLimit: null,
    reasoning: null,
    toolCall: null,
    inputModalities: [],
    outputModalities: [],
    price: { input: null, output: null, cacheRead: null, cacheWrite: null },
    perRequest: null,
    priceSource: "site",
    priceGroup: "default",
    billing: "token",
    officialPrice: null,
    priceTiers: [],
    longContextBasis: null,
    timePricing: null,
    ...overrides,
  };
}

function site(
  id: string,
  options: {
    visibleModels?: string[];
    overrides?: Record<string, { wireName: string }>;
    pricingModels?: Record<string, unknown>;
    performanceModels?: Record<string, unknown>;
    displayLabel?: string;
    quota?: Partial<Provider["quota"]>;
    groupPricing?: ProviderGroupPricing;
  } = {},
): Provider {
  return {
    displayLabel: options.displayLabel ?? `站点 ${id}`,
    identity: { id, name: `站点 ${id}` },
    visibleModels: options.visibleModels ?? ["gpt-5"],
    modelConfig: { overrides: options.overrides ?? {} },
    capabilities: {
      pricing: {
        models: options.pricingModels ?? {},
        groupPricing: options.groupPricing,
      },
      performance: { models: options.performanceModels ?? {} },
    },
    quota: {
      known: true,
      available: 12.5,
      used: 0,
      totalKnown: false,
      unlimited: false,
      displayType: "currency",
      currencySymbol: "$",
      windows: [],
      scope: "account",
      ...options.quota,
    },
    automation: { lastSyncedAt: "2026-11-01T00:00:00Z" },
  } as unknown as Provider;
}

test("offer detection follows display names, wire names, overrides and site pricing", () => {
  assert.equal(providerOffersModel(site("direct"), "gpt-5"), true);
  assert.equal(
    providerOffersModel(site("other", { visibleModels: [] }), "gpt-5"),
    false,
  );

  const mapped = site("mapped", {
    visibleModels: ["gpt-5-wire"],
    overrides: { "gpt-5": { wireName: "gpt-5-wire" } },
  });
  assert.equal(providerOffersModel(mapped, "gpt-5"), true);
  assert.equal(providerModelWireName(mapped, "gpt-5"), "gpt-5-wire");

  const aliasOnly = site("alias", {
    visibleModels: ["display-name"],
    overrides: { "display-name": { wireName: "gpt-5" } },
  });
  assert.equal(providerOffersModel(aliasOnly, "gpt-5"), true);

  const pricedOnly = site("priced", {
    visibleModels: [],
    pricingModels: { "gpt-5": {} },
  });
  assert.equal(providerOffersModel(pricedOnly, "gpt-5"), true);

  const hidden = site("hidden", {
    visibleModels: ["claude-4"],
    pricingModels: { "gpt-5": {} },
  });
  assert.equal(providerOffersModel(hidden, "gpt-5"), false);
});

test("comparison providers are deduplicated and keep only sites that offer the model", () => {
  const providers = [
    site("a"),
    site("a"),
    site("b", { visibleModels: ["claude-4"] }),
    site("c", { pricingModels: { "gpt-5": {} }, visibleModels: [] }),
  ];
  assert.deepEqual(
    modelComparisonProviders(providers, "gpt-5").map((item) => item.identity.id),
    ["a", "c"],
  );
});

test("rows rank site prices first, leave catalog prices unranked and mark the cheapest", () => {
  const providers = [
    site("a", {
      displayLabel: "站点 A",
      groupPricing: {
        rateMultiplier: 1.2,
        userRateMultiplier: 0.8,
        peakRateEnabled: false,
        peakStart: "",
        peakEnd: "",
        peakRateMultiplier: null,
        longContextPricingEnabled: true,
      },
      performanceModels: {
        "gpt-5-wire": { successRate: 99.5, avgTps: 42, avgLatencyMs: 850 },
      },
      overrides: { "gpt-5": { wireName: "gpt-5-wire" } },
      visibleModels: ["gpt-5-wire"],
    }),
    site("b", { displayLabel: "站点 B" }),
    site("c", { displayLabel: "站点 C" }),
    site("d", { displayLabel: "站点 D" }),
  ];
  const rows = buildModelComparisonRows(
    "gpt-5",
    [
      {
        provider: providers[0],
        metadata: metadata({
          price: { input: 2.5, output: 10, cacheRead: null, cacheWrite: null },
          officialPrice: {
            input: 5,
            output: 15,
            cacheRead: null,
            cacheWrite: null,
          },
        }),
      },
      {
        provider: providers[1],
        metadata: metadata({
          price: { input: 4, output: 12, cacheRead: null, cacheWrite: null },
        }),
      },
      {
        provider: providers[2],
        metadata: metadata({
          price: { input: 1, output: 2, cacheRead: null, cacheWrite: null },
          priceSource: "catalog",
          priceGroup: "",
        }),
      },
      { provider: providers[3], metadata: null, error: "读取失败" },
    ],
    "b",
  );

  assert.deepEqual(
    rows.map((row) => row.providerId),
    ["a", "b", "c", "d"],
  );
  assert.equal(rows[0].lowest, true);
  assert.equal(rows[1].lowest, false);
  assert.equal(rows[1].current, true);
  assert.equal(rows[2].lowest, false);
  assert.equal(rows[2].sourceLabel, "公开参考价");
  assert.equal(rows[0].comparison?.label, "5 折");
  assert.equal(rows[0].quota, "$12.50");
  assert.equal(rows[0].performance, "可用性 99.5% · 42 t/s · 延迟 850ms");
  assert.match(rows[0].scopeNote, /专属倍率 ×0\.8/);
  assert.equal(rows[3].error, "读取失败");
  assert.equal(rows[3].sourceLabel, "暂无价格资料");
});

test("different billing units are not compared as one cheapest price", () => {
  const rows = buildModelComparisonRows("gpt-5", [
    {
      provider: site("token"),
      metadata: metadata({
        price: { input: 2.5, output: 10, cacheRead: null, cacheWrite: null },
      }),
    },
    {
      provider: site("image", { visibleModels: ["gpt-5"] }),
      metadata: metadata({
        billing: "image",
        perRequest: 0.04,
        price: { input: null, output: null, cacheRead: null, cacheWrite: null },
      }),
    },
  ]);
  assert.equal(rows.some((row) => row.lowest), false);
});

test("token rows without an input price do not claim the cheapest badge", () => {
  const rows = buildModelComparisonRows("gpt-5", [
    {
      provider: site("input"),
      metadata: metadata({
        price: { input: 2.5, output: 10, cacheRead: null, cacheWrite: null },
      }),
    },
    {
      provider: site("output"),
      // 输出价更低，但它和输入价不是同一口径，不能据此标成最低价。
      metadata: metadata({
        price: { input: null, output: 1, cacheRead: null, cacheWrite: null },
      }),
    },
  ]);
  assert.equal(rows.some((row) => row.lowest), false);
  assert.equal(
    rows.find((row) => row.providerId === "output")?.price,
    "$1",
  );
});

test("disabled cards never claim the cheapest badge", () => {
  const disabled = {
    ...site("disabled"),
    runtime: { enabled: false },
  } as Provider;
  const rows = buildModelComparisonRows("gpt-5", [
    {
      provider: disabled,
      metadata: metadata({
        price: { input: 1, output: 2, cacheRead: null, cacheWrite: null },
      }),
    },
    {
      provider: site("a"),
      metadata: metadata({
        price: { input: 3, output: 4, cacheRead: null, cacheWrite: null },
      }),
    },
    {
      provider: site("b"),
      metadata: metadata({
        price: { input: 4, output: 5, cacheRead: null, cacheWrite: null },
      }),
    },
  ]);
  assert.equal(rows[0].providerId, "disabled");
  assert.equal(rows[0].disabled, true);
  assert.equal(rows[0].lowest, false);
  assert.equal(rows[1].lowest, true);
});

interface ComparisonState {
  model: { value: string };
  rows: {
    value: Array<{ error: string; metadata: ModelMetadata | null }>;
  };
  loading: { value: boolean };
  error: { value: string };
  open: (model: string) => Promise<void>;
  close: () => void;
}

interface ComparisonModule {
  useModelComparison(options: {
    providers: () => readonly Provider[];
    currentProviderId: () => string | undefined;
  }): ComparisonState;
}

function loadComparison(
  getModelMetadata: (
    ids: string[],
    refresh: boolean,
    providerId?: string,
  ) => Promise<{ models: ModelMetadata[]; error: string | null }>,
  withTimeout: (
    operation: Promise<unknown>,
    timeoutMs: number,
    message: string,
  ) => Promise<unknown> = (operation) => operation,
): ComparisonModule {
  return loadSource<ComparisonModule>("composables/useModelComparison.ts", {
    "../api/model-catalog": { getModelMetadata },
    "../utils/promise-timeout": { withTimeout },
    "../utils/model-comparison": {
      buildModelComparisonRows,
      modelComparisonProviders,
    },
  });
}

test("closing comparison releases loading immediately and ignores a late IPC result", async () => {
  const pending = deferred<{ models: ModelMetadata[]; error: string | null }>();
  const { useModelComparison } = loadComparison(() => pending.promise);
  const scope = effectScope();
  const state = scope.run(() =>
    useModelComparison({
      providers: () => [site("a"), site("b")],
      currentProviderId: () => "a",
    }),
  )!;
  const opening = state.open("gpt-5");
  assert.equal(state.loading.value, true);
  state.close();
  assert.equal(state.loading.value, false);
  assert.equal(state.model.value, "");
  pending.resolve({ models: [metadata()], error: null });
  await opening;
  assert.deepEqual(state.rows.value, []);
  assert.equal(state.loading.value, false);
  scope.stop();
});

test("a comparison timeout releases loading and keeps the failure on its own row", async () => {
  const { useModelComparison } = loadComparison(
    () => new Promise(() => {}),
    (_operation, _timeoutMs, message) => Promise.reject(new Error(message)),
  );
  const scope = effectScope();
  const state = scope.run(() =>
    useModelComparison({
      providers: () => [site("a"), site("b")],
      currentProviderId: () => "a",
    }),
  )!;
  await state.open("gpt-5");
  assert.equal(state.loading.value, false);
  assert.equal(state.rows.value.length, 2);
  assert.match(state.rows.value[0].error, /读取比价资料超时/);
  assert.equal(state.error.value, "没有读取到任何中转站的模型资料");
  scope.stop();
});

test("a stale comparison result cannot replace a newer model", async () => {
  const pending = new Map<
    string,
    Deferred<{ models: ModelMetadata[]; error: string | null }>
  >();
  const { useModelComparison } = loadComparison((ids, _refresh, providerId) => {
    const key = `${ids[0]}:${providerId}`;
    const item = deferred<{
      models: ModelMetadata[];
      error: string | null;
    }>();
    pending.set(key, item);
    return item.promise;
  });
  const scope = effectScope();
  const state = scope.run(() =>
    useModelComparison({
      providers: () => [
        site("a", { visibleModels: ["gpt-5", "claude-4"] }),
      ],
      currentProviderId: () => "a",
    }),
  )!;
  const first = state.open("gpt-5");
  const second = state.open("claude-4");
  pending
    .get("claude-4:a")!
    .resolve({
      models: [metadata({ id: "claude-4", name: "claude-4" })],
      error: null,
    });
  await second;
  assert.equal(state.rows.value[0].metadata?.id, "claude-4");
  pending.get("gpt-5:a")!.resolve({ models: [metadata()], error: null });
  await first;
  assert.equal(state.rows.value[0].metadata?.id, "claude-4");
  assert.equal(state.model.value, "claude-4");
  scope.stop();
});

test("comparison rows expose which cards can be written into an Agent config", () => {
  const relay = {
    ...site("relay"),
    actions: { defaultCliConfig: true, temporaryCli: true },
  } as unknown as Provider;
  const oauth = {
    ...site("oauth"),
    actions: { defaultCliConfig: true, temporaryCli: true },
    auth: { oauth: { vendor: "claude" } },
  } as unknown as Provider;
  const readOnly = {
    ...site("readonly"),
    actions: { defaultCliConfig: false, temporaryCli: false },
  } as unknown as Provider;
  const rows = buildModelComparisonRows("gpt-5", [
    { provider: relay, metadata: metadata() },
    { provider: oauth, metadata: metadata() },
    { provider: readOnly, metadata: metadata() },
  ]);
  const byId = new Map(rows.map((row) => [row.providerId, row]));
  assert.equal(byId.get("relay")?.canSwitch, true);
  assert.equal(byId.get("relay")?.canLaunch, true);
  // A relay credential is not vendor-bound; an OAuth authorization is.
  assert.equal(byId.get("relay")?.allowedKind, null);
  assert.equal(byId.get("oauth")?.allowedKind, "claudeCode");
  assert.equal(byId.get("readonly")?.canSwitch, false);
  assert.equal(byId.get("readonly")?.canLaunch, false);
});

test("the comparison panel switches the bound model through the CLI preview", () => {
  const panel = readFileSync(
    new URL("../src/components/ModelComparisonPanel.vue", import.meta.url),
    "utf8",
  );
  const modal = readFileSync(
    new URL("../src/components/AvailableModelsModal.vue", import.meta.url),
    "utf8",
  );
  const overlays = readFileSync(
    new URL("../src/components/AppOverlays.vue", import.meta.url),
    "utf8",
  );
  const app = readFileSync(
    new URL("../src/App.vue", import.meta.url),
    "utf8",
  );

  assert.match(panel, /切换到此站/);
  assert.match(panel, /availableCliKinds\(store\.cliEnvironmentProbe, "defaultConfig"\)/);
  // 行内按钮必须尊重卡片的写入能力，OAuth 账号只能写进本家 Agent。
  assert.match(panel, /row\.canSwitch/);
  assert.match(panel, /row\.allowedKind/);
  // 先收起模型窗口再打开默认配置编辑器，避免两层弹窗叠加。
  assert.match(
    modal,
    /emit\("update:visible", false\);\s*\n\s*emit\("switch", provider, cliKind, model\);/,
  );
  assert.match(overlays, /switchAvailableModel/);
  assert.match(app, /@switch-available-model="app\.switchProviderCliConfig"/);
});

test("the comparison panel launches a one-off CLI with the compared model", () => {
  const panel = readFileSync(
    new URL("../src/components/ModelComparisonPanel.vue", import.meta.url),
    "utf8",
  );
  const modal = readFileSync(
    new URL("../src/components/AvailableModelsModal.vue", import.meta.url),
    "utf8",
  );
  const overlays = readFileSync(
    new URL("../src/components/AppOverlays.vue", import.meta.url),
    "utf8",
  );
  const app = readFileSync(
    new URL("../src/App.vue", import.meta.url),
    "utf8",
  );
  const picker = readFileSync(
    new URL("../src/composables/useWorkspacePicker.ts", import.meta.url),
    "utf8",
  );

  // 启动与切换是两种能力：临时启动只读卡片能力，不要求能写默认配置。
  assert.match(panel, /availableCliKinds\(store\.cliEnvironmentProbe, "temporaryLaunch"\)/);
  assert.match(panel, /row\.canLaunch/);
  assert.match(panel, /启动/);
  assert.match(
    modal,
    /emit\("update:visible", false\);\s*\n\s*emit\("launch", provider, cliKind, model\);/,
  );
  assert.match(overlays, /launchAvailableModel/);
  assert.match(
    app,
    /@launch-available-model="\(provider, cliKind, model\) => app\.launchTemporaryCli\(provider, cliKind, \{ model \}\)"/,
  );
  // 启动选择器收到的模型优先于卡片偏好和 CLI 默认模型。
  assert.match(picker, /context\?\.model\?\.trim\(\)/);
  // 没有交接模型时，先看该 Agent 的绑定，再回落到中转站全局偏好。
  assert.match(picker, /provider\.cli\.agentModels\?\.\[initialCliKind\]\?\.trim\(\)/);
});

test("可用模型面板用共享的 wireName 规则读取站点表现", () => {
  const modal = readFileSync(
    new URL("../src/components/AvailableModelsModal.vue", import.meta.url),
    "utf8",
  );

  assert.match(modal, /providerModelPerformance\(provider, model\)/);
  // 覆盖率也必须走同一个查找函数，否则列显示有样本、统计却说 0。
  assert.match(modal, /performanceFor\(model\) \? 1 : 0/);
  assert.doesNotMatch(modal, /performance\.value\?\.models\?\.\[model\]/);
});
