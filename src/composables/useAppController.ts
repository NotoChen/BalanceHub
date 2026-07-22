import { reactive } from "vue";
import { storeToRefs } from "pinia";
import { Message } from "@arco-design/web-vue";
import { useProviderStore, type Provider } from "../stores/providers";
import { useApiKeyManager } from "./useApiKeyManager";
import { useAppDataTransfer } from "./useAppDataTransfer";
import { useAppLifecycle } from "./useAppLifecycle";
import { useAppUpdater } from "./useAppUpdater";
import { useAppVersion } from "./useAppVersion";
import { useAvailableModels } from "./useAvailableModels";
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
import { useSystemNotification } from "./useSystemNotification";
import { useUsageSummary } from "./useUsageSummary";
import { useWindowDrag } from "./useWindowDrag";
import { useWorkspacePicker } from "./useWorkspacePicker";

export function useAppController() {
  const providerStore = useProviderStore();
  const {
    initialized,
    cliRuntime,
    cliRuntimeLoading,
    loadError,
    loading,
    providers,
    refreshInProgress,
    settings,
    workspaces,
    temporaryCliPreferences,
  } = storeToRefs(providerStore);

  const { startWindowDrag } = useWindowDrag();

  const settingsController = useSettingsController({
    providers,
    settings,
    initialSettings: providerStore.settings,
    saveSettings: (value) => providerStore.saveSettings(value),
    probeCliEnvironment: (terminalKind, terminalCommand) =>
      providerStore.probeCliEnvironment(terminalKind, terminalCommand),
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

  const apiKeyManager = useApiKeyManager({
    listKeys: (providerId) => providerStore.listApiKeys(providerId),
    createKey: (providerId, name) => providerStore.createApiKey(providerId, name),
    deleteKey: (providerId, tokenId) => providerStore.deleteApiKey(providerId, tokenId),
    saveProvider: (input) => providerStore.saveProvider(input),
    getProvider: (providerId) => providers.value.find((provider) => provider.identity.id === providerId),
  });

  const availableModels = useAvailableModels({
    providers,
    syncModels: (providerId) => providerStore.syncCodexModels(providerId),
  });

  const cliRuntimeController = useCliRuntime({
    providers,
    cliRuntime,
    refreshInstances: () => providerStore.refreshTemporaryCliInstances(),
    activate: (instanceId) => providerStore.activateTemporaryCli(instanceId),
    previewConfig: (providerId, cliKind) => providerStore.previewCliConfig(providerId, cliKind),
    switchConfig: (providerId, cliKind, revision) =>
      providerStore.switchCliConfig(providerId, cliKind, revision),
  });

  async function removeProvider(provider: Provider) {
    await providerStore.removeProvider(provider.identity.id);
  }

  async function toggleProvider(provider: Provider, enabled: boolean) {
    await providerStore.toggleProvider(provider.identity.id, enabled);
  }

  const providerEditor = useProviderEditor({ store: providerStore });

  const onboarding = useOnboardingController({
    initialized,
    loadError,
    providers,
    settings,
    settingsForm: settingsController.settingsForm,
    saveSettings: (value) => providerStore.saveSettings(value),
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
    listApiKeys: (providerId) => providerStore.listApiKeys(providerId),
    browse: (path) => providerStore.browseWorkspaceDirectories(path),
    forget: (path) => providerStore.forgetWorkspace(path),
    launch: (input) => providerStore.launchTemporaryCli(input),
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
    openApiKeyManager: apiKeyManager.openApiKeyManager,
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
    autoProbeCliEnvironment: settingsController.autoProbeCliEnvironment,
    reloadProviders: () => providerStore.reload().catch(() => {}),
    applyTheme: settingsController.applyTheme,
    resetSettingsDraft: settingsController.resetDraftOnClose,
    resetProviderPointerDrag: workspace.resetProviderPointerDrag,
    refreshUsageSummary: usage.refreshUsageSummary,
    loadCheckInRecords: checkInRecords.loadCheckInRecords,
  });

  // 全量刷新失败此前只把卡片染红、无任何提示，用户无从得知失败原因。
  async function refreshAllProviders() {
    const [error] = await Promise.all([
      providerStore.refreshAll(),
      providerStore.refreshCliRuntime().catch(() => null),
    ]);
    if (error) {
      Message.error(`刷新失败：${error}`);
    }
  }

  return reactive({
    initialized,
    loadError,
    loading,
    providers,
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
    ...checkInRecords,
    ...usage,
    ...requestLogs,
    ...passwordChange,
    ...apiKeyManager,
    ...availableModels,
    ...cliRuntimeController,
    ...workspacePicker,
    ...providerEditor,
    ...providerActions,
    ...workspace,
    refreshAllProviders,
  });
}
