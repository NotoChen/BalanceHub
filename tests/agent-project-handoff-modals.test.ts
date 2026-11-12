import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, nextTick, reactive } from "vue";
import { loadSource } from "./helpers/load-source.ts";
import type {
  AgentHandoff,
  AgentHandoffRequest,
} from "../src/stores/agent-handoff-types.ts";
import type {
  AgentProjectReport,
  AgentProjectReportRequest,
} from "../src/stores/agent-project-types.ts";

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

function usage() {
  return {
    tokens: {
      inputTokens: 1200,
      outputTokens: 300,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      reasoningTokens: 0,
    },
    models: [],
    partial: false,
    nativeCumulative: false,
    records: 2,
    estimatedCostMicrousd: 2500,
    unpricedRecords: 0,
  };
}

function projectReport(
  overrides: Partial<AgentProjectReport> = {},
): AgentProjectReport {
  return {
    period: "7d",
    generatedAt: "2027-01-15T08:00:00+08:00",
    windowStart: "2027-01-08T08:00:00+08:00",
    windowEnd: "2027-01-15T08:00:00+08:00",
    sessions: 5,
    undatedSessions: 1,
    decodedSessions: 4,
    skippedSessions: 1,
    partial: false,
    truncated: false,
    projects: [
      {
        workspaceIds: ["ws-alpha", "ws-alpha-wsl"],
        path: "/work/alpha",
        sessions: 5,
        undatedSessions: 1,
        lastActivity: "2027-01-15T07:00:00+08:00",
        agents: [
          { cliKind: "codex", sessions: 3, lastActivity: null },
          { cliKind: "claudeCode", sessions: 2, lastActivity: null },
        ],
        accounts: [
          {
            providerId: "alpha",
            providerName: "Alpha",
            accountLabel: "demo",
            cliKind: "codex",
            sessions: 3,
          },
        ],
        usageSessions: 4,
        usage: usage(),
      },
    ],
    diagnostics: [],
    accountRetentionDays: 7,
    pricingError: null,
    ...overrides,
  };
}

function handoff(overrides: Partial<AgentHandoff> = {}): AgentHandoff {
  return {
    path: "/data/agent-handoffs/handoff-20270115-abc.md",
    content: "# Agent 会话交接\n\n## 最近对话\n\n### 用户\n\n继续完成项目视图",
    sessionTitle: "项目视图",
    sessionId: "session-1",
    workdir: "/work/alpha",
    cliKind: "codex",
    model: "gpt-5",
    messages: 2,
    truncated: false,
    generatedAt: "2027-01-15T08:00:00+08:00",
    ...overrides,
  };
}

function projectProps() {
  return { visible: true };
}

function handoffProps() {
  return {
    visible: true,
    row: {
      sessionRef: "native:session-1",
      session: {
        title: "项目视图",
        updatedAt: "2027-01-15T07:00:00+08:00",
        workdir: "/work/alpha",
        id: "session-1",
        cliKind: "codex",
        model: "gpt-5",
      },
    },
    scopeRevision: "revision-1",
    providers: [
      {
        identity: { id: "alpha" },
        actions: { temporaryCli: true },
      },
    ],
    agents: [
      { kind: "codex", label: "Codex" },
      { kind: "claudeCode", label: "Claude Code" },
    ],
  };
}

function projectModal(
  t: TestContext,
  bindings: Record<string, unknown> = {},
  props: Record<string, unknown> = projectProps(),
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/agent-workspace/AgentProjectModal.vue",
    {
      "@lucide/vue": { FolderGit2: {}, RefreshCw: {} },
      "../AgentCliIcon.vue": {},
      "../../api/agent-projects": {
        getAgentProjects: async () => projectReport(),
      },
      "../../api/agent-sessions": { cancelAgentSessionQuery: async () => {} },
      "../../styles/modules/agent-project.css": {},
      "../../utils/promise-timeout": {
        withTimeout: (promise: Promise<unknown>) => promise,
      },
      "../../utils/cli-environment": {
        fullAgentCliLabel: (kind: string) => kind,
      },
      "../../utils/agent-session-display": {
        sessionTime: () => "刚刚",
      },
      "../../utils/session-usage-display": {
        usageCount: (value: number) => String(value),
        usageCostLabel: () => "$0.0025",
        usageTokenAmount: () => "1,500",
      },
      "../../utils/usage-trend": {
        usagePeriodOptions: [
          { label: "今日", value: "24h" },
          { label: "7 天", value: "7d" },
          { label: "30 天", value: "30d" },
        ],
      },
      ...bindings,
    },
  );
  return mount(t, component, props);
}

function handoffModal(
  t: TestContext,
  bindings: Record<string, unknown> = {},
  props: Record<string, unknown> = handoffProps(),
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/agent-workspace/AgentHandoffModal.vue",
    {
      "@lucide/vue": { Copy: {}, FileDown: {}, Send: {} },
      "@arco-design/web-vue": {
        Message: { success() {}, error() {} },
      },
      "../AgentCliIcon.vue": {},
      "../../api/agent-handoff": {
        prepareAgentHandoff: async () => handoff(),
      },
      "../../api/agent-sessions": { cancelAgentSessionQuery: async () => {} },
      "../../composables/useClipboard": {
        copyText: async () => {},
      },
      "../../styles/modules/agent-handoff.css": {},
      "../../utils/promise-timeout": {
        withTimeout: (promise: Promise<unknown>) => promise,
      },
      "../../utils/cli-environment": {
        fullAgentCliLabel: (kind: string) => kind,
      },
      "../../utils/provider-display": {
        providerDisplayLabel: (provider: { identity: { id: string } }) =>
          provider.identity.id,
      },
      "../../utils/agent-session-display": {
        sessionTime: () => "刚刚",
      },
      ...bindings,
    },
  );
  return mount(t, component, props);
}

test("project view loads the aggregate and filters history by its workspace ids", async (t) => {
  const requests: AgentProjectReportRequest[] = [];
  const { state, emitted } = projectModal(t, {
    "../../api/agent-projects": {
      getAgentProjects: async (request: AgentProjectReportRequest) => {
        requests.push(request);
        return projectReport();
      },
    },
  });
  await settle();
  assert.equal(requests.length, 1);
  assert.equal(requests[0].period, "7d");
  assert.equal(requests[0].includeUsage, true);
  assert.ok(requests[0].consumerId);
  assert.equal(state.loading.value, false);
  assert.equal(state.report.value.projects[0].path, "/work/alpha");
  assert.equal(
    state.summary.value,
    "1 个项目 · 5 个会话 · 计入 4 个",
  );

  state.openProject(state.report.value.projects[0]);
  const filter = emitted.find((event) => event.name === "filter");
  assert.deepEqual(filter?.args, [["ws-alpha", "ws-alpha-wsl"]]);
});

test("project view shows scan progress while the report is loading", async (t) => {
  const pending = deferred<AgentProjectReport>();
  const { state } = projectModal(t, {
    "../../api/agent-projects": {
      getAgentProjects: (
        _request: AgentProjectReportRequest,
        onProgress?: (progress: {
          phase: string;
          completed: number;
          total: number;
          message: string;
        }) => void,
      ) => {
        onProgress?.({
          phase: "scanning",
          completed: 1,
          total: 2,
          message: "已扫描 1/2 个来源",
        });
        return pending.promise;
      },
    },
  });
  await settle();
  assert.equal(state.loading.value, true);
  assert.equal(state.progress.value.message, "已扫描 1/2 个来源");
  assert.equal(state.progressPercent.value, 50);

  pending.resolve(projectReport());
  await settle();
  assert.equal(state.loading.value, false);
});

test("a late project report from a closed modal never writes back", async (t) => {
  const pending = deferred<AgentProjectReport>();
  const props = reactive(projectProps());
  const { state } = projectModal(
    t,
    {
      "../../api/agent-projects": {
        getAgentProjects: () => pending.promise,
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

  pending.resolve(projectReport());
  await settle();
  assert.equal(state.report.value, null);
});

test("closing the project view cancels its backend request", async (t) => {
  const cancelled: { consumerId: string; requestId: number }[] = [];
  const pending = deferred<AgentProjectReport>();
  const { state } = projectModal(t, {
    "../../api/agent-projects": {
      getAgentProjects: () => pending.promise,
    },
    "../../api/agent-sessions": {
      cancelAgentSessionQuery: async (request: {
        consumerId: string;
        requestId: number;
      }) => {
        cancelled.push(request);
      },
    },
  });
  await settle();
  state.close();
  await settle();
  assert.equal(cancelled.length, 1);
  assert.equal(cancelled[0].requestId, 1);
  pending.resolve(projectReport());
  await settle();
  assert.equal(state.report.value, null);
});

test("handoff loads the file and launches the target with workdir and prompt", async (t) => {
  const requests: AgentHandoffRequest[] = [];
  const { state, emitted } = handoffModal(t, {
    "../../api/agent-handoff": {
      prepareAgentHandoff: async (request: AgentHandoffRequest) => {
        requests.push(request);
        return handoff();
      },
    },
  });
  await settle();
  assert.equal(requests.length, 1);
  assert.equal(requests[0].scopeRevision, "revision-1");
  assert.equal(requests[0].sessionRef, "native:session-1");
  assert.equal(state.loading.value, false);
  assert.equal(state.handoff.value.path, handoff().path);
  assert.match(state.initialPrompt.value, /handoff-20270115-abc\.md/);
  assert.equal(state.selectedProviderId.value, "alpha");

  state.launch();
  const launched = emitted.find((event) => event.name === "launch");
  assert.ok(launched);
  const [provider, kind, context] = launched.args as [
    { identity: { id: string } },
    string,
    { workdir: string; initialPrompt: string },
  ];
  assert.equal(provider.identity.id, "alpha");
  assert.equal(kind, "codex");
  assert.equal(context.workdir, "/work/alpha");
  assert.match(context.initialPrompt, /交接文件/);
  assert.ok(emitted.some((event) => event.name === "close"));
});

test("a late handoff from a closed modal never writes back", async (t) => {
  const pending = deferred<AgentHandoff>();
  const props = reactive(handoffProps());
  const { state } = handoffModal(
    t,
    {
      "../../api/agent-handoff": {
        prepareAgentHandoff: () => pending.promise,
      },
    },
    props,
  );
  await settle();
  assert.equal(state.loading.value, true);

  props.visible = false;
  await settle();
  assert.equal(state.loading.value, false);
  assert.equal(state.handoff.value, null);

  pending.resolve(handoff());
  await settle();
  assert.equal(state.handoff.value, null);
});

test("closing the handoff cancels its backend request", async (t) => {
  const cancelled: { consumerId: string; requestId: number }[] = [];
  const pending = deferred<AgentHandoff>();
  const { state } = handoffModal(t, {
    "../../api/agent-handoff": {
      prepareAgentHandoff: () => pending.promise,
    },
    "../../api/agent-sessions": {
      cancelAgentSessionQuery: async (request: {
        consumerId: string;
        requestId: number;
      }) => {
        cancelled.push(request);
      },
    },
  });
  await settle();
  state.close();
  await settle();
  assert.equal(cancelled.length, 1);
  assert.equal(cancelled[0].requestId, 1);
  pending.resolve(handoff());
  await settle();
  assert.equal(state.handoff.value, null);
});
