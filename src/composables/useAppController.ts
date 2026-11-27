import { useOAuthTasks } from "./useOAuthTasks";
import { computed, onMounted, reactive, watch } from "vue";
import { storeToRefs } from "pinia";
import { Message, Modal } from "@arco-design/web-vue";
import { relaunch } from "@tauri-apps/plugin-process";
import { useProviderStore, type Provider } from "../stores/providers";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import { useAgentConfigurationStore } from "../stores/agent-configuration";
import { useSettingsStore } from "../stores/settings";
import { useWorkspaceStore } from "../stores/workspaces";
import { useAgentWorkspaceStore } from "../stores/agent-workspace";
import { useApiKeyManager } from "./useApiKeyManager";
import { useAppDataTransfer } from "./useAppDataTransfer";
import { useAppLifecycle } from "./useAppLifecycle";
import { useAppExitGuard } from "./useAppExitGuard";
import { useAppUpdater } from "./useAppUpdater";
import { useAppVersion } from "./useAppVersion";
import { useAvailableModels } from "./useAvailableModels";
import { useBatchOperation } from "./useBatchOperation";
import { useBackgroundTaskCenter } from "./useBackgroundTaskCenter";
import { useBrowserRuntime } from "./useBrowserRuntime";
import { useCloudSync } from "./useCloudSync";
import { useAgentCatalogStore } from "../stores/agent-catalog";
import { useLoginAccounts } from "./useLoginAccounts";
import { useProviderCredentials } from "./useProviderCredentials";
import { useAgentBackgroundTasks } from "./useAgentBackgroundTasks";
import { useAgentLifecycleEvents } from "./useAgentLifecycleEvents";
import { useCheckInActions } from "./useCheckInActions";
import { useCheckInRecords } from "./useCheckInRecords";
import { useCliRuntime } from "./useCliRuntime";
import { useAccountProfile } from "./useAccountProfile";
import { usePasswordChange } from "./usePasswordChange";
import { useOnboardingController } from "./useOnboardingController";
import { useProviderEditor } from "./useProviderEditor";
import { useProviderDetail } from "./useProviderDetail";
import { supportsCommand } from "./useHostCapabilities";
import { useProviderActions } from "./useProviderActions";
import { useProviderWorkspaceController } from "./useProviderWorkspaceController";
import { useRequestLogs } from "./useRequestLogs";
import { useSettingsController } from "./useSettingsController";
import { useSiteAnnouncements } from "./useSiteAnnouncements";
import { useSystemNotification } from "./useSystemNotification";
import { useUsageSummary } from "./useUsageSummary";
import { useUsageReconciliation } from "./useUsageReconciliation";
import { useWindowDrag } from "./useWindowDrag";
import { useWorkspacePicker } from "./useWorkspacePicker";
import {
  getUsageOverview as getUsageOverviewCommand,
  openProjectRepository as openProjectRepositoryCommand,
} from "../api/app";
import { cancelProviderBrowserLogin, startProviderBrowserLogin } from "../api/provider-browser-login";
import { withTimeout } from "../utils/promise-timeout";
import { providerToInput } from "../utils/provider-input";
import { agentCliLabel } from "../utils/cli-environment";
import { runWhenIdle } from "../utils/idle";

export function useAppController() {
  useAgentLifecycleEvents();
  const providerStore = useProviderStore();
  const settingsStore = useSettingsStore();
  const workspaceStore = useWorkspaceStore();
  const agentWorkspaceStore = useAgentWorkspaceStore();
  const cliRuntimeStore = useCliRuntimeStore();
  const configurationTasks = useAgentConfigurationStore();
  onMounted(() => {
    // 恢复上次未完成的 Agent 配置任务会扫描历史和任务目录；先让首屏可交互，
    // 再在空闲时补上，不阻塞中转站看板。
    runWhenIdle(() => {
      void configurationTasks.recover();
    });
  });
  const {
    initialized,
    loadError,
    storageBlockedError,
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
    agentRuntimeSnapshot,
    cliEnvironmentProbe,
    terminalEnvironmentProbe,
  } = storeToRefs(cliRuntimeStore);

  const { startWindowDrag } = useWindowDrag();

  const settingsController = useSettingsController({
    providers,
    settings,
    initialSettings: settingsStore.settings,
    saveSettings: (value, expected) => settingsStore.save(value, expected),
  });

  const systemNotification = useSystemNotification(settingsController.settingsForm);
  const { appVersion } = useAppVersion();
  const appUpdater = useAppUpdater({ settings });

  const appDataTransfer = useAppDataTransfer({
    exportAppData: (path) => providerStore.exportAppData(path),
    importAppData: (path) => settingsController.replaceSettings(() => providerStore.importAppData(path)),
    restoreLatestBackup: () => settingsController.replaceSettings(() => providerStore.restoreLatestBackup()),
    installDevSampleData: async () => {
      await providerStore.installDevSampleData();
      settingsController.syncFromSettings();
    },
    beforeTransfer: settingsController.flushSettingsSave,
  });

  const browserRuntime = useBrowserRuntime();
  const cloudSync = useCloudSync(async () => {
    await providerStore.reload();
    settingsController.syncFromSettings();
    const catalog = useAgentCatalogStore();
    for (const key of Object.keys(catalog.catalogs)) catalog.invalidate(key === "__native__" ? undefined : key);
  });
  const loginAccounts = useLoginAccounts(browserRuntime);
  const checkInRecords = useCheckInRecords({
    providers,
    loadRecords: (providerId, month) => providerStore.getCheckInRecords(providerId, month),
  });
  const checkIn = useCheckInActions({
    reload: async (providerIds) => {
      // 签到成功会改变当月的记录；缓存不失效的话，再次打开日历读到的还是旧值。
      checkInRecords.invalidateCheckInRecords(providerIds);
      // 完成的站点各自只补自己的卡片视图；签到不改设置和工作区。
      await providerStore.reloadProviderViews(providerIds);
    },
    browserRuntime,
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
  });

  const usage = useUsageSummary({
    providers,
    loadUsage: (providerId, period) => providerStore.getUsage(providerId, period),
    loadOverview: (period) => getUsageOverviewCommand(period),
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

  const accountProfile = useAccountProfile({
    providers,
    changeProfile: (providerId, username, displayName) =>
      providerStore.changeProfile(providerId, username, displayName),
    redeemCode: (providerId, code) => providerStore.redeemCode(providerId, code),
  });

  const providerEditor = useProviderEditor({ store: providerStore, browserRuntime, loginAccounts });
  const providerDetail = useProviderDetail(providers);
  const usageReconciliation = useUsageReconciliation();
  /// 凭据详情里的“重新登录 / 授权”直接复用已绑定的登录账号；只有缺少绑定
  /// 或没有浏览器组件时才退回编辑器，避免为一次重新登录先打开整个编辑抽屉。
  async function reloginBrowserProvider(id: string) {
    const provider = providers.value.find((item) => item.identity.id === id);
    if (!provider) {
      Message.error("中转站已不存在");
      return;
    }
    const accountId = provider.auth.browserBinding?.accountId?.trim();
    if (!provider.actions.reauthenticate || !accountId) {
      // 编辑抽屉已经打开同一个站点时，不能重新初始化草稿：用户未保存的
      // 修改会被静默还原，进行中的保存也会被作废。直接复用当前草稿。
      if (
        !providerEditor.drawerVisible.value
        || providerEditor.editingProviderId.value !== id
      ) {
        providerEditor.openEditProvider(provider, "credentials");
      }
      void providerEditor.loginAndImport();
      return;
    }
    if (!browserRuntime.available.value) {
      Message.info("当前宿主没有本机浏览器，请改用账号密码、会话 Cookie 或访问令牌");
      return;
    }
    await browserRuntime.refresh(true);
    if (!browserRuntime.state.value?.ready) {
      const error = browserRuntime.error.value;
      browserRuntime.open();
      Message.info(
        error
          ? `浏览器组件检测失败：${error}`
          : "请先确认安装浏览器组件，完成后再次点击重新登录",
      );
      return;
    }
    try {
      let accepted = false;
      const starting = startProviderBrowserLogin(providerToInput(provider), accountId);
      // 启动 IPC 卡住时任务可能稍后才真正创建；此时用户已经看到超时提示，
      // 不能让一个晚到的登录窗口在后台自己弹出来。
      void starting.then((task) => {
        if (!accepted) void cancelProviderBrowserLogin(task.runId).catch(() => {});
      }).catch(() => {});
      await withTimeout(starting, 10_000, "登录任务启动超时，请稍后重试");
      accepted = true;
      Message.info("登录任务已开始，可在后台任务中查看进度或取消");
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    }
  }
  const providerCredentials = useProviderCredentials((id) => {
    void reloginBrowserProvider(id);
  });
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
    createRemoteKey: (providerId, revision, patch) => providerStore.createApiKey(providerId, revision, patch),
    updateRemoteKey: (providerId, revision, tokenId, patch) => providerStore.updateApiKey(providerId, revision, tokenId, patch),
    getRemoteKeyEditorContext: (providerId, tokenId) => providerStore.apiKeyEditorContext(providerId, tokenId),
    setRemark: (providerId, localId, remark) =>
      providerStore.setLocalApiKeyRemark(providerId, localId, remark),
    setDefaultKey: (providerId, localId) => providerStore.setDefaultLocalApiKey(providerId, localId),
    removeLocalKey: (providerId, localId) => providerStore.removeLocalApiKey(providerId, localId),
    deleteRemoteKey: (providerId, tokenId) => providerStore.deleteApiKey(providerId, tokenId),
    getProvider: (providerId) => providers.value.find((provider) => provider.identity.id === providerId),
    refreshByIds: (ids) => providerStore.refreshByIds(ids),
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
      if (supportsCommand("get_cli_runtime_snapshot")) {
        void cliRuntimeStore.refresh().catch(() => undefined);
      }
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
    agentRuntime: agentRuntimeSnapshot,
    refreshAgentRuntime: () => cliRuntimeStore.refreshAgentRuntimeSnapshot(),
    activateAgentRuntime: (runtimeId) => cliRuntimeStore.activateAgentRuntime(runtimeId),
    previewConfig: (providerId, cliKind, apiKeyLocalId, model) =>
      cliRuntimeStore.previewConfig(providerId, cliKind, apiKeyLocalId, model),
    bindAgentModel: (providerId, cliKind, model) =>
      providerStore.setAgentModel(providerId, cliKind, model),
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
    await apiKeyManager.setDefaultManagedApiKey(option);
  }

  async function selectManagedApiKeyForCard(
    provider: Provider,
    option: Provider["auth"]["apiKeyOptions"][number],
  ) {
    await apiKeyManager.setDefaultKeyForProvider(provider, option);
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
    cliEnvironmentProbe,
    flushSettingsSave: settingsController.flushSettingsSave,
    importAppData: appDataTransfer.importAppData,
    openAddProvider: providerEditor.openAddProvider,
    detectCliTools: () => cliRuntimeStore.probeCliTools(true),
    openAgentWorkspace: () => agentWorkspaceStore.setView("agents"),
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
  });

  const providerActions = useProviderActions({
    providers,
    refreshByIds: async (ids) => {
      if (providerStore.refreshInProgress) {
        // 否则这次点击没有任何反馈：全局刷新完成后所有卡片都会重读。
        Message.info("正在刷新全部中转站，完成后这里会自动更新");
        return;
      }
      const error = await providerStore.refreshByIds(ids);
      if (error) {
        Message.error(`刷新失败：${error}`);
      }
    },
    openWorkspacePicker: workspacePicker.openWorkspacePicker,
    probeCapabilities: (id) => providerStore.probeCapabilities(id),
    getInviteLink: (id) => providerStore.getInviteLink(id),
    reloadProvider: (id) => providerStore.reloadProvider(id),
    openEditProvider: providerEditor.openEditProvider,
    checkInProviderAction: checkIn.checkInProviderAction,
    openApiKeyManager: openProviderApiKeys,
    openAvailableModels: availableModels.openAvailableModels,
    openUsage: usage.openUsage,
    openRequestLogs: requestLogs.openRequestLogs,
    openPasswordChange: passwordChange.openPasswordChange,
    openAccountProfile: accountProfile.openAccountProfile,
    openCheckInRecords: checkInRecords.openCheckInRecords,
    reloginProvider: (provider) => reloginBrowserProvider(provider.identity.id),
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
    openDetail: (provider) => providerDetail.openProviderDetail(provider),
  });

  useAppLifecycle({
    loadError,
    settings,
    settingsForm: settingsController.settingsForm,
    settingsDrawerVisible: settingsController.settingsDrawerVisible,
    initialize: () => providerStore.initialize(),
    syncFromSettings: settingsController.syncFromSettings,
    setupThemeListener: settingsController.setupThemeListener,
    cleanupThemeListener: settingsController.cleanupThemeListener,
    syncLaunchAtLogin: settingsController.syncLaunchAtLogin,
    autoProbeCliTools: async () => {
      // 先用上次探测结果点亮卡片；完整探测等首屏空闲后再跑。
      await cliRuntimeStore.hydrateCachedCliTools();
      runWhenIdle(() => {
        void cliRuntimeStore.probeCliTools(false).catch(() => {});
      });
    },
    reloadProviders: () => providerStore.reloadProviders().catch(() => {}),
    reloadProviderViews: (ids) => providerStore.reloadProviderViews(ids).catch(() => {}),
    onStorageBlocked: (error) => providerStore.markStorageBlocked(error),
    onConfigReloaded: (result) => {
      // 编辑器自动保存会连续触发重载；成功和失败共用固定 id，后一次替换
      // 前一条提示，不在角落里叠一摞消息，也不会让旧错误盖住新结果。
      Message.success({
        id: "config-reload-status",
        content:
          result.providerCount === null
            ? "已从 config.json 重新读取配置"
            : `已从 config.json 重新读取 ${result.providerCount} 个中转站`,
      });
      if (result.settingsChanged) {
        // 配置可能改了开机启动。等 providers-changed 快照水合后，再把系统
        // 登录项改成新配置，而不是用宿主旧状态反向覆盖配置。
        settingsController.requestExternalLaunchSync();
      }
    },
    onConfigReloadFailed: (result) => {
      // 同一份损坏文件只会在后端去重一次；编辑器再改坏一次时替换旧提示，
      // 让最新错误始终可见。
      Message.error({
        id: "config-reload-status",
        content: `config.json 修改未生效：${result.error}`,
      });
    },
    applyTheme: settingsController.applyTheme,
    flushSettingsSave: settingsController.flushSettingsSave,
    resetProviderPointerDrag: workspace.resetProviderPointerDrag,
  });

  /// 运行期存储保护态（云同步事务未完成）必须重启由启动恢复流程处理；
  /// 就地导入会和磁盘上的事务日志冲突。
  function restartApp() {
    Modal.confirm({
      title: "重启应用",
      content:
        "应用会重新启动并重新读取本地配置；尚未完成的网络任务会被中断。",
      okText: "立即重启",
      cancelText: "稍后",
      onOk: async () => {
        try {
          await relaunch();
        } catch (error) {
          Message.error(
            `自动重启失败：${error instanceof Error ? error.message : String(error)}，请手动退出后重新打开`,
          );
        }
      },
    });
  }

  async function refreshAllProviders() {
    // 启动读取未完成或失败时，providers 还是空的；直接走批量刷新会把
    // “配置还没读出来”误报成“没有已启用的中转站”。
    if (!initialized.value) {
      Message.info(
        loadError.value
          ? "应用配置尚未成功读取，请先在提示中重试读取"
          : "正在读取本地配置，请稍后再刷新",
      );
      return;
    }
    await batchOperation.runRefresh();
  }

  async function checkInAllProviders() {
    await checkIn.checkInAllProvidersAction();
  }

  async function openProjectRepository() {
    try {
      await openProjectRepositoryCommand();
    } catch (error) {
      Message.error(`无法打开 GitHub：${error instanceof Error ? error.message : String(error)}`);
    }
  }

  const globalCheckInInProgress = checkIn.globalCheckInInProgress;
  const agentTasks = useAgentBackgroundTasks();
  const oauthTasks = useOAuthTasks();

  const backgroundTaskCenter = useBackgroundTaskCenter({
    domainTasks: computed(() => [...agentTasks.value, ...cloudSync.tasks.value, ...oauthTasks.value]),
    providers,
    openLoginAccount: loginAccounts.open,
    openProviderCredentials: providerCredentials.open,
    batchOperation: batchOperation.operation,
    batchOperationRunning: batchOperation.running,
    batchOperationItems: batchOperation.items,
    batchOperationWaitingReason: batchOperation.waitingReason,
    batchOperationError: batchOperation.error,
    batchOperationCompleted: batchOperation.completed,
    batchOperationCanceling: batchOperation.canceling,
    cancelBatchRefresh: batchOperation.cancelRefresh,
    refreshInProgress,
    refreshingProviderIds: refreshingIds,
    checkInTasks: checkIn.checkInTasks,
    checkInPending: checkIn.checkInPending,
    resumeCheckInTask: checkIn.resumeCheckInTask,
    cancelCheckInTask: checkIn.cancelCheckInTask,
    showCheckInWindow: checkIn.showCheckInWindow,
    browserRuntime: browserRuntime.state,
    cancelBrowserRuntime: browserRuntime.cancel,
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
    storageBlockedError,
    restartApp,
  });
  const appExitGuard = useAppExitGuard({
    activeTasks: backgroundTaskCenter.activeTasks,
    pendingSettings: computed(() => settingsController.settingsSaveState.value !== "saved"),
    flushSettings: settingsController.flushSettingsSave,
  });
  // 只有还在排队的站点才显示「排队中」。批量刷新里已经结束的卡片必须
  // 立刻回到自己的成功/失败状态，不能在整个批次结束前一直被盖住。
  const refreshQueuedProviderIds = computed(
    () =>
      new Set(
        batchOperation.items.value
          .filter((item) => item.status === "pending")
          .map((item) => item.providerId),
      ),
  );

  return reactive({
    initialized,
    loadError,
    storageBlockedError,
    restartApp,
    loading,
    providers,
    settings,
    providerProtocols,
    workspaces,
    temporaryCliPreferences,
    cliRuntime,
    agentRuntimeSnapshot,
    cliRuntimeLoading,
    refreshInProgress,
    refreshQueuedProviderIds,
    startWindowDrag,
    retryLoadAppData: () => providerStore.initialize(),
    ...settingsController,
    ...onboarding,
    ...systemNotification,
    appVersion,
    ...appUpdater,
    ...appDataTransfer,
    ...checkIn,
    batchOperation: batchOperation.operation,
    batchOperationRunning: batchOperation.running,
    batchOperationVisible: batchOperation.visible,
    batchOperationItems: batchOperation.items,
    batchOperationWaitingReason: batchOperation.waitingReason,
    batchOperationError: batchOperation.error,
    batchOperationStartedAt: batchOperation.startedAt,
    batchOperationFinishedAt: batchOperation.finishedAt,
    batchOperationCompleted: batchOperation.completed,
    batchOperationCanceling: batchOperation.canceling,
    batchOperationCancelled: batchOperation.cancelled,
    batchOperationCancel: batchOperation.cancelRefresh,
    globalCheckInInProgress,
    activeBackgroundTasks: backgroundTaskCenter.activeTasks,
    recentBackgroundTasks: backgroundTaskCenter.recentTasks,
    backgroundTaskCount: backgroundTaskCenter.activeTaskCount,
    attentionTaskCount: backgroundTaskCenter.attentionTaskCount,
    ambientTaskCount: backgroundTaskCenter.ambientTaskCount,
    clearRecentBackgroundTasks: backgroundTaskCenter.clearRecentTasks,
    ...appExitGuard,
    ...checkInRecords,
    ...usage,
    ...requestLogs,
    ...passwordChange,
    ...accountProfile,
    ...apiKeyManager,
    ...availableModels,
    ...siteAnnouncements,
    ...cliRuntimeController,
    ...workspacePicker,
    ...providerEditor,
    ...providerDetail,
    ...usageReconciliation,
    editorApiKeyRemoteManaged,
    selectManagedApiKey,
    selectManagedApiKeyForCard,
    ...providerActions,
    ...workspace,
    // 这几个弹窗靠注入的控制器状态驱动，自身不接收 visible prop；导出可见性
    // 供 App 层做首次打开闸门，按需加载对应模块。
    browserRuntimeVisible: browserRuntime.visible,
    loginAccountsVisible: loginAccounts.visible,
    loginAccountsChoosing: loginAccounts.choosing,
    providerCredentialsVisible: providerCredentials.visible,
    cloudSyncReviewVisible: cloudSync.reviewVisible,
    refreshAllProviders,
    checkInAllProviders,
    openProjectRepository,
  });
}
