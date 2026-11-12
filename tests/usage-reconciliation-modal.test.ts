import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, nextTick, reactive } from "vue";
import { loadSource } from "./helpers/load-source.ts";
import type {
  AgentUsageReport,
  AgentUsageReportRequest,
} from "../src/stores/agent-usage-types.ts";

type SetupComponent = {
  setup: (props: any, context: any) => Record<string, any>;
};

function mount(t: TestContext, component: SetupComponent, props: any = {}) {
  const renderer = createRenderer<object, object>({
    patchProp() {},
    insert() {},
    remove() {},
    createElement: () => ({}),
    createText: () => ({}),
    createComment: () => ({}),
    setText() {},
    setElementText() {},
    parentNode: () => null,
    nextSibling: () => null,
  });
  let state!: Record<string, any>;
  const emitted: { name: string; args: unknown[] }[] = [];
  const app = renderer.createApp({
    setup() {
      state = component.setup(props, {
        expose() {},
        emit(name: string, ...args: unknown[]) {
          emitted.push({ name, args });
        },
      });
      return () => null;
    },
  });
  app.mount({});
  let active = true;
  const dispose = () => {
    if (active) {
      active = false;
      app.unmount();
    }
  };
  t.after(dispose);
  return { state, emitted, dispose };
}

async function settle() {
  for (let index = 0; index < 20; index += 1) await Promise.resolve();
  await nextTick();
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function summary(overrides: Record<string, unknown> = {}) {
  return {
    tokens: {
      inputTokens: 1200,
      outputTokens: 300,
      cacheReadTokens: 400,
      cacheWriteTokens: 0,
      reasoningTokens: 0,
    },
    models: [
      {
        model: "claude-sonnet-4-5",
        tokens: {
          inputTokens: 1200,
          outputTokens: 300,
          cacheReadTokens: 400,
          cacheWriteTokens: 0,
          reasoningTokens: 0,
        },
        records: 2,
        estimatedCostMicrousd: 2500,
      },
    ],
    partial: false,
    nativeCumulative: false,
    records: 2,
    estimatedCostMicrousd: 2500,
    unpricedRecords: 0,
    ...overrides,
  };
}

function report(overrides: Partial<AgentUsageReport> = {}): AgentUsageReport {
  return {
    period: "7d",
    generatedAt: "2027-01-15T08:00:00+08:00",
    windowStart: "2027-01-08T08:00:00+08:00",
    windowEnd: "2027-01-15T08:00:00+08:00",
    scannedSessions: 4,
    countedSessions: 3,
    skippedSessions: 1,
    undatedSessions: 0,
    partial: false,
    truncated: false,
    totals: summary(),
    accounts: [
      {
        providerId: "alpha",
        providerName: "Alpha",
        accountLabel: "demo",
        cliKind: "codex",
        sessions: 3,
        usage: summary(),
      },
    ],
    projects: [
      {
        path: "/work/alpha",
        sessions: 3,
        usage: summary(),
      },
    ],
    sites: [
      {
        providerId: "alpha",
        providerName: "Alpha",
        localTokens: 1500,
        error: null,
        usage: {
          requestCount: 6,
          tokenUsed: 2100,
        },
      },
    ],
    diagnostics: [],
    accountRetentionDays: 7,
    pricingError: null,
    ...overrides,
  };
}

function modal(
  t: TestContext,
  bindings: Record<string, unknown> = {},
  props: Record<string, unknown> = {},
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/agent-workspace/UsageReconciliationModal.vue",
    {
      "@lucide/vue": { RefreshCw: {} },
      "../../api/agent-usage": { getAgentUsageReport: async () => report() },
      "../../api/agent-sessions": { cancelAgentSessionQuery: async () => {} },
      "../../styles/modules/usage-reconciliation.css": {},
      "../../utils/promise-timeout": {
        withTimeout: (promise: Promise<unknown>) => promise,
      },
      "../../utils/cli-environment": { fullAgentCliLabel: (kind: string) => kind },
      "../../utils/session-usage-display": {
        usageCount: (value: number) => value.toLocaleString("zh-CN"),
        usageCostLabel: () => "$0.0025",
        usageCostValue: () => "$0.0025",
        usagePriceNote: () => "",
        usageTokenAmount: () => "1,500",
        usageTokenLabel: () => "1,500 token",
        usageTokenTotal: (tokens: {
          inputTokens: number | null;
          outputTokens: number | null;
        }) => (tokens.inputTokens ?? 0) + (tokens.outputTokens ?? 0),
      },
      "../../utils/usage-trend": {
        usagePeriodOptions: [
          { label: "今日", value: "24h" },
          { label: "7 天", value: "7d" },
          { label: "30 天", value: "30d" },
        ],
        usagePeriodTitle: (period: string) => `周期 ${period}`,
      },
      ...bindings,
    },
  );
  if (Object.keys(props).length > 0) return mount(t, component, props);
  return mount(t, component, { visible: true });
}

test("opening the modal loads the local report and its session totals", async (t) => {
  const requests: AgentUsageReportRequest[] = [];
  const { state } = modal(t, {
    "../../api/agent-usage": {
      getAgentUsageReport: async (request: AgentUsageReportRequest) => {
        requests.push(request);
        return report();
      },
    },
  });
  await settle();
  assert.equal(requests.length, 1);
  assert.equal(requests[0].period, "7d");
  assert.equal(requests[0].includeSiteUsage, true);
  assert.ok(requests[0].consumerId);
  assert.equal(state.loading.value, false);
  assert.equal(state.report.value.countedSessions, 3);
  assert.equal(state.summary.value, "计入 3 / 扫描 4 个会话 · 跳过 1 个");
});

test("a late report from a closed modal never writes back", async (t) => {
  const pending = deferred<AgentUsageReport>();
  const props = reactive({ visible: true });
  const { state } = modal(
    t,
    {
      "../../api/agent-usage": {
        getAgentUsageReport: () => pending.promise,
      },
    },
    props,
  );
  await settle();
  assert.equal(state.loading.value, true);

  props.visible = false;
  await settle();
  assert.equal(state.loading.value, false);
  assert.equal(state.report.value, null);

  pending.resolve(report());
  await settle();
  assert.equal(state.report.value, null);
});

test("closing the modal cancels the backend request with its last request id", async (t) => {
  const cancelled: { consumerId: string; requestId: number }[] = [];
  const pending = deferred<AgentUsageReport>();
  const props = reactive({ visible: true });
  const { state } = modal(
    t,
    {
      "../../api/agent-usage": { getAgentUsageReport: () => pending.promise },
      "../../api/agent-sessions": {
        cancelAgentSessionQuery: async (request: {
          consumerId: string;
          requestId: number;
        }) => {
          cancelled.push(request);
        },
      },
    },
    props,
  );
  await settle();
  assert.equal(state.loading.value, true);
  props.visible = false;
  await settle();
  assert.equal(cancelled.length, 1);
  assert.equal(cancelled[0].requestId, 1);
  pending.resolve(report());
  await settle();
});

test("a failed request releases the busy state and shows the error", async (t) => {
  const { state } = modal(t, {
    "../../api/agent-usage": {
      getAgentUsageReport: async () => {
        throw new Error("读取失败");
      },
    },
  });
  await settle();
  assert.equal(state.loading.value, false);
  assert.equal(state.error.value, "读取失败");
  assert.equal(state.report.value, null);
});

test("switching the period reloads with the new period", async (t) => {
  const periods: string[] = [];
  const { state } = modal(t, {
    "../../api/agent-usage": {
      getAgentUsageReport: async (request: AgentUsageReportRequest) => {
        periods.push(request.period);
        return report({ period: request.period });
      },
    },
  });
  await settle();
  state.period.value = "30d";
  await settle();
  assert.deepEqual(periods, ["7d", "30d"]);
  assert.equal(state.report.value.period, "30d");
});

test("a site bill failure still leaves the local report visible", async (t) => {
  const { state } = modal(t, {
    "../../api/agent-usage": {
      getAgentUsageReport: async () =>
        report({
          sites: [
            {
              providerId: "alpha",
              providerName: "Alpha",
              localTokens: 1500,
              error: "站点账单读取失败",
              usage: null,
            },
          ],
        }),
    },
  });
  await settle();
  assert.equal(state.report.value.countedSessions, 3);
  assert.equal(state.report.value.sites[0].error, "站点账单读取失败");
});
