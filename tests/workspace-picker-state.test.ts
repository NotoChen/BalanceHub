import assert from "node:assert/strict";
import test from "node:test";
import { computed, nextTick, ref } from "vue";

import { useWorkspaceApiKeySelection } from "../src/composables/useWorkspaceApiKeySelection.ts";
import { useWorkspaceDirectoryBrowser } from "../src/composables/useWorkspaceDirectoryBrowser.ts";
import { useWorkspaceLaunchFlow } from "../src/composables/useWorkspaceLaunchFlow.ts";
import { useWorkspaceSessionHistory } from "../src/composables/useWorkspaceSessionHistory.ts";
import type {
  CliSessionDetail,
  CliSessionSearchResponse,
  CliSessionSearchResult,
  CliSessionSummary,
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
  const first = deferred<CliSessionSearchResponse>();
  const second = deferred<CliSessionSearchResponse>();
  const visible = ref(true);
  const directoryRef = ref<WorkspaceDirectoryListing | null>(directory("/old"));
  const history = useWorkspaceSessionHistory({
    visible,
    cliKind: ref("codex"),
    sessionMode: ref("history"),
    selectedModel: ref(""),
    directory: directoryRef,
    searchSessions: async (_kind, workdir) => workdir === "/old" ? first.promise : second.promise,
    getSessionDetail: async () => { throw new Error("not expected"); },
  });

  const firstLoad = history.loadWorkspaceSessions();
  directoryRef.value = directory("/current");
  const secondLoad = history.loadWorkspaceSessions();
  second.resolve(searchResponse([searchResult("current")]));
  await secondLoad;
  first.resolve(searchResponse([searchResult("old")]));
  await firstLoad;
  assert.deepEqual(
    history.workspaceSessionResults.value.map((item) => item.session.id),
    ["current"],
  );
});

test("session detail ignores an older selection result", async () => {
  const first = deferred<CliSessionDetail>();
  const second = deferred<CliSessionDetail>();
  const visible = ref(true);
  const history = useWorkspaceSessionHistory({
    visible,
    cliKind: ref("codex"),
    sessionMode: ref("history"),
    selectedModel: ref(""),
    directory: ref(directory("/workspace")),
    searchSessions: async () => searchResponse([]),
    getSessionDetail: async (_kind, _workdir, sessionId) =>
      sessionId === "first" ? first.promise : second.promise,
  });

  const firstOpen = history.openWorkspaceSessionDetail(session("first"));
  const secondOpen = history.openWorkspaceSessionDetail(session("second"));
  second.resolve(sessionDetail("second"));
  await secondOpen;
  first.resolve(sessionDetail("first"));
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
    searchSessions: async () => searchResponse([]),
    getSessionDetail: async () => { throw new Error("not expected"); },
  });
  history.workspaceSessionResults.value = [searchResult("stale")];

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
    selectedSessionTitle: ref(""),
    error: ref(""),
    preview: async (input) => {
      previewInput = input;
      return launchPreview();
    },
    launch: async () => { throw new Error("not expected"); },
    getInstance: async () => null,
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
    error: ref(""),
    preview: async () => pendingPreview.promise,
    launch: async () => { throw new Error("not expected"); },
    getInstance: async () => null,
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

function session(id: string) {
  return {
    id,
    title: id,
    model: "",
    updatedAt: "",
    canResume: true,
  } as CliSessionSummary;
}

function searchResult(id: string): CliSessionSearchResult {
  return {
    session: session(id),
  };
}

function searchResponse(results: CliSessionSearchResult[]): CliSessionSearchResponse {
  return {
    results,
    indexState: "ready",
    indexMessage: null,
  };
}

function sessionDetail(id: string): CliSessionDetail {
  return {
    session: session(id),
    messages: [],
    truncated: false,
    omittedMessageCount: 0,
    contentSource: "test",
  };
}

function provider(id: string, apiKey = "sk-configured") {
  return {
    identity: { id },
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
