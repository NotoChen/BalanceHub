import { ref, watch, onUnmounted } from "vue";
import type { BrowserRuntimeController } from "./useBrowserRuntime";
import type { LoginAccountsController } from "./useLoginAccounts";
import { createProviderLoginFlow } from "../utils/provider-login-flow";
import { startProviderBrowserLogin, cancelProviderBrowserLogin } from "../api/provider-browser-login";
import { Message } from "@arco-design/web-vue";
import type { Provider, ProviderInput } from "../stores/providers";
import { copyText } from "./useClipboard";
import {
  normalizeProviderBaseUrl,
  type ProviderEditorSection,
  type ProviderSaveCompletion,
  type ProviderEditorStore,
} from "./provider-editor-shared";
import { useProviderConnectionTest } from "./useProviderConnectionTest";
import { useProviderCredentialCompletion } from "./useProviderCredentialCompletion";
import { useProviderEditorState } from "./useProviderEditorState";
import { useProviderSave } from "./useProviderSave";
import { normalizeLivenessTiming } from "../utils/liveness-defaults";
import { chooseSameSiteApiKeyAction, confirmAction } from "./provider-credential-dialogs";
import { providerDisplayLabel } from "../utils/provider-display";

interface UseProviderEditorOptions {
  store: ProviderEditorStore;
  browserRuntime: BrowserRuntimeController;
  loginAccounts: LoginAccountsController;
}

export function useProviderEditor(options: UseProviderEditorOptions) {
  const state = useProviderEditorState();
  const {
    drawerVisible,
    editorSession,
    editingProviderId,
    completingCredentials,
    probingSite,
    credentialCompletionMessage,
    credentialCompletionSteps,
    connectionTestResult,
    siteProbeResult,
    protocolDetectionResult,
    protocolSelectionSource,
    protocolSelectionBaseUrl,
    draftProvider,
    availableModels,
    siteNameSourceBaseUrl,
    setApiKeyOptions,
  } = state;

  const { testConnection, testingConnection } = useProviderConnectionTest({
    draftProvider,
    drawerVisible,
    editorSession,
    editingProviderId,
    connectionTestResult,
    testProviderConnection: (input) => options.store.testProviderConnection(input),
  });

  const saveFlow = useProviderSave({
    visible: drawerVisible,
    session: editorSession,
    input: currentProviderInput,
    prepare: () => credentialAssistant.ensureProtocolSelection(),
    canSave: () => !credentialAssistant.credentialAssistantBusy.value && !startingBrowserLogin.value,
    save: (input, saveOptions) => options.store.saveProvider(input, saveOptions),
    resolveConflict: (conflict) => resolveDuplicateConflict(conflict.kind, conflict.existingProviderName),
    accept: acceptSavedProvider,
    completed: (provider) => {
      Message.success("中转站已保存");
      refreshAfterSave(provider);
    },
  });

  const credentialAssistant = useProviderCredentialCompletion({
    draftProvider,
    providerProtocols: () => options.store.providerProtocols,
    drawerVisible,
    editorSession,
    editingProviderId,
    probingSite,
    siteProbeResult,
    protocolDetectionResult,
    protocolSelectionSource,
    protocolSelectionBaseUrl,
    completingCredentials,
    credentialCompletionMessage,
    credentialCompletionSteps,
    siteNameSourceBaseUrl,
    detectProviderProtocol: (input) => options.store.detectProviderProtocol(input),
    probeProviderSite: (input) => options.store.probeProviderSite(input),
    completeProviderCredentials: (input) => options.store.completeProviderCredentials(input),
    createApiKeyForInput: (input, name) => options.store.createApiKeyForInput(input, name),
    generateAccessTokenForInput: (input) => options.store.generateAccessTokenForInput(input),
    setApiKeyOptions,
    saveDraftAndFindProvider: saveFlow.saveDraft,
    refreshAfterSave,
  });

  const startingBrowserLogin = ref(false);
  const loginFlow = createProviderLoginFlow({
    editorSession: () => editorSession.value,
    visible: () => drawerVisible.value,
    input: currentProviderInput,
    ensureRuntime: async () => {
      await options.browserRuntime.refresh(true);
      if (options.browserRuntime.state.value?.ready) return true;
      options.browserRuntime.open();
      Message.info("请先确认安装浏览器组件，完成后再次点击登录并导入");
      return false;
    },
    start: startProviderBrowserLogin,
    chooseAccount: (input) => options.loginAccounts.choose({
      name: input.identity.name.trim() || "此中转站",
      baseUrl: input.identity.baseUrl,
      previousAccountId: input.auth.browserBinding?.accountId ?? null,
    }),
    cancelSelection: options.loginAccounts.cancelSelection,
    cancel: cancelProviderBrowserLogin,
    close: () => { drawerVisible.value = false; },
    pending: (value) => { startingBrowserLogin.value = value; },
    started: () => { Message.info("登录任务已开始，可在后台任务中查看进度或取消"); },
    failed: (message) => { Message.error(message); },
  });
  watch(editorSession, loginFlow.invalidate);
  watch(drawerVisible, (visible) => { if (!visible) loginFlow.invalidate(); });
  onUnmounted(loginFlow.invalidate);

  function openAddProvider() {
    state.openAddProvider();
    credentialAssistant.resetCredentialAssistant();
  }

  function openEditProvider(provider: Provider, initialSection: ProviderEditorSection = "basics") {
    state.openEditProvider(provider, initialSection);
    credentialAssistant.resetCredentialAssistant();
  }

  async function copyDraftApiKey() {
    const value = draftProvider.auth.apiKey.trim();
    if (!value) {
      Message.warning("API 密钥为空");
      return;
    }

    try {
      await copyText(value);
      Message.success("已复制 API 密钥");
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    }
  }

  function acceptSavedProvider(savedProvider: Provider, completion: ProviderSaveCompletion) {
    if (completion === "mergedApiKey") {
      openEditProvider(savedProvider, "credentials");
      refreshAfterSave(savedProvider);
      Message.success(`API Key 已加入“${providerDisplayLabel(savedProvider)}”的认证凭据`);
      return;
    }
    editingProviderId.value = savedProvider.identity.id;
    draftProvider.auth.credentialRevision = savedProvider.auth.credentialRevision;
    draftProvider.auth.browserBinding = savedProvider.auth.browserBinding;
    draftProvider.auth.sessionUpdatedAt = savedProvider.auth.sessionUpdatedAt;
    siteNameSourceBaseUrl.value = normalizeProviderBaseUrl(savedProvider.identity.baseUrl);
  }

  async function resolveDuplicateConflict(
    kind: "sameAccount" | "sameApiKey" | "sameUrlDifferentApiKey",
    existingName: string,
  ) {
    if (kind === "sameUrlDifferentApiKey") {
      return chooseSameSiteApiKeyAction(existingName);
    }
    if (kind === "sameApiKey") {
      const confirmed = await confirmAction(
        "API Key 已存在",
        `“${existingName}”已经保存了相同的 API Key。是否覆盖已有中转站配置？`,
        "覆盖配置",
        "warning",
      );
      return confirmed ? "overwrite" : "cancel";
    }
    const confirmed = await confirmAction(
      "账号已存在",
      `检测到“${existingName}”是同一站点的同一账号。是否覆盖已有中转站配置？`,
      "覆盖配置",
      "warning",
    );
    return confirmed ? "overwrite" : "cancel";
  }

  function currentProviderInput(): ProviderInput {
    normalizeLivenessTiming(draftProvider.liveness);
    return {
      ...draftProvider,
      identity: {
        ...draftProvider.identity,
        backupUrls: normalizeBackupUrls(draftProvider.identity.backupUrls),
        name:
          normalizeProviderBaseUrl(draftProvider.identity.baseUrl) === siteNameSourceBaseUrl.value
            ? draftProvider.identity.name
            : "",
      },
      cli: {
        preferredModel: draftProvider.cli.preferredModel.trim(),
      },
      id: editingProviderId.value ?? undefined,
    };
  }

  function normalizeBackupUrls(values: string[]) {
    const normalized: string[] = [];
    for (const value of values) {
      const url = value.trim().replace(/\/+$/, "");
      if (url && !normalized.includes(url)) {
        normalized.push(url);
      }
    }
    return normalized;
  }

  function refreshAfterSave(provider: Provider | undefined) {
    if (!provider?.runtime.enabled) {
      return;
    }
    const session = editorSession.value;
    void options.store.refreshByIds([provider.identity.id]).then((error) => {
      if (error) {
        Message.error(`保存后刷新失败：${error}`);
      }
    }).catch((error: unknown) => {
      Message.warning(`中转站已保存，额度刷新失败：${error instanceof Error ? error.message : String(error)}`);
    });
    void options.store
      .probeCapabilities(provider.identity.id)
      .then((result) => {
        if (editorSession.value === session && editingProviderId.value === provider.identity.id) {
          availableModels.value = [...(result.provider.capabilities.availableModels || [])];
        }
      })
      .catch(() => undefined);
  }

  return {
    ...state,
    startingBrowserLogin,
    loginAndImport: loginFlow.run,
    openAddProvider,
    openEditProvider,
    copyDraftApiKey,
    testConnection,
    testingConnection,
    saveProvider: saveFlow.run,
    savingProvider: saveFlow.saving,
    providerSaveError: saveFlow.error,
    ...credentialAssistant,
  };
}
