import { computed, reactive, watch } from "vue";
import { storeToRefs } from "pinia";
import { Message } from "@arco-design/web-vue";
import { useProviderStore, type Provider } from "../stores/providers";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import { useSettingsStore } from "../stores/settings";
import { useWorkspaceStore } from "../stores/workspaces";
import { useApiKeyManager } from "./useApiKeyManager";
import { useAppDataTransfer } from "./useAppDataTransfer";
import { useAppLifecycle } from "./useAppLifecycle";
import { useAppUpdater } from "./useAppUpdater";
import { useAppVersion } from "./useAppVersion";
import { useAvailableModels } from "./useAvailableModels";
import { useBatchOperation } from "./useBatchOperation";
import { useBackgroundTaskCenter } from "./useBackgroundTaskCenter";
import { useCheckInActions } from "./useCheckInActions";
import { useCheckInRecords } from "./useCheckInRecords";
import { useCliRuntime } from "./useCliRuntime";
import { usePasswordChange } from "./usePasswordChange";
import { useOnboardingController } from "./useOnboardingController";
import { useProviderEditor } from "./useProviderEditor";
import { useProviderActions } from "./useProviderActions";
import { useProviderWorkspaceController } from "./useProviderWorkspaceController";
import { useRequestLogs } from "./useRequestLogs";
import { useSettingsController } from "./useSettingsController";
import { useSiteAnnouncements } from "./useSiteAnnouncements";
import { useSystemNotification } from "./useSystemNotification";
import { useUsageSummary } from "./useUsageSummary";
import { useWindowDrag } from "./useWindowDrag";
import { useWorkspacePicker } from "./useWorkspacePicker";
import { openProjectRepository as openProjectRepositoryCommand } from "../api/app";
import { agentCliLabel } from "../utils/cli-environment";

export function useAppController() {
  const providerStore = useProviderStore();
  const settingsStore = useSettingsStore();
  const workspaceStore = useWorkspaceStore();
  const cliRuntimeStore = useCliRuntimeStore();
  const {
    initialized,
    loadError,
    loading,
    providers,
    providerProtocols,
    refreshInProgress,
    refreshingIds,
  } = storeToRefs(providerStore);
  const { settings } = storeToRefs(settingsStore);
  const { workspaces, temporaryCliPreferences } = storeToRefs(workspaceStore);
  const {
    cliRuntime,
    cliRuntimeLoading,
    cliEnvironmentProbe,
    terminalEnvironmentProbe,
  } = storeToRefs(cliRuntimeStore);

  const { startWindowDrag } = useWindowDrag();

  const settingsController = useSettingsController({
    providers,
    settings,
    initialSettings: settingsStore.settings,
    saveSettings: (value) => settingsStore.save(value),
    probeCliTools: (deep) => cliRuntimeStore.probeCliTools(deep),
  });

  const { notifySystem, sendTestNotification } = useSystemNotification(
    settings,
    settingsController.settingsForm,
  );
  const { appVersion } = useAppVersion();
  const appUpdater = useAppUpdater();

  const appDataTransfer = useAppDataTransfer({
    exportAppData: (path) => providerStore.exportAppData(path),
    importAppData: (path) => providerStore.importAppData(path),
    afterImport: () => {
      settingsController.syncFromSettings();
    },
  });

  const checkIn = useCheckInActions({
    providers,
    reload: () => providerStore.reload(),
    notifySystem,
  });

  const batchOperation = useBatchOperation({
    providers,
    replaceProviders: (nextProviders) => {
      providerStore.replaceProviders(nextProviders);
    },
    upsertProviders: (nextProviders) => providerStore.upsertProviders(nextProviders),
    setRefreshInProgress: (value) => {
      providerStore.refreshInProgress = value;
      if (!value) {
        void providerStore.flushPendingProviderReload();
      }
    },
    refreshCliRuntime: () => cliRuntimeStore.refresh(),
    notifySystem,
  });

  const checkInRecords = useCheckInRecords({
    providers,
    loadRecords: (providerId, month) => providerStore.getCheckInRecords(providerId, month),
  });

  const usage = useUsageSummary({
    loadUsage: (providerId, period) => providerStore.getUsage(providerId, period),
  });

  const requestLogs = useRequestLogs({
    providers,
    loadLogs: (providerId, query) => providerStore.getRequestLogs(providerId, query),
  });

  const passwordChange = usePasswordChange({
    providers,
    changePassword: (providerId, originalPassword, password) =>
      providerStore.changePassword(providerId, originalPassword, password),
  });

  const providerEditor = useProviderEditor({ store: providerStore });
  const editorApiKeyRemoteManaged = computed(() => {
    const providerId = providerEditor.editingProviderId.value;
    return Boolean(
      providerId
        && providerEditor.draftProvider.auth.mode !== "apiKey"
        && providers.value.find((provider) => provider.identity.id === providerId)?.actions.apiKeyManagement,
    );
  });

  const apiKeyManager = useApiKeyManager({
    providers,
    syncRemoteKeys: (providerId) => providerStore.listApiKeys(providerId),
    addLocalKey: (providerId, key, remark) =>
      providerStore.addLocalApiKey(providerId, key, remark),
    createRemoteKey: (providerId, name) => providerStore.createApiKey(providerId, name),
    setRemark: (providerId, localId, remark) =>
      providerStore.setLocalApiKeyRemark(providerId, localId, remark),
    setDefaultKey: (providerId, localId) => providerStore.setDefaultLocalApiKey(providerId, localId),
    removeLocalKey: (providerId, localId) => providerStore.removeLocalApiKey(providerId, localId),
    deleteRemoteKey: (providerId, tokenId) => providerStore.deleteApiKey(providerId, tokenId),
    getProvider: (providerId) => providers.value.find((provider) => provider.identity.id === providerId),
    getBoundAgentLabels: (providerId, localId, current) => [
      ...new Set(
        cliRuntime.value.configs
          .filter((snapshot) =>
            snapshot.providerId === providerId
            && (snapshot.apiKeyLocalId?.trim()
              ? snapshot.apiKeyLocalId.trim() === localId.trim()
              : current),
          )
          .map((snapshot) => agentCliLabel(cliEnvironmentProbe.value, snapshot.cliKind)),
      ),
    ],
    onProviderUpdated: (provider) => {
      providerEditor.syncManagedApiKeys(provider);
      // Key 增删或切换会改变 CLI 配置归属。立即重读本机配置，避免卡片继续展示
      // 已删除 Key 的 Agent 绑定，或把旧绑定误认成新的当前调用 Key。
      void cliRuntimeStore.refresh().catch(() => undefined);
    },
  });

  const availableModels = useAvailableModels({
    providers,
    syncModels: (providerId) => providerStore.syncAvailableModels(providerId),
  });

  const siteAnnouncements = useSiteAnnouncements({
    providers,
    initialized,
    reloadProviders: () => providerStore.reload(),
  });

  const cliRuntimeController = useCliRuntime({
    providers,
    cliRuntime,
    refreshInstances: () => cliRuntimeStore.refreshInstances(),
    activate: (instanceId) => cliRuntimeStore.activate(instanceId),
    previewConfig: (providerId, cliKind, apiKeyLocalId) =>
      cliRuntimeStore.previewConfig(providerId, cliKind, apiKeyLocalId),
    switchConfig: (providerId, cliKind, apiKeyLocalId, revision, files) =>
      cliRuntimeStore.switchConfig(providerId, cliKind, apiKeyLocalId, revision, files),
  });

  async function removeProvider(provider: Provider) {
    await providerStore.removeProvider(provider.identity.id);
  }

  async function toggleProvider(provider: Provider, enabled: boolean) {
    await providerStore.toggleProvider(provider.identity.id, enabled);
  }

  function openProviderApiKeys(provider: Provider) {
    providerEditor.openEditProvider(provider, "credentials");
  }

  async function selectManagedApiKey(option: Provider["auth"]["apiKeyOptions"][number]) {
    const selected = await apiKeyManager.setDefaultManagedApiKey(option);
    if (selected && providerEditor.credentialAssistantState.value === "needApiKeySelection") {
      await providerEditor.selectCredentialApiKey(option);
    }
  }

  watch(
    [providerEditor.drawerVisible, providerEditor.editingProviderId],
    ([visible, providerId]) => {
      if (!visible || !providerId) {
        apiKeyManager.closeApiKeyManager();
        return;
      }
      const provider = providers.value.find((item) => item.identity.id === providerId);
      if (provider) apiKeyManager.bindApiKeyManager(provider);
    },
    { flush: "sync" },
  );

  const onboarding = useOnboardingController({
    initialized,
    loadError,
    providers,
    settings,
    settingsForm: settingsController.settingsForm,
    saveSettings: (value) => settingsStore.save(value),
    syncFromSettings: settingsController.syncFromSettings,
    importAppData: appDataTransfer.importAppData,
    openAddProvider: providerEditor.openAddProvider,
    openSettings: () => {
      settingsController.settingsDrawerVisible.value = true;
    },
  });

  const workspacePicker = useWorkspacePicker({
    workspaces,
    preferences: temporaryCliPreferences,
    terminalKind: computed(() => settings.value.temporaryCliTerminalKind),
    cliEnvironmentProbe,
    terminalEnvironmentProbe,
    probeCliTools: (deep) => cliRuntimeStore.probeCliTools(deep),
    probeTerminals: () => cliRuntimeStore.probeTerminals(),
    listApiKeys: (providerId) => providerStore.listLocalApiKeys(providerId),
    browse: (path) => workspaceStore.browse(path),
    forget: (path) => workspaceStore.forget(path),
    launch: (input) => cliRuntimeStore.launch(input),
    preview: (input) => cliRuntimeStore.previewLaunch(input),
    getInstance: (instanceId) => cliRuntimeStore.getInstance(instanceId),
    searchSessions: (cliKind, workdir, query, forceRefresh) =>
      cliRuntimeStore.searchSessions(cliKind, workdir, query, forceRefresh),
    getSessionDetail: (cliKind, workdir, sessionId) =>
      cliRuntimeStore.getSessionDetail(cliKind, workdir, sessionId),
  });

  const providerActions = useProviderActions({
    providers,
    refreshByIds: async (ids) => {
      const error = await providerStore.refreshByIds(ids);
      if (error) {
        Message.error(`刷新失败：${error}`);
      }
    },
    openWorkspacePicker: workspacePicker.openWorkspacePicker,
    probeCapabilities: (id) => providerStore.probeCapabilities(id),
    getInviteLink: (id) => providerStore.getInviteLink(id),
    reload: () => providerStore.reload(),
    openEditProvider: providerEditor.openEditProvider,
    checkInProviderAction: checkIn.checkInProviderAction,
    openApiKeyManager: openProviderApiKeys,
    openAvailableModels: availableModels.openAvailableModels,
    openUsage: usage.openUsage,
    openRequestLogs: requestLogs.openRequestLogs,
    openPasswordChange: passwordChange.openPasswordChange,
    openCheckInRecords: checkInRecords.openCheckInRecords,
    toggleProvider,
    removeProvider,
  });

  const workspace = useProviderWorkspaceController({
    providers,
    settings,
    checkingInProviderIds: checkIn.checkingInProviderIds,
    probingCapabilitiesProviderId: providerActions.probingCapabilitiesProviderId,
    editingProviderId: providerEditor.editingProviderId,
    probingSite: providerEditor.probingSite,
    testingConnection: providerEditor.testingConnection,
    completingCredentials: providerEditor.completingCredentials,
    reorderProviders: (ids) => providerStore.reorderProviders(ids),
    removeProvider: (id) => providerStore.removeProvider(id),
    toggleProvider: (id, enabled) => providerStore.toggleProvider(id, enabled),
    checkInProvider: (provider) => checkIn.checkInProviderAction(provider),
  });

  useAppLifecycle({
    loadError,
    settings,
    settingsForm: settingsController.settingsForm,
    settingsDrawerVisible: settingsController.settingsDrawerVisible,
    usageVisible: usage.usageVisible,
    usageProvider: usage.usageProvider,
    usagePeriod: usage.usagePeriod,
    checkInRecordsVisible: checkInRecords.checkInRecordsVisible,
    checkInRecordsProviderId: checkInRecords.checkInRecordsProviderId,
    checkInRecordsMonth: checkInRecords.checkInRecordsMonth,
    initialize: () => providerStore.initialize(),
    syncFromSettings: settingsController.syncFromSettings,
    setupThemeListener: settingsController.setupThemeListener,
    cleanupThemeListener: settingsController.cleanupThemeListener,
    syncLaunchAtLogin: settingsController.syncLaunchAtLogin,
    autoProbeCliTools: settingsController.autoProbeCliTools,
    reloadProviders: () => providerStore.reloadProviders().catch(() => {}),
    applyTheme: settingsController.applyTheme,
    resetSettingsDraft: settingsController.resetDraftOnClose,
    resetProviderPointerDrag: workspace.resetProviderPointerDrag,
    refreshUsageSummary: usage.refreshUsageSummary,
    loadCheckInRecords: checkInRecords.loadCheckInRecords,
  });

  async function refreshAllProviders() {
    await batchOperation.runRefresh();
  }

  async function checkInAllProviders() {
    await batchOperation.runCheckIn();
  }

  async function openProjectRepository() {
    try {
      await openProjectRepositoryCommand();
    } catch (error) {
      Message.error(`无法打开 GitHub：${error instanceof Error ? error.message : String(error)}`);
    }
  }

  const globalCheckInInProgress = computed(
    () => batchOperation.running.value && batchOperation.operation.value === "checkIn",
  );

  const backgroundTaskCenter = useBackgroundTaskCenter({
    providers,
    batchOperation: batchOperation.operation,
    batchOperationRunning: batchOperation.running,
    batchOperationItems: batchOperation.items,
    batchOperationError: batchOperation.error,
    batchOperationCompleted: batchOperation.completed,
    refreshInProgress,
    refreshingProviderIds: refreshingIds,
    globalCheckInInProgress,
    checkingInProviderIds: checkIn.checkingInProviderIds,
    checkingForUpdate: appUpdater.checkingForUpdate,
    updateCheckError: appUpdater.updateCheckError,
    installingUpdate: appUpdater.installingUpdate,
    updateDownloadProgress: appUpdater.updateDownloadProgress,
    updateInstallStatus: appUpdater.updateInstallStatus,
    updateInstallError: appUpdater.updateInstallError,
    announcementsLoading: siteAnnouncements.siteAnnouncementsLoading,
    announcementFatalError: siteAnnouncements.siteAnnouncementsFatalError,
    announcementErrors: siteAnnouncements.siteAnnouncementErrors,
    cliRuntimeLoading,
    temporaryCliLaunchTasks: workspacePicker.temporaryCliLaunchTasks,
    probingCapabilitiesProviderId: providerActions.probingCapabilitiesProviderId,
  });

  return reactive({
    initialized,
    loadError,
    loading,
    providers,
    providerProtocols,
    workspaces,
    temporaryCliPreferences,
    cliRuntime,
    cliRuntimeLoading,
    refreshInProgress,
    startWindowDrag,
    ...settingsController,
    ...onboarding,
    sendTestNotification,
    appVersion,
    ...appUpdater,
    ...appDataTransfer,
    ...checkIn,
    batchOperation: batchOperation.operation,
    batchOperationRunning: batchOperation.running,
    batchOperationVisible: batchOperation.visible,
    batchOperationItems: batchOperation.items,
    batchOperationError: batchOperation.error,
    batchOperationStartedAt: batchOperation.startedAt,
    batchOperationFinishedAt: batchOperation.finishedAt,
    batchOperationCompleted: batchOperation.completed,
    globalCheckInInProgress,
    activeBackgroundTasks: backgroundTaskCenter.activeTasks,
    recentBackgroundTasks: backgroundTaskCenter.recentTasks,
    backgroundTaskCount: backgroundTaskCenter.activeTaskCount,
    clearRecentBackgroundTasks: backgroundTaskCenter.clearRecentTasks,
    ...checkInRecords,
    ...usage,
    ...requestLogs,
    ...passwordChange,
    ...apiKeyManager,
    ...availableModels,
    ...siteAnnouncements,
    ...cliRuntimeController,
    ...workspacePicker,
    ...providerEditor,
    editorApiKeyRemoteManaged,
    selectManagedApiKey,
    ...providerActions,
    ...workspace,
    refreshAllProviders,
    checkInAllProviders,
    openProjectRepository,
  });
}
