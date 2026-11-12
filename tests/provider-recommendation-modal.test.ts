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
        livenessSuccessRate: 0.9,
        livenessSamples: 10,
        quotaRemainingPercent: 80,
        quotaLabel: "余额 $8.00",
        resetsAt: null,
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
