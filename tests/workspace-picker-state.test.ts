import assert from "node:assert/strict";
import test from "node:test";
import { computed, ref } from "vue";

import { useWorkspaceApiKeySelection } from "../src/composables/useWorkspaceApiKeySelection.ts";
import { useWorkspaceDirectoryBrowser } from "../src/composables/useWorkspaceDirectoryBrowser.ts";
import { useWorkspaceLaunchFlow } from "../src/composables/useWorkspaceLaunchFlow.ts";
import { useWorkspaceSessionHistory } from "../src/composables/useWorkspaceSessionHistory.ts";
import type {
  CliSessionSummary,
  Provider,
  ProviderApiKeyOption,
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
  const first = deferred<CliSessionSummary[]>();
  const second = deferred<CliSessionSummary[]>();
  const visible = ref(true);
  const directoryRef = ref<WorkspaceDirectoryListing | null>(directory("/old"));
  const history = useWorkspaceSessionHistory({
    visible,
    cliKind: ref("codex"),
    sessionMode: ref("history"),
    selectedModel: ref(""),
    directory: directoryRef,
    listSessions: async (_kind, workdir) => workdir === "/old" ? first.promise : second.promise,
  });

  const firstLoad = history.loadWorkspaceSessions();
  directoryRef.value = directory("/current");
  const secondLoad = history.loadWorkspaceSessions();
  second.resolve([session("current")]);
  await secondLoad;
  first.resolve([session("old")]);
  await firstLoad;
  assert.deepEqual(history.workspaceSessions.value.map((item) => item.id), ["current"]);
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
  assert.equal(selection.workspaceApiKeyTokenId.value, "second-token");
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
    apiKeyTokenId: ref(""),
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

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
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

function provider(id: string, apiKey = "sk-configured") {
  return {
    identity: { id },
    auth: { apiKey },
    cli: { preferredModel: "" },
    actions: { apiKeyManagement: true },
  } as Provider;
}

function apiKey(tokenId: string, key: string) {
  return {
    tokenId,
    key,
  } as ProviderApiKeyOption;
}
