<script setup lang="ts">
import { defineAsyncComponent } from "vue";
import zhCN from "@arco-design/web-vue/es/locale/lang/zh-cn";
import AppDrawers from "./components/AppDrawers.vue";
import AppWorkspace from "./components/AppWorkspace.vue";
import { useAppController } from "./composables/useAppController";
import { useFirstOpenLatch } from "./composables/useFirstOpenLatch";
import { useWindowGridSnap } from "./composables/useWindowGridSnap";

const app = useAppController();
useWindowGridSnap();

// 面板和弹窗只在第一次打开时加载对应模块，之后保持挂载，关闭再打开不丢
// 草稿和分区位置；首屏只保留中转站看板真正需要的东西。
const configurationEditorOpened = useFirstOpenLatch(() => app.cliConfigurationEditor.visible);
const launchPreviewOpened = useFirstOpenLatch(() => app.workspaceLaunchPreviewVisible);
const providerDetailOpened = useFirstOpenLatch(() => app.providerDetailVisible);
const usageReconciliationOpened = useFirstOpenLatch(() => app.usageReconciliationVisible);
const exitConfirmOpened = useFirstOpenLatch(() => app.exitConfirmVisible);
const workspacePickerOpened = useFirstOpenLatch(() => app.workspacePickerVisible);
const sessionDetailOpened = useFirstOpenLatch(() => app.workspaceSessionDetailVisible);
const cliConfigKeyPickerOpened = useFirstOpenLatch(() => app.cliConfigKeyPickerVisible);
const browserRuntimeOpened = useFirstOpenLatch(() => app.browserRuntimeVisible);
const loginAccountsOpened = useFirstOpenLatch(
  () => app.loginAccountsVisible || app.loginAccountsChoosing,
);
const providerCredentialsOpened = useFirstOpenLatch(() => app.providerCredentialsVisible);
const cloudSyncReviewOpened = useFirstOpenLatch(() => app.cloudSyncReviewVisible);
const overlaysOpened = useFirstOpenLatch(() => Boolean(
  app.availableModelsVisible
  || app.usageVisible
  || app.requestLogsVisible
  || app.passwordChangeVisible
  || app.accountProfileVisible
  || app.livenessDetailsVisible
  || app.cliInstancesVisible
  || app.checkInRecordsVisible
  || app.capabilityProbeVisible
  || app.batchOperationVisible
  || app.checkInBatchVisible
  || app.siteAnnouncementsVisible
  || app.onboardingVisible
  || app.updateDialogVisible
));
const AppOverlays = defineAsyncComponent(() => import("./components/AppOverlays.vue"));
const ProviderDetailModal = defineAsyncComponent(() => import("./components/ProviderDetailModal.vue"));
const UsageReconciliationModal = defineAsyncComponent(() => import("./components/agent-workspace/UsageReconciliationModal.vue"));
const AppExitConfirmModal = defineAsyncComponent(() => import("./components/AppExitConfirmModal.vue"));
const WorkspacePickerModal = defineAsyncComponent(() => import("./components/WorkspacePickerModal.vue"));
const CliSessionDetailModal = defineAsyncComponent(() => import("./components/CliSessionDetailModal.vue"));
const CliConfigKeyPickerModal = defineAsyncComponent(() => import("./components/CliConfigKeyPickerModal.vue"));
const BrowserRuntimeModal = defineAsyncComponent(() => import("./components/BrowserRuntimeModal.vue"));
const CloudSyncReviewModal = defineAsyncComponent(() => import("./components/CloudSyncReviewModal.vue"));
const LoginAccountsModal = defineAsyncComponent(() => import("./components/LoginAccountsModal.vue"));
const LoginAccountPicker = defineAsyncComponent(() => import("./components/LoginAccountPicker.vue"));
const ProviderCredentialsModal = defineAsyncComponent(() => import("./components/ProviderCredentialsModal.vue"));
const AgentConfigurationEditorModal = defineAsyncComponent(
  () => import("./components/agent-workspace/AgentConfigurationEditorModal.vue"),
);
const TemporaryCliLaunchPreviewModal = defineAsyncComponent(
  () => import("./components/TemporaryCliLaunchPreviewModal.vue"),
);
</script>

<template>
  <a-config-provider :locale="zhCN">
    <!-- 桌面界面不显示 WebView 原生右键菜单，卡片操作统一在卡片内完成。 -->
    <div class="app-shell" @contextmenu.prevent>
      <BrowserRuntimeModal v-if="browserRuntimeOpened" />
      <CloudSyncReviewModal v-if="cloudSyncReviewOpened" />
      <LoginAccountsModal v-if="loginAccountsOpened" />
      <LoginAccountPicker v-if="loginAccountsOpened" />
      <ProviderCredentialsModal v-if="providerCredentialsOpened" />
      <ProviderDetailModal
        v-if="providerDetailOpened"
        v-model:visible="app.providerDetailVisible"
        :provider="app.providerDetailProvider"
        :settings="app.settings"
        :checking-in="Boolean(app.providerDetailProvider && app.checkingInProviderIds.includes(app.providerDetailProvider.identity.id))"
        @edit="app.editProvider"
        @refresh="app.refreshProvider"
        @check-in="app.checkInProvider"
        @probe-capabilities="app.probeProviderCapabilities"
        @open-available-models="app.openAvailableModels"
        @open-usage="app.openUsage"
        @open-request-logs="app.openRequestLogs"
        @open-check-in-records="app.openCheckInRecords"
        @open-liveness-details="app.openLivenessDetails"
        @open-api-key-manager="app.openApiKeyManager"
        @switch-cli-config="app.switchProviderCliConfig"
        @launch-temporary-cli="app.launchTemporaryCli"
      />
      <UsageReconciliationModal
        v-if="usageReconciliationOpened"
        :visible="app.usageReconciliationVisible"
        @close="app.closeUsageReconciliation"
      />
      <AppExitConfirmModal
        v-if="exitConfirmOpened"
        v-model:visible="app.exitConfirmVisible"
        :tasks="app.exitConfirmTasks"
        :settings-pending="app.exitConfirmSettingsPending"
        @confirm="app.confirmExit"
        @cancel="app.cancelExit"
      />
      <AppWorkspace
        :loading="app.loading"
        :initialized="app.initialized"
        :load-error="app.loadError"
        :storage-blocked-error="app.storageBlockedError"
        :restoring-backup="app.restoringBackup"
        :providers="app.providers"
        :settings="app.settings"
        :provider-sections="app.providerSections"
        :cli-runtime="app.cliRuntime"
        :agent-runtime-snapshot="app.agentRuntimeSnapshot"
        :runtime-loading="app.cliInstancesRefreshing"
        :activating-runtime-id="app.activatingCliInstanceId"
        :announcements-loaded="app.siteAnnouncementsLoaded"
        :announcements-available="app.siteAnnouncementSourcesAvailable"
        :announcements-loading="app.siteAnnouncementsLoading"
        :announcement-total-count="app.siteAnnouncements.length"
        :announcement-unread-count="app.unreadSiteAnnouncementCount"
        :announcement-error-count="app.siteAnnouncementErrors.length + (app.siteAnnouncementsFatalError ? 1 : 0)"
        :background-tasks="app.activeBackgroundTasks"
        :recent-background-tasks="app.recentBackgroundTasks"
        :background-task-count="app.backgroundTaskCount"
        :background-task-attention-count="app.attentionTaskCount"
        :background-task-ambient-count="app.ambientTaskCount"
        :switching-cli-config="app.switchingCliConfig"
        :refresh-in-progress="app.refreshInProgress"
        :refresh-queued-provider-ids="app.refreshQueuedProviderIds"
        :global-check-in-in-progress="app.globalCheckInInProgress"
        :app-version="app.appVersion"
        :checking-for-update="app.checkingForUpdate"
        :checking-in-provider-ids="app.checkingInProviderIds"
        :probing-capabilities-provider-id="app.probingCapabilitiesProviderId"
        :provider-drag="app.providerDrag"
        :drag-over-provider-id="app.dragOverProviderId"
        :dragged-provider="app.draggedProvider"
        :drag-style="app.providerDragStyle"
        :provider-card-tone="app.providerCardTone"
        :card-status-tooltip="app.cardStatusTooltip"
        :show-liveness-timeline="app.showProviderLivenessTimeline"
        @start-drag="app.startWindowDrag"
        @add="app.openAddProvider"
        @import-data="app.importAppData"
        @restore-backup="app.restoreLatestBackup"
        @retry-load="app.retryLoadAppData"
        @restart-app="app.restartApp"
        @check-for-update="app.checkForUpdate"
        @open-github="app.openProjectRepository"
        @open-usage-summary="app.openUsageSummary"
        @refresh-all="app.refreshAllProviders"
        @check-in-all="app.checkInAllProviders"
        @settings="app.settingsDrawerVisible = true"
        @card-click="app.handleProviderCardClick"
        @card-pointerdown="app.handleProviderPointerDown"
        @toggle="app.toggleProviderAction"
        @refresh="app.refreshProvider"
        @probe-capabilities="app.probeProviderCapabilities"
        @launch-temporary-cli="app.launchTemporaryCli"
        @cpa-renew-all="app.renewAllCpaAccounts"
        @reauthorize-card="app.reauthorizeCard"
        @edit="app.editProvider"
        @check-in="app.checkInProvider"
        @open-api-key-manager="app.openApiKeyManager"
        @select-api-key="app.selectManagedApiKeyForCard"
        @open-available-models="app.openAvailableModels"
        @open-usage="app.openUsage"
        @open-request-logs="app.openRequestLogs"
        @open-password-change="app.openPasswordChange"
        @open-account-profile="app.openAccountProfile"
        @open-liveness-details="app.openLivenessDetails"
        @open-check-in-records="app.openCheckInRecords"
        @add-provider-to-app="app.addProviderToApp"
        @copy-url="app.copyProviderUrlAction"
        @copy-invite="app.copyInviteLinkAction"
        @copy-secret="app.copyProviderSecretAction"
        @remove="app.removeProviderAction"
        @open-cli-instances="app.openCliInstances"
        @open-agent-cli-instances="app.openAgentCliInstances"
        @refresh-runtime="app.refreshCliRuntime"
        @activate-runtime="app.activateCliInstance"
        @open-site-announcements="app.openSiteAnnouncements"
        @clear-background-tasks="app.clearRecentBackgroundTasks"
        @switch-cli-config="app.switchProviderCliConfig"
      />

    <WorkspacePickerModal
      v-if="workspacePickerOpened"
      v-model:visible="app.workspacePickerVisible"
      v-model:path-draft="app.workspacePathDraft"
      v-model:cli-kind="app.workspacePickerCliKind"
      v-model:api-key-local-id="app.workspaceApiKeyLocalId"
      v-model:selected-model="app.workspaceSelectedModel"
      v-model:session-name="app.workspaceSessionName"
      v-model:session-mode="app.workspaceSessionMode"
      v-model:run-settings="app.workspaceRunSettings"
      :selected-resume-id="app.workspaceSelectedResumeId"
      v-model:history-query="app.workspaceSessionQuery"
      v-model:history-role-filter="app.workspaceSessionRoleFilter"
      :can-name-session="app.workspaceCanNameSession"
      v-model:terminal-kind="app.workspaceTerminalKind"
      :provider="app.workspacePickerProvider"
      :cli-options="app.workspaceCliOptions"
      :api-keys="app.workspaceApiKeys"
      :terminal-options="app.workspaceTerminalOptions"
      :api-key-loading="app.workspaceApiKeyLoading"
      :api-key-error="app.workspaceApiKeyError"
      :workspaces="app.workspaces"
      :directory="app.workspaceDirectory"
      :browsing="app.workspaceBrowsing"
      :launch-preview-visible="app.workspaceLaunchPreviewVisible"
      :launch-preview-loading="app.workspaceLaunchPreviewLoading"
      :forgetting-path="app.workspaceForgettingPath"
      :error="app.workspaceBrowserError"
      :history-results="app.workspaceSessionResults"
      :history-total="app.workspaceSessionTotal"
      :history-has-more="app.workspaceSessionHasMore"
      :history-loading-more="app.workspaceSessionsLoadingMore"
      :selected-session-ref="app.workspaceSelectedSessionRef"
      :history-loading="app.workspaceSessionsLoading"
      :history-error="app.workspaceSessionsError"
      :history-status-message="app.workspaceSessionStatusMessage"
      :selected-session-title="app.workspaceSelectedSessionTitle"
      :selected-session-workdir="app.workspaceSelectedSessionWorkdir"
      @browse="app.browseWorkspaceDirectory"
      @launch="app.launchWorkspace"
      @forget="app.forgetWorkspace"
      @select-session="app.selectWorkspaceSession"
      @view-session="app.openWorkspaceSessionDetail"
      @view-parent-session="app.openWorkspaceSessionParent"
      @load-more-sessions="app.loadMoreWorkspaceSessions"
      @refresh-sessions="app.refreshWorkspaceSessions"
    />

    <CliSessionDetailModal
      v-if="sessionDetailOpened"
      v-model:visible="app.workspaceSessionDetailVisible"
      :loading="app.workspaceSessionDetailLoading"
      :error="app.workspaceSessionDetailError"
      :detail="app.workspaceSessionDetail"
      :session-row="app.workspaceSessionDetailRow"
      :query="app.workspaceSessionDetailQuery"
      :paused="app.workspaceSessionDetailPaused"
      :message-states="app.workspaceSessionMessageStates"
      :session-search="app.workspaceSessionSearch"
      @navigate="app.navigateWorkspaceSessionDetail"
      @expand="app.expandWorkspaceSessionMessage"
      @pause="app.pauseWorkspaceSessionDetail"
      @retry="app.refreshWorkspaceSessionDetail"
      @parent="app.openWorkspaceSessionParent"
      @search-all="app.searchWorkspaceSession()"
      @continue-search="app.searchWorkspaceSession(true)"
      @stop-search="app.stopWorkspaceSessionSearch()"
      @clear-search="app.clearWorkspaceSessionSearch()"
    />

    <TemporaryCliLaunchPreviewModal
      v-if="launchPreviewOpened"
      v-model:visible="app.workspaceLaunchPreviewVisible"
      :preview="app.workspaceLaunchPreview"
      @confirm="app.confirmWorkspaceLaunch"
    />

    <AgentConfigurationEditorModal
      v-if="configurationEditorOpened"
      :model="app.cliConfigurationEditor"
    />

    <CliConfigKeyPickerModal
      v-if="cliConfigKeyPickerOpened"
      v-model:visible="app.cliConfigKeyPickerVisible"
      :provider="app.cliConfigKeyPickerProvider"
      :cli-kind="app.cliConfigKeyPickerKind"
      :keys="app.cliConfigKeyPickerKeys"
      :current-config="app.cliConfigKeyPickerCurrentConfig"
      :model="app.cliConfigKeyPickerModel"
      @select="app.selectCliConfigApiKey"
    />

    <AppOverlays
      v-if="overlaysOpened"
      v-model:available-models-visible="app.availableModelsVisible"
      v-model:usage-visible="app.usageVisible"
      v-model:usage-period="app.usagePeriod"
      v-model:request-logs-visible="app.requestLogsVisible"
      v-model:password-change-visible="app.passwordChangeVisible"
      v-model:account-profile-visible="app.accountProfileVisible"
      v-model:liveness-details-visible="app.livenessDetailsVisible"
      v-model:cli-instances-visible="app.cliInstancesVisible"
      v-model:check-in-records-visible="app.checkInRecordsVisible"
      v-model:check-in-records-month="app.checkInRecordsMonth"
      v-model:capability-probe-visible="app.capabilityProbeVisible"
      v-model:batch-operation-visible="app.batchOperationVisible"
      v-model:check-in-batch-visible="app.checkInBatchVisible"
      v-model:site-announcements-visible="app.siteAnnouncementsVisible"
      :onboarding-visible="app.onboardingVisible"
      :onboarding-provider-count="app.onboardingProviderCount"
      :onboarding-cli-configured="app.onboardingCliConfigured"
      :onboarding-cli-detecting="app.detectingOnboardingCli"
      :importing-app-data="app.importingAppData"
      :providers="app.providers"
      :available-models-provider="app.availableModelsProvider"
      :available-models-loading="app.availableModelsLoading"
      :available-models-error="app.availableModelsError"
      @switch-available-model="app.switchProviderCliConfig"
      @launch-available-model="(provider, cliKind, model) => app.launchTemporaryCli(provider, cliKind, { model })"
      @open-usage-reconciliation="app.openUsageReconciliation"
      :usage-provider="app.usageProvider"
      :usage-overview="app.usageOverview"
      :usage-loading="app.usageLoading"
      :usage-error="app.usageError"
      :usage-summary="app.usageSummary"
      :request-logs-provider="app.requestLogsProvider"
      :request-logs-loading="app.requestLogsLoading"
      :request-logs-error="app.requestLogsError"
      :request-logs-result="app.requestLogsResult"
      :request-logs-filters="app.requestLogsFilters"
      :request-logs-page="app.requestLogsPage"
      :request-logs-page-size="app.requestLogsPageSize"
      :password-change-provider="app.passwordChangeProvider"
      :password-change-loading="app.passwordChangeLoading"
      :password-change-error="app.passwordChangeError"
      :account-profile-provider="app.accountProfileProvider"
      :account-profile-loading="app.accountProfileLoading"
      :account-profile-error="app.accountProfileError"
      :redeem-loading="app.redeemLoading"
      :redeem-error="app.redeemError"
      :liveness-details-provider="app.livenessDetailsProvider"
      :cli-runtime-loading="app.cliInstancesRefreshing"
      :cli-runtime="app.cliRuntime"
      :cli-instances-provider="app.cliInstancesProvider"
      :cli-instances-kind="app.cliInstancesKind"
      :cli-instances="app.cliInstances"
      :activating-cli-instance-id="app.activatingCliInstanceId"
      :check-in-records-provider="app.checkInRecordsProvider"
      :check-in-records-loading="app.checkInRecordsLoading"
      :check-in-records-result="app.checkInRecordsResult"
      :check-in-records-error="app.checkInRecordsError"
      :provider-protocols="app.providerProtocols"
      :capability-probe-provider="app.capabilityProbeProvider"
      :capability-probe-running="app.capabilityProbeRunning"
      :capability-probe-error="app.capabilityProbeError"
      :capability-probe-result-message="app.capabilityProbeResultMessage"
      :capability-probe-started-at="app.capabilityProbeStartedAt"
      :capability-probe-finished-at="app.capabilityProbeFinishedAt"
      :update-dialog-visible="app.updateDialogVisible"
      :available-update-current-version="app.availableUpdateCurrentVersion"
      :available-update-version="app.availableUpdateVersion"
      :available-update-release-notes="app.availableUpdateReleaseNotes"
      :installing-update="app.installingUpdate"
      :update-can-cancel="app.updateCanCancel"
      :cancelling-update="app.cancellingUpdate"
      :update-download-progress="app.updateDownloadProgress"
      :update-install-status="app.updateInstallStatus"
      :batch-operation="app.batchOperation"
      :batch-operation-running="app.batchOperationRunning"
      :batch-operation-items="app.batchOperationItems"
      :batch-operation-waiting-reason="app.batchOperationWaitingReason"
      :batch-operation-error="app.batchOperationError"
      :batch-operation-started-at="app.batchOperationStartedAt"
      :batch-operation-finished-at="app.batchOperationFinishedAt"
      :batch-operation-completed="app.batchOperationCompleted"
      :batch-operation-canceling="app.batchOperationCanceling"
      :batch-operation-cancelled="app.batchOperationCancelled"
      @cancel-batch-refresh="app.batchOperationCancel"
      :check-in-batch-progress="app.checkInBatchProgress"
      :check-in-pending="app.checkInPending"
      :site-announcements-loading="app.siteAnnouncementsLoading"
      :site-announcements-fatal-error="app.siteAnnouncementsFatalError"
      :site-announcements="app.siteAnnouncements"
      :site-announcement-errors="app.siteAnnouncementErrors"
      :site-announcement-truncated-sources="app.siteAnnouncementTruncatedSources"
      :unread-site-announcement-count="app.unreadSiteAnnouncementCount"
      :marking-announcement-fingerprints="app.markingAnnouncementFingerprints"
      :site-announcement-is-read="app.announcementIsRead"
      @open-onboarding-add-provider="app.openOnboardingAddProvider"
      @import-onboarding-data="app.importOnboardingData"
      @probe-onboarding-cli="app.detectOnboardingCliTools"
      @open-onboarding-agent-workspace="app.openOnboardingAgentWorkspace"
      @dismiss-onboarding="app.dismissOnboarding"
      @complete-onboarding="app.completeOnboarding"
      @refresh-available-models="app.refreshAvailableModels"
      @copy-available-model="app.copyAvailableModel"
      @copy-available-models="app.copyAvailableModels"
      @refresh-usage-summary="() => app.refreshUsageSummary({ force: true })"
      @search-request-logs="app.searchRequestLogs"
      @reset-request-log-filters="app.resetRequestLogFilters"
      @load-request-logs="() => app.loadRequestLogs({ force: true })"
      @set-request-logs-page="app.setRequestLogsPage"
      @set-request-logs-page-size="app.setRequestLogsPageSize"
      @submit-password-change="app.submitPasswordChange"
      @submit-account-profile="app.submitProfile"
      @submit-redeem-code="app.submitRedeem"
      @refresh-cli-runtime="app.refreshCliRuntime"
      @activate-cli-instance="app.activateCliInstance"
      @load-check-in-records="app.loadCheckInRecords"
      @resume-check-in-task="app.resumeCheckInTask"
      @cancel-check-in-task="app.cancelCheckInTask"
      @show-check-in-window="app.showCheckInWindow"
      @retry-capability-probe="app.retryCapabilityProbe"
      @dismiss-update="app.dismissUpdate"
      @cancel-update="app.cancelUpdate"
      @install-update="app.installUpdate"
      @refresh-site-announcements="app.refreshSiteAnnouncements"
      @mark-site-announcement-read="app.markSiteAnnouncementRead"
      @mark-all-site-announcements-read="app.markAllSiteAnnouncementsRead"
    />

    <AppDrawers
      v-model:settings-visible="app.settingsDrawerVisible"
      v-model:provider-editor-visible="app.drawerVisible"
      v-model:api-key-add-visible="app.apiKeyAddVisible"
      v-model:api-key-add-remark="app.apiKeyAddRemark"
      v-model:api-key-add-value="app.apiKeyAddValue"
      v-model:api-key-remark-visible="app.apiKeyRemarkVisible"
      v-model:api-key-remark-value="app.apiKeyRemarkValue"
      :settings="app.settingsForm"
      :settings-save-state="app.settingsSaveState"
      :settings-save-error="app.settingsSaveError"
      :testing-notification="app.testingNotification"
      :notification-test-result="app.notificationTestResult"
      :notification-test-error="app.notificationTestError"
      @retry-settings-save="app.flushSettingsSave"
      :liveness-model-options="app.livenessModelOptions"
      :selected-liveness-model-providers="app.selectedLivenessModelProviders"
      :exporting-app-data="app.exportingAppData"
      :installing-dev-sample="app.installingDevSample"
      :importing-app-data="app.importingAppData"
      :app-version="app.appVersion"
      :checking-for-update="app.checkingForUpdate"
      :provider-editor-title="app.drawerTitle"
      :provider-editor-session="app.editorSession"
      :provider-editor-draft-revision="app.editorDraftRevision"
      :provider-editor-draft-before-sync="app.editorDraftBeforeSync"
      :provider-editor-initial-section="app.editorInitialSection"
      :draft-provider="app.draftProvider"
      :provider-protocols="app.providerProtocols"
      :api-key-options="app.apiKeyOptions"
      :api-key-remote-managed="app.editorApiKeyRemoteManaged"
      :api-key-manager-provider="app.apiKeyManagerProvider"
      :api-key-manager-operation="app.apiKeyManagerOperation"
      :api-key-remark-target="app.apiKeyRemarkTarget"
      :available-models="app.availableModels"
      :site-probe-result="app.siteProbeResult"
      :protocol-detection-result="app.protocolDetectionResult"
      :protocol-selection-source="app.protocolSelectionSource"
      :probing-site="app.probingSite"
      :site-name-source-base-url="app.siteNameSourceBaseUrl"
      :testing-connection="app.testingConnection"
      :connection-test-result="app.connectionTestResult"
      :saving-provider="app.savingProvider"
      :provider-save-error="app.providerSaveError"
      :starting-browser-login="app.startingBrowserLogin"
      @login-and-import="app.loginAndImport"
      :credential-assistant-state="app.credentialAssistantState"
      :credential-assistant-steps="app.credentialAssistantSteps"
      :credential-assistant-message="app.credentialAssistantMessage"
      :credential-assistant-busy="app.credentialAssistantBusy"
      :can-run-credential-assistant="app.canRunCredentialAssistant"
      :credential-assistant-saved="app.credentialAssistantSaved"
      @test-notification="app.sendTestNotification"
      @export-app-data="app.exportAppData"
      @install-dev-sample-data="app.installDevSampleData"
      @import-app-data="app.importAppData"
      @check-for-update="app.checkForUpdate"
      @copy-api-key="app.copyDraftApiKey"
      @sync-remote-api-keys="app.syncRemoteApiKeys"
      @open-api-key-create-editor="app.openApiKeyCreateEditor"
      @open-api-key-settings-editor="app.openApiKeySettingsEditor"
      @open-api-key-add-panel="app.openApiKeyAddPanel"
      @open-api-key-remark-editor="app.openApiKeyRemarkEditor"
      @add-local-api-key="app.addLocalApiKey"
      @save-managed-api-key-remark="app.saveManagedApiKeyRemark"
      @set-default-managed-api-key="app.selectManagedApiKey"
      @copy-managed-api-key="app.copyManagedApiKey"
      @delete-managed-api-key="app.deleteManagedApiKey"
      @run-credential-assistant="app.runCredentialAssistant"
      @test-connection="app.testConnection"
      @probe-site="app.probeSite"
      @select-protocol="app.selectProtocol"
      @save-provider="app.saveProvider"
    />
  </div>
  </a-config-provider>
</template>
