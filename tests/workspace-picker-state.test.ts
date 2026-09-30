import assert from "node:assert/strict";
import test from "node:test";
import { computed, nextTick, ref } from "vue";

import { useWorkspaceApiKeySelection } from "../src/composables/useWorkspaceApiKeySelection.ts";
import { useWorkspaceDirectoryBrowser } from "../src/composables/useWorkspaceDirectoryBrowser.ts";
import { useWorkspaceLaunchFlow } from "../src/composables/useWorkspaceLaunchFlow.ts";
import { useWorkspaceSessionHistory } from "../src/composables/useWorkspaceSessionHistory.ts";
import type { AgentSessionDetail, AgentSessionPage, AgentSessionResumeRequest } from "../src/stores/agent-session-types.ts";
import type { AgentSessionQueryApi } from "../src/api/agent-sessions.ts";
import { sessionDetail, sessionPage, sessionRow, sessionScope } from "./agent-session-fixtures.ts";
import type {
  Provider,
  ProviderApiKeyOption,
  TemporaryCliLaunchInput,
  TemporaryCliLaunchResult,
  TemporaryCliLaunchPreview,
  WorkspaceDirectoryListing,
} from "../src/stores/provider-types.ts";

test("late directory results cannot replace the latest browsing target", async () => {
  const first = deferred<WorkspaceDirectoryListing>();
  const second = deferred<WorkspaceDirectoryListing>();
  let request = 0;
  const browser = useWorkspaceDirectoryBrowser({
    browse: async () => (++request === 1 ? first.promise : second.promise),
    forget: async () => [],
  });

  const firstBrowse = browser.browseWorkspaceDirectory("/old");
  const secondBrowse = browser.browseWorkspaceDirectory("/current");
  second.resolve(directory("/current"));
  assert.equal(await secondBrowse, true);
  first.resolve(directory("/old"));
  assert.equal(await firstBrowse, false);
  assert.equal(browser.workspaceDirectory.value?.currentPath, "/current");
});

test("session history ignores results from a previous directory", async () => {
  const first = deferred<AgentSessionPage>();
  const second = deferred<AgentSessionPage>();
  const visible = ref(true);
  const directoryRef = ref<WorkspaceDirectoryListing | null>(directory("/old"));
  const history = useWorkspaceSessionHistory({
    visible,
    cliKind: ref("codex"),
    sessionMode: ref("history"),
    selectedModel: ref(""),
    directory: directoryRef,
    sessionApi: historyApi({ query: async (request) => request.scopeRevision === "scope:/old" ? first.promise : second.promise }),
  });

  const firstLoad = history.loadWorkspaceSessions();
  await settleSessions();
  directoryRef.value = directory("/current");
  const secondLoad = history.loadWorkspaceSessions();
  second.resolve(sessionPage([sessionRow("current")], { scopeRevision: "scope:/current" }));
  await secondLoad;
  first.resolve(sessionPage([sessionRow("old")], { scopeRevision: "scope:/old" }));
  await firstLoad;
  assert.deepEqual(
    history.workspaceSessionResults.value.map((item) => item.session.id),
    ["current"],
  );
});

test("session detail ignores an older selection result", async () => {
  const first = deferred<AgentSessionDetail>();
  const second = deferred<AgentSessionDetail>();
  const visible = ref(true);
  const history = useWorkspaceSessionHistory({
    visible,
    cliKind: ref("codex"),
    sessionMode: ref("history"),
    selectedModel: ref(""),
    directory: ref(directory("/workspace")),
    sessionApi: historyApi({ detail: async (request) => request.sessionRef === "ref:first" ? first.promise : second.promise }),
  });

  await history.loadWorkspaceSessions();
  const firstOpen = history.openWorkspaceSessionDetail(sessionRow("first"));
  const secondOpen = history.openWorkspaceSessionDetail(sessionRow("second"));
  second.resolve(sessionDetail(sessionRow("second")));
  await secondOpen;
  first.resolve(sessionDetail(sessionRow("first")));
  await firstOpen;
  assert.equal(history.workspaceSessionDetail.value?.session.id, "second");
});

test("typing a new session query clears stale results before the debounced search", async () => {
  const history = useWorkspaceSessionHistory({
    visible: ref(true),
    cliKind: ref("codex"),
    sessionMode: ref("history"),
    selectedModel: ref(""),
    directory: ref(directory("/workspace")),
    sessionApi: historyApi(),
  });
  history.workspaceSessionResults.value = [sessionRow("stale")];

  history.workspaceSessionQuery.value = "新的关键字";
  await nextTick();

  assert.deepEqual(history.workspaceSessionResults.value, []);
  assert.equal(history.workspaceSessionsLoading.value, true);
  history.invalidateWorkspaceSessionRequests();
});

test("API Key results cannot cross-write after switching providers", async () => {
  const first = deferred<ProviderApiKeyOption[]>();
  const second = deferred<ProviderApiKeyOption[]>();
  const currentProvider = ref<Provider | null>(provider("first", ""));
  const selection = useWorkspaceApiKeySelection({
    currentProvider,
    listApiKeys: async (providerId) => providerId === "first" ? first.promise : second.promise,
  });

  const firstLoad = selection.loadWorkspaceApiKeys(currentProvider.value!);
  currentProvider.value = provider("second", "");
  selection.resetWorkspaceApiKeys();
  const secondLoad = selection.loadWorkspaceApiKeys(currentProvider.value!);
  second.resolve([apiKey("second-token", "sk-second")]);
  await secondLoad;
  first.resolve([apiKey("first-token", "sk-first")]);
  await firstLoad;
  assert.deepEqual(selection.workspaceApiKeys.value.map((item) => item.tokenId), ["second-token"]);
  assert.equal(selection.workspaceApiKeyLocalId.value, "key-2");
});

test("local API Keys remain selectable when a provider has no remote key endpoint", async () => {
  let remoteRequests = 0;
  const currentProvider = ref<Provider | null>(provider("local", ""));
  currentProvider.value!.actions.apiKeyManagement = false;
  currentProvider.value!.auth.apiKeyOptions = [apiKey("", "sk-local", "key-local")];
  const selection = useWorkspaceApiKeySelection({
    currentProvider,
    listApiKeys: async () => {
      remoteRequests += 1;
      throw new Error("not expected");
    },
  });

  await selection.loadWorkspaceApiKeys(currentProvider.value!);

  assert.equal(remoteRequests, 0);
  assert.deepEqual(selection.workspaceApiKeys.value.map((item) => item.localId), ["key-local"]);
  assert.equal(selection.workspaceApiKeyLocalId.value, "key-local");
  assert.equal(selection.workspaceApiKeyError.value, "");
});

test("legacy token preferences normalize to the stable local API Key identity", async () => {
  const currentProvider = ref<Provider | null>(provider("remote", "sk-remote"));
  currentProvider.value!.auth.apiKeyOptions = [apiKey("legacy-token", "sk-remote", "key-stable")];
  const selection = useWorkspaceApiKeySelection({
    currentProvider,
    listApiKeys: async () => currentProvider.value!.auth.apiKeyOptions,
  });
  selection.resetWorkspaceApiKeys("legacy-token");

  await selection.loadWorkspaceApiKeys(currentProvider.value!);

  assert.equal(selection.workspaceApiKeyLocalId.value, "key-stable");
});

test("a synthetic current API Key is never sent as a local key identity", async () => {
  let previewInput: TemporaryCliLaunchInput | null = null;
  const launchFlow = useWorkspaceLaunchFlow({
    visible: ref(true),
    provider: ref<Provider | null>(provider("provider", "sk-configured")),
    cliKind: ref("codex"),
    cliOptions: ref([{ value: "codex", label: "Codex" }]),
    cliTool: computed(() => cliTool()),
    cliProbe: ref(null),
    terminalKind: ref("terminal"),
    terminalOptions: ref([{ value: "terminal", label: "Terminal" }]),
    directory: ref(directory("/workspace")),
    apiKeys: ref([apiKey("", "sk-configured", "")]),
    apiKeyLocalId: ref("sk-configured"),
    selectedModel: ref(""),
    sessionMode: ref("new"),
    sessionName: ref(""),
    canNameSession: ref(false),
    selectedResumeId: ref(""),
    selectedSessionRef: ref(""),
    sessionScopeRevision: ref(""),
    selectedSessionTitle: ref(""),
    error: ref(""),
    preview: async (input) => {
      previewInput = input;
      return launchPreview();
    },
    launch: async () => { throw new Error("not expected"); },
    getInstance: async () => null,
    resume: async () => { throw new Error("not expected"); },
    notify: { success: () => {}, warning: () => {}, error: () => {} },
  });

  await launchFlow.launchWorkspace();

  assert.ok(previewInput);
  assert.equal(previewInput.apiKey, "sk-configured");
  assert.equal(previewInput.apiKeyLocalId, "");
});

test("closing a launch flow discards a late preview", async () => {
  const pendingPreview = deferred<TemporaryCliLaunchPreview>();
  const visible = ref(true);
  const launchFlow = useWorkspaceLaunchFlow({
    visible,
    provider: ref<Provider | null>(provider("provider")),
    cliKind: ref("codex"),
    cliOptions: ref([{ value: "codex", label: "Codex" }]),
    cliTool: computed(() => ({
      kind: "codex",
      label: "Codex",
      executable: "codex",
      sessionNameHint: "",
      available: true,
      path: "/usr/local/bin/codex",
      version: "1.0.0",
      message: "",
      capabilities: {
        liveness: true,
        temporaryLaunch: true,
        sessionHistory: true,
        sessionSearch: true,
        sessionDetail: true,
        sessionResume: true,
        sessionName: false,
        modelSelection: true,
        defaultConfig: true,
      },
    })),
    cliProbe: ref(null),
    terminalKind: ref("terminal"),
    terminalOptions: ref([{ value: "terminal", label: "Terminal" }]),
    directory: ref(directory("/workspace")),
    apiKeys: ref([]),
    apiKeyLocalId: ref(""),
    selectedModel: ref(""),
    sessionMode: ref("new"),
    sessionName: ref(""),
    canNameSession: ref(false),
    selectedResumeId: ref(""),
    selectedSessionRef: ref(""),
    sessionScopeRevision: ref(""),
    selectedSessionTitle: ref(""),
    error: ref(""),
    preview: async () => pendingPreview.promise,
    launch: async () => { throw new Error("not expected"); },
    getInstance: async () => null,
    resume: async () => { throw new Error("not expected"); },
  });

  const launch = launchFlow.launchWorkspace();
  launchFlow.resetWorkspaceLaunch();
  pendingPreview.resolve({
    providerName: "Provider",
    cliKind: "codex",
    cliPath: "/usr/local/bin/codex",
    args: [],
    terminalKind: "terminal",
    terminalName: "Terminal",
    workdir: "/workspace",
    command: "codex",
    baseUrl: "https://example.com",
    apiKeyLabel: "合成当前 Key",
    apiKey: "sk-configured",
    model: "",
    sessionMode: "new",
    sessionName: "",
    resumeId: "",
    environment: {},
    settingsPath: null,
    settingsContent: null,
  });
  await launch;
  assert.equal(launchFlow.workspaceLaunchPreview.value, null);
  assert.equal(launchFlow.workspaceLaunchPreviewLoading.value, false);
});

test("confirming a launch closes the picker without waiting for terminal dispatch", async () => {
  const visible = ref(true);
  const pendingLaunch = deferred<TemporaryCliLaunchResult>();
  let launchCalled = false;
  const launchFlow = useWorkspaceLaunchFlow({
    visible,
    provider: ref<Provider | null>(provider("provider")),
    cliKind: ref("codex"),
    cliOptions: ref([{ value: "codex", label: "Codex" }]),
    cliTool: computed(() => ({
      kind: "codex",
      label: "Codex",
      executable: "codex",
      sessionNameHint: "",
      available: true,
      path: "/usr/local/bin/codex",
      version: "1.0.0",
      message: "",
      capabilities: {
        liveness: true,
        temporaryLaunch: true,
        sessionHistory: true,
        sessionSearch: true,
        sessionDetail: true,
        sessionResume: true,
        sessionName: false,
        modelSelection: true,
        defaultConfig: true,
      },
    })),
    cliProbe: ref(null),
    terminalKind: ref("terminal"),
    terminalOptions: ref([{ value: "terminal", label: "Terminal" }]),
    directory: ref(directory("/workspace")),
    apiKeys: ref([]),
    apiKeyLocalId: ref(""),
    selectedModel: ref(""),
    sessionMode: ref("new"),
    sessionName: ref(""),
    canNameSession: ref(false),
    selectedResumeId: ref(""),
    selectedSessionRef: ref(""),
    sessionScopeRevision: ref(""),
    selectedSessionTitle: ref(""),
    error: ref(""),
    preview: async () => ({
      providerName: "Provider",
      cliKind: "codex",
      cliPath: "/usr/local/bin/codex",
      args: [],
      terminalKind: "terminal",
      terminalName: "Terminal",
      workdir: "/workspace",
      command: "codex",
      baseUrl: "https://example.com",
      apiKeyLabel: "Codex 主用",
      apiKey: "***",
      model: "",
      sessionMode: "new",
      sessionName: "",
      resumeId: "",
      environment: {},
      settingsPath: null,
      settingsContent: null,
    } satisfies TemporaryCliLaunchPreview),
    launch: async () => {
      launchCalled = true;
      return pendingLaunch.promise;
    },
    getInstance: async () => null,
    resume: async () => { throw new Error("not expected"); },
    notify: {
      success: () => {},
      warning: () => {},
      error: () => {},
    },
  });

  await launchFlow.launchWorkspace();
  assert.equal(launchFlow.workspaceLaunchPreviewVisible.value, true);

  const confirmationResult = launchFlow.confirmWorkspaceLaunch();
  assert.equal(confirmationResult, undefined);
  assert.equal(launchCalled, true);
  assert.equal(visible.value, false);
  assert.equal(launchFlow.workspaceLaunchPreviewVisible.value, false);
  assert.equal(launchFlow.temporaryCliLaunchTasks.value[0]?.status, "running");

  // Resolve the detached promise only after the confirmation handler returned;
  // this proves the UI does not wait for terminal dispatch.
  pendingLaunch.reject(new Error("test dispatch failure"));
  await new Promise<void>((resolve) => globalThis.setTimeout(resolve, 0));
  assert.equal(launchFlow.temporaryCliLaunchTasks.value[0]?.status, "failed");
});

test("history preview confirms through the shared resume request after closing, without a second launch task", async () => {
  const visible = ref(true);
  const selectedSessionRef = ref("ref:source-a:native-id");
  const scopeRevision = ref("scope:at-preview");
  const selectedModel = ref("must-not-override-history");
  const previewReply = deferred<TemporaryCliLaunchPreview>();
  const resumeReply = deferred<unknown>();
  const previews: TemporaryCliLaunchInput[] = [];
  const submissions: Omit<AgentSessionResumeRequest, "requestId">[] = [];
  let legacyLaunches = 0;
  const flow = useWorkspaceLaunchFlow(historyLaunchOptions({
    visible, selectedSessionRef, sessionScopeRevision: scopeRevision, selectedModel,
    preview: async (input) => { previews.push(input); return previewReply.promise; },
    resume: async (input) => {
      assert.equal(visible.value, false, "close the picker before submitting any resume IPC");
      assert.equal(flow.workspaceLaunchPreviewVisible.value, false);
      submissions.push(input);
      return resumeReply.promise;
    },
    launch: async () => { legacyLaunches += 1; throw new Error("history must not use legacy launch"); },
  }));

  const preparing = flow.launchWorkspace();
  assert.equal(previews[0]?.sessionMode, "history");
  assert.equal(previews[0]?.resumeId, "native-id");
  assert.equal(previews[0]?.model, "", "preview cannot claim a model override absent from the resume contract");
  selectedSessionRef.value = "ref:different-selection";
  scopeRevision.value = "scope:new-selection";
  previewReply.resolve({ ...launchPreview(), cliPath: "/fixture/verified/codex", sessionMode: "history", resumeId: "native-id" });
  await preparing;
  assert.equal(flow.workspaceLaunchPreview.value?.cliPath, "/fixture/verified/codex");

  flow.confirmWorkspaceLaunch();
  flow.confirmWorkspaceLaunch();
  assert.deepEqual(submissions, [{
    sessionRef: "ref:source-a:native-id", scopeRevision: "scope:at-preview",
    cliPath: "/fixture/verified/codex", terminalKind: "terminal",
    intent: { kind: "provider", providerId: "provider", apiKeyLocalId: "fixture-local-key" },
  }]);
  assert.equal(legacyLaunches, 0);
  assert.deepEqual(flow.temporaryCliLaunchTasks.value, []);
  assert.equal(flow.workspaceLaunchPreviewLoading.value, false);
  assert.equal(visible.value, false);
  resumeReply.resolve(null);
  await settleSessions();
  assert.equal(visible.value, false, "a background acknowledgement must never reopen the picker");
});

test("history confirmation refuses a native ID without its source reference and scope revision", async () => {
  for (const [sessionRef, scopeRevision] of [["", "scope:valid"], ["ref:valid", ""]]) {
    const error = ref("");
    let previews = 0;
    const flow = useWorkspaceLaunchFlow(historyLaunchOptions({
      selectedSessionRef: ref(sessionRef), sessionScopeRevision: ref(scopeRevision), error,
      preview: async () => { previews += 1; return launchPreview(); },
    }));
    await flow.launchWorkspace();
    assert.equal(previews, 0);
    assert.match(error.value, /来源已失效/);
    assert.equal(flow.workspaceLaunchPreviewVisible.value, false);
    assert.equal(flow.workspaceLaunchPreviewLoading.value, false);
    assert.deepEqual(flow.temporaryCliLaunchTasks.value, []);
  }
});

function historyLaunchOptions(
  overrides: Partial<Parameters<typeof useWorkspaceLaunchFlow>[0]> = {},
): Parameters<typeof useWorkspaceLaunchFlow>[0] {
  return {
    visible: ref(true), provider: ref(provider("provider")), cliKind: ref("codex"),
    cliOptions: ref([{ value: "codex", label: "Codex" }]), cliTool: computed(cliTool), cliProbe: ref(null),
    terminalKind: ref("terminal"), terminalOptions: ref([{ value: "terminal", label: "Terminal" }]),
    directory: ref(directory("/workspace")), apiKeys: ref([apiKey("fixture-token", "fixture-secret", "fixture-local-key")]),
    apiKeyLocalId: ref("fixture-local-key"), selectedModel: ref(""), sessionMode: ref("history"),
    sessionName: ref(""), canNameSession: ref(false), selectedResumeId: ref("native-id"),
    selectedSessionRef: ref("ref:source-a:native-id"), sessionScopeRevision: ref("scope:valid"),
    selectedSessionTitle: ref("合成历史会话"), error: ref(""),
    preview: async () => launchPreview(), getInstance: async () => null,
    launch: async () => { throw new Error("unexpected legacy launch"); },
    resume: async () => { throw new Error("unexpected resume"); },
    notify: { success: () => {}, warning: () => {}, error: () => {} }, ...overrides,
  };
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

function directory(currentPath: string): WorkspaceDirectoryListing {
  return {
    currentPath,
    parentPath: null,
    homePath: "/",
    entries: [],
  };
}

function historyApi(overrides: Partial<AgentSessionQueryApi> = {}): AgentSessionQueryApi {
  return {
    scope: async (path = null) => sessionScope(path),
    query: async (request) => sessionPage([], { scopeRevision: request.scopeRevision }),
    detail: async () => { throw new Error("unexpected detail read"); },
    cancel: async () => undefined,
    ...overrides,
  };
}
async function settleSessions() { for (let index = 0; index < 12; index += 1) await Promise.resolve(); }

function provider(id: string, apiKey = "sk-configured") {
  return {
    displayLabel: id,
    identity: { id, name: id, remark: "" },
    auth: { apiKey, apiKeyOptions: [] },
    cli: { preferredModel: "" },
    actions: { apiKeyManagement: true },
  } as Provider;
}

function apiKey(tokenId: string, key: string, localId?: string) {
  return {
    localId: localId ?? (tokenId === "second-token" ? "key-2" : "key-1"),
    tokenId,
    key,
    keyAvailable: true,
  } as ProviderApiKeyOption;
}

function cliTool() {
  return {
    kind: "codex" as const,
    label: "Codex",
    executable: "codex",
    sessionNameHint: "",
    available: true,
    path: "/usr/local/bin/codex",
    version: "1.0.0",
    message: "",
    capabilities: {
      liveness: true,
      temporaryLaunch: true,
      sessionHistory: true,
      sessionSearch: true,
      sessionDetail: true,
      sessionResume: true,
      sessionName: false,
      modelSelection: true,
      defaultConfig: true,
    },
  };
}

function launchPreview(): TemporaryCliLaunchPreview {
  return {
    providerName: "Provider",
    cliKind: "codex",
    cliPath: "/usr/local/bin/codex",
    args: [],
    terminalKind: "terminal",
    terminalName: "Terminal",
    workdir: "/workspace",
    command: "codex",
    baseUrl: "https://example.com",
    apiKeyLabel: "Codex 主用",
    apiKey: "***",
    model: "",
    sessionMode: "new",
    sessionName: "",
    resumeId: "",
    environment: {},
    settingsPath: null,
    settingsContent: null,
  };
}
