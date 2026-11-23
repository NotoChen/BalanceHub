import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, nextTick, reactive } from "vue";
import { loadSource } from "./helpers/load-source.ts";
import type { Provider } from "../src/stores/provider-types.ts";
import type { ProviderRecommendationReport } from "../src/stores/provider-recommendation-types.ts";

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

function provider(id: string): Provider {
  return {
    identity: { id, name: id, remark: "", baseUrl: `https://${id}.example.com` },
    auth: { mode: "password" },
    cli: { preferredModel: "" },
    liveness: { model: "" },
    modelConfig: { overrides: {} },
    visibleModels: [],
  } as unknown as Provider;
}

function report(overrides: Partial<ProviderRecommendationReport> = {}): ProviderRecommendationReport {
  return {
    model: "",
    generatedAt: "2027-01-15T08:00:00Z",
    recommendedProviderId: "alpha",
    entries: [
      {
        providerId: "alpha",
        label: "Alpha",
        baseUrl: "https://alpha.example.com",
        score: 92,
        eligible: true,
        model: "",
        modelSupported: null,
        modelWireName: "",
        inputPrice: null,
        outputPrice: null,
        perRequestPrice: null,
        livenessSuccessRate: 0.9,
        livenessSamples: 10,
        siteSuccessRate: null,
        siteAvgTps: null,
        siteAvgLatencyMs: null,
        quotaRemainingPercent: 80,
        quotaLabel: "余额 $8.00",
        resetsAt: null,
        livenessUpdatedAt: null,
        livenessStale: false,
        reasons: ["测活成功率 90%（近 10 次）"],
        blockers: [],
      },
    ],
    ...overrides,
  };
}

type Bindings = Record<string, unknown>;

function modal(
  t: TestContext,
  bindings: Bindings = {},
  props: Record<string, unknown> = {},
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/ProviderRecommendationModal.vue",
    {
      "@lucide/vue": { ArrowRight: {}, RefreshCw: {}, Sparkles: {} },
      "../styles/modules/provider-recommendation.css": {},
      "../api/provider-recommendation": {
        getProviderRecommendations: async () => report(),
      },
      "../utils/promise-timeout": {
        withTimeout: (promise: Promise<unknown>) => promise,
      },
      "../stores/cli-runtime": {
        useCliRuntimeStore: () => ({
          cliEnvironmentProbe: {
            tools: [{ kind: "codex", label: "Codex", available: true }],
          },
        }),
      },
      ...bindings,
    },
  );
  if (Object.keys(props).length > 0) return mount(t, component, props);
  return mount(t, component, {
    visible: true,
    providers: [provider("alpha"), provider("beta")],
  });
}

test("opening the modal asks the backend for a report and shows the recommendation", async (t) => {
  const models: (string | undefined)[] = [];
  const { state } = modal(t, {
    "../api/provider-recommendation": {
      getProviderRecommendations: async (model?: string) => {
        models.push(model);
        return report();
      },
    },
  });
  await settle();
  assert.deepEqual(models, [""]);
  assert.equal(state.loading.value, false);
  assert.equal(state.report.value.entries[0].providerId, "alpha");
  assert.equal(state.summary.value, "1 / 1 个账号可用");
});

test("a late report from a closed modal never writes back", async (t) => {
  const pending = deferred<ProviderRecommendationReport>();
  const props = reactive({ visible: true, providers: [provider("alpha")] });
  const { state } = modal(t, {
    "../api/provider-recommendation": {
      getProviderRecommendations: () => pending.promise,
    },
  }, props);
  await settle();
  assert.equal(state.loading.value, true);

  props.visible = false;
  await settle();
  assert.equal(state.loading.value, false);

  pending.resolve(report());
  await settle();
  assert.equal(state.report.value, null);
});

test("a failed request releases the busy state and shows the error", async (t) => {
  const { state } = modal(t, {
    "../api/provider-recommendation": {
      getProviderRecommendations: async () => {
        throw new Error("读取失败");
      },
    },
  });
  await settle();
  assert.equal(state.loading.value, false);
  assert.equal(state.error.value, "读取失败");
  assert.equal(state.report.value, null);
});

test("switching a recommendation asks the parent with the chosen CLI", async (t) => {
  const { state, emitted } = modal(t);
  await settle();
  state.switchTo("alpha");
  assert.deepEqual(
    emitted.map((event) => event.name),
    ["switch", "close"],
  );
  const [target, cliKind] = emitted[0].args as [Provider, string];
  assert.equal(target.identity.id, "alpha");
  assert.equal(cliKind, "codex");
});

test("switching is ignored without an available CLI", async (t) => {
  const { state, emitted } = modal(t, {
    "../stores/cli-runtime": {
      useCliRuntimeStore: () => ({ cliEnvironmentProbe: { tools: [] } }),
    },
  });
  await settle();
  assert.equal(state.selectedCli.value, "");
  state.switchTo("alpha");
  assert.deepEqual(emitted, []);
});

test("agent context sets the default target and model before ranking", async (t) => {
  const models: (string | undefined)[] = [];
  const { state } = modal(t, {
    "../api/provider-recommendation": {
      getProviderRecommendations: async (model?: string) => {
        models.push(model);
        return report({ model: model ?? "" });
      },
    },
  }, {
    visible: true,
    providers: [provider("alpha"), provider("beta")],
    agent: {
      kind: "codex",
      label: "Codex",
      providerId: "beta",
      providerLabel: "Beta",
      model: "gpt-5",
    },
  });
  await settle();
  assert.equal(state.selectedCli.value, "codex");
  assert.equal(state.model.value, "gpt-5");
  assert.deepEqual(models, ["gpt-5"]);
});

test("CLI 探测晚到时仍按 Agent 上下文选择目标", async (t) => {
  const probe = reactive({
    cliEnvironmentProbe: { tools: [] as Array<{ kind: string; label: string; available: boolean }> },
  });
  const { state } = modal(t, {
    "../stores/cli-runtime": {
      useCliRuntimeStore: () => ({
        get cliEnvironmentProbe() {
          return probe.cliEnvironmentProbe;
        },
      }),
    },
  }, {
    visible: true,
    providers: [provider("alpha")],
    agent: {
      kind: "codex",
      label: "Codex",
      providerId: "alpha",
      providerLabel: "Alpha",
      model: "gpt-5",
    },
  });
  await settle();
  assert.equal(state.selectedCli.value, "");

  probe.cliEnvironmentProbe = {
    tools: [
      { kind: "claudeCode", label: "Claude Code", available: true },
      { kind: "codex", label: "Codex", available: true },
    ],
  };
  await settle();
  assert.equal(state.selectedCli.value, "codex");
});

test("CLI 探测刷新不会把用户手动选择的切换目标改回去", async (t) => {
  const probe = reactive({
    cliEnvironmentProbe: {
      tools: [
        { kind: "codex", label: "Codex", available: true },
        { kind: "claudeCode", label: "Claude Code", available: true },
      ],
    },
  });
  const { state } = modal(t, {
    "../stores/cli-runtime": {
      useCliRuntimeStore: () => ({
        get cliEnvironmentProbe() {
          return probe.cliEnvironmentProbe;
        },
      }),
    },
  }, {
    visible: true,
    providers: [provider("alpha"), provider("beta")],
    agent: {
      kind: "codex",
      label: "Codex",
      providerId: "alpha",
      providerLabel: "Alpha",
      model: "gpt-5",
    },
  });
  await settle();
  assert.equal(state.selectedCli.value, "codex");

  state.selectedCli.value = "claudeCode";
  // 仪表盘每 30 秒会替换一次探测结果；替换后用户的选择必须保留。
  probe.cliEnvironmentProbe = {
    tools: [
      { kind: "codex", label: "Codex", available: true },
      { kind: "claudeCode", label: "Claude Code", available: true },
    ],
  };
  await settle();
  assert.equal(state.selectedCli.value, "claudeCode");
});

test("changing the model hides the previous model's ranking before the new one arrives", async (t) => {
  const pending = deferred<ProviderRecommendationReport>();
  const models: string[] = [];
  const { state } = modal(t, {
    "../api/provider-recommendation": {
      getProviderRecommendations: (model?: string) => {
        models.push(model ?? "");
        return model ? pending.promise : Promise.resolve(report());
      },
    },
  });
  await settle();
  assert.equal(state.report.value?.model, "");

  state.model.value = "gpt-5";
  await nextTick();
  assert.equal(
    state.report.value,
    null,
    "旧模型的排序不能留在屏幕上，否则用户会按旧排名切换、却把新模型写过去",
  );
  assert.equal(state.loading.value, true);

  await new Promise((resolve) => setTimeout(resolve, 400));
  await settle();
  assert.deepEqual(models, ["", "gpt-5"]);
  pending.resolve(report({ model: "gpt-5" }));
  await settle();
  assert.equal(state.report.value?.model, "gpt-5");
  assert.equal(state.loading.value, false);
});

test("closing before the debounce expires does not start a read", async (t) => {
  const models: string[] = [];
  const props = reactive({ visible: true, providers: [provider("alpha")] });
  const { state } = modal(t, {
    "../api/provider-recommendation": {
      getProviderRecommendations: async (model?: string) => {
        models.push(model ?? "");
        return report();
      },
    },
  }, props);
  await settle();
  assert.deepEqual(models, [""]);

  state.model.value = "gpt-5";
  props.visible = false;
  await settle();
  await new Promise((resolve) => setTimeout(resolve, 400));
  await settle();
  assert.deepEqual(models, [""], "关闭后的待命计算不能继续读取推荐");
  assert.equal(state.loading.value, false);
});

test("changing the model re-runs the recommendation after the debounce", async (t) => {
  const models: (string | undefined)[] = [];
  const { state } = modal(t, {
    "../api/provider-recommendation": {
      getProviderRecommendations: async (model?: string) => {
        models.push(model);
        return report({ model: model ?? "" });
      },
    },
  });
  await settle();
  state.model.value = "claude-sonnet-4-5";
  await new Promise((resolve) => setTimeout(resolve, 400));
  await settle();
  assert.deepEqual(models, ["", "claude-sonnet-4-5"]);
  assert.equal(state.report.value.model, "claude-sonnet-4-5");
});
