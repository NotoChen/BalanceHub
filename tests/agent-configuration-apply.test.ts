import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, nextTick, reactive, ref, shallowRef } from "vue";
import { loadSource } from "./helpers/load-source.ts";
import { withTimeout } from "../src/utils/promise-timeout.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
async function settle() {
  for (let i = 0; i < 16; i++) await Promise.resolve();
  await nextTick();
}

function mount(t: TestContext, setup: () => Record<string, any>) {  const renderer = createRenderer<object, object>({
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
  const app = renderer.createApp({
    setup() {
      state = setup();
      return () => null;
    },
  });
  app.mount({});
  t.after(() => app.unmount());
  return state;
}

const FUTURE = new Date(Date.now() + 10 * 60_000).toISOString();
const EDIT = {
  editId: "edit-1",
  revision: "rev-1",
  expiresAt: FUTURE,
  agentKind: "codex",
  documents: [
    {
      visualSettings: false,
      sourceId: "config",
      label: "配置",
      path: "/tmp/fixture/config.toml",
      format: "toml",
      creating: false,
      originalText: "a = 1",
      text: "a = 1",
      readOnlyReason: null,
    },
  ],
  diagnostics: [],
  resource: null,
};
const PLAN = {
  token: "plan-1",
  editId: "edit-1",
  expiresAt: FUTURE,
  sourceIds: ["config"],
  changes: [],
  reloadHints: [],
  diagnostics: [],
};

/** Only the store surface the editor reads; pinia unwraps every ref it returns. */
function fakeTasks() {
  const operations = shallowRef<Record<string, any>>({});
  const owned = ref<Record<string, boolean>>({});
  const starting = reactive<Record<string, boolean>>({});
  const startErrors = reactive<Record<string, string>>({});
  return {
    operations,
    owned,
    starting,
    startErrors,
    startDiagnostics: reactive<Record<string, unknown>>({}),
    declined: reactive<Record<string, boolean>>({}),
    ownsEdit: (id: string) => Boolean(owned.value[id]),
    reserve(request: { editId: string }) {
      if (owned.value[request.editId]) return false;
      owned.value[request.editId] = true;
      starting[request.editId] = true;
      return true;
    },
    submit: (_id: string) => Promise.resolve(null),
    operationFor: (id: string) =>
      Object.values(operations.value).find((item: any) => item.editId === id) ?? null,
    cancel: async () => {},
  };
}

function editor(t: TestContext, tasks: ReturnType<typeof fakeTasks>) {
  const { useAgentConfigurationEditor } = loadSource<{
    useAgentConfigurationEditor: () => Record<string, any>;
  }>("composables/useAgentConfigurationEditor.ts", {
    "../api/agent-configuration": {
      discardAgentConfigurationEdit: async () => {},
      planAgentConfigurationSave: async () => PLAN,
      readAgentConfigurationForm: async () => ({ text: "", validation: "" }),
    },
    "../stores/agent-configuration": { useAgentConfigurationStore: () => tasks },
    "../stores/agent-configuration-sources": {
      useAgentConfigurationSourcesStore: () => ({
        forRuntime: () => ({
          hold: () => () => {},
          transaction: async (_kind: string, run: (publish: unknown) => Promise<unknown>) =>
            run(() => {}),
          invalidate() {},
        }),
      }),
    },
    "../utils/agent-configuration-display": {
      agentConfigurationErrorDiagnostics: () => [],
      agentConfigurationErrorMessage: (failure: unknown) => String(failure),
    },
    "../utils/promise-timeout": { withTimeout },
  });
  return mount(t, () => useAgentConfigurationEditor());
}

test("the save panel stays open and reads its own per-file progress", async (t) => {
  const tasks = fakeTasks();
  const state = editor(t, tasks);
  await state.open(async () => EDIT, { agentKind: "codex", workspace: null, label: "Codex" }, "codex");
  state.setText("config", "a = 2");
  await state.prepare();
  assert.equal(state.planVisible.value, true);
  state.confirm();
  assert.equal(
    state.visible.value,
    true,
    "confirming never closes the panel that owns the write",
  );
  assert.equal(state.applyVisible.value, true);
  assert.equal(state.planVisible.value, false);
  tasks.operations.value = {
    "op-1": {
      id: "op-1",
      editId: "edit-1",
      phase: "applying",
      outcome: null,
      canCancel: true,
      files: [{ sourceId: "config", state: "applied", message: null }],
      message: null,
    },
  };
  await settle();
  assert.equal(state.applyDone.value, false, "the panel keeps following the write");
  assert.equal(state.operation.value.files[0].state, "applied");
  tasks.operations.value = {
    ...tasks.operations.value,
    "op-1": { ...tasks.operations.value["op-1"], phase: "completed", outcome: "appliedVerified", revision: 2 },
  };
  await settle();
  assert.equal(state.applyDone.value, true);
});

test("leaving mid-write closes the panel without touching the running operation", async (t) => {
  const tasks = fakeTasks();
  const state = editor(t, tasks);
  await state.open(async () => EDIT, { agentKind: "codex", workspace: null, label: "Codex" }, "codex");
  state.setText("config", "a = 2");
  await state.prepare();
  state.confirm();
  let cancelled = 0;
  tasks.cancel = async () => {
    cancelled++;
  };
  state.closeApply();
  assert.equal(state.visible.value, false, "the panel is closable while writing");
  assert.equal(cancelled, 0, "closing is never a cancel");
});

test("a refused submission releases the panel with the reason instead of a spinner", async (t) => {
  const tasks = fakeTasks();
  const state = editor(t, tasks);
  await state.open(async () => EDIT, { agentKind: "codex", workspace: null, label: "Codex" }, "codex");
  state.setText("config", "a = 2");
  await state.prepare();
  state.confirm();
  tasks.startErrors["edit-1"] = "配置任务未提交";
  tasks.starting["edit-1"] = false;
  await settle();
  assert.equal(state.applyVisible.value, true);
  assert.equal(state.applyDone.value, true, "a failure is a settled result");
  assert.equal(state.submissionError.value, "配置任务未提交");
});
