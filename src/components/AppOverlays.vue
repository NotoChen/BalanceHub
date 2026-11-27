<script setup lang="ts">
import { defineAsyncComponent } from "vue";
import { useFirstOpenLatch } from "../composables/useFirstOpenLatch";
import type {
  AgentCliKind,
  CliRuntimeSnapshot,
  Provider,
  ProviderCheckInRecordsResult,
  ProviderProtocolDescriptor,
  ProviderRequestLogFilters,
  ProviderRequestLogsResult,
  ProviderUsageOverview,
  ProviderUsageSummary,
  AgentRuntimeSession,
  SiteAnnouncement,
  SiteAnnouncementSourceError,
} from "../stores/providers";
import type { ProviderBatchOperation, ProviderBatchProgressItem } from "../api/batch-operation";
import type { CheckInTask } from "../api/checkin";
import type { CheckInBatchProgress } from "../composables/useCheckInBatchProgress";
import type { UsagePeriod } from "../utils/usage-trend";

const props = defineProps<{
  onboardingVisible: boolean;
  onboardingProviderCount: number;
  onboardingCliConfigured: boolean;
  onboardingCliDetecting: boolean;
  importingAppData: boolean;
  availableModelsProvider: Provider | null;
  availableModelsLoading: boolean;
  availableModelsError: string;
  providers: readonly Provider[];
  usageProvider: Provider | null;
  usageLoading: boolean;
  usageError: string;
  usageSummary: ProviderUsageSummary | null;
  usageOverview: ProviderUsageOverview | null;
  requestLogsProvider: Provider | null;
  requestLogsLoading: boolean;
  requestLogsError: string;
  requestLogsResult: ProviderRequestLogsResult | null;
  requestLogsFilters: ProviderRequestLogFilters;
  requestLogsPage: number;
  requestLogsPageSize: number;
  passwordChangeProvider: Provider | null;
  passwordChangeLoading: boolean;
  passwordChangeError: string;
  accountProfileProvider: Provider | null;
  accountProfileLoading: boolean;
  accountProfileError: string;
  redeemLoading: boolean;
  redeemError: string;
  livenessDetailsProvider: Provider | null;
  cliRuntimeLoading: boolean;
  cliRuntime: CliRuntimeSnapshot;
  cliInstancesProvider: Provider | null;
  cliInstancesKind: AgentCliKind | null;
  cliInstances: AgentRuntimeSession[];
  activatingCliInstanceId: string | null;
  checkInRecordsProvider: Provider | null;
  checkInRecordsLoading: boolean;
  checkInRecordsResult: ProviderCheckInRecordsResult | null;
  checkInRecordsError: string;
  providerProtocols: ProviderProtocolDescriptor[];
  capabilityProbeProvider: Provider | null;
  capabilityProbeRunning: boolean;
  capabilityProbeError: string;
  capabilityProbeResultMessage: string;
  capabilityProbeStartedAt: number | null;
  capabilityProbeFinishedAt: number | null;
  updateDialogVisible: boolean;
  availableUpdateCurrentVersion: string;
  availableUpdateVersion: string;
  availableUpdateReleaseNotes: string;
  installingUpdate: boolean;
  updateCanCancel: boolean;
  cancellingUpdate: boolean;
  updateDownloadProgress: number | null;
  updateInstallStatus: string;
  batchOperation: ProviderBatchOperation | null;
  batchOperationRunning: boolean;
  batchOperationItems: ProviderBatchProgressItem[];
  batchOperationWaitingReason: string;
  batchOperationError: string;
  batchOperationStartedAt: number | null;
  batchOperationFinishedAt: number | null;
  batchOperationCompleted: boolean;
  batchOperationCanceling: boolean;
  batchOperationCancelled: boolean;
  checkInBatchProgress: CheckInBatchProgress;
  checkInPending: string[];
  siteAnnouncementsLoading: boolean;
  siteAnnouncementsFatalError: string;
  siteAnnouncements: SiteAnnouncement[];
  siteAnnouncementErrors: SiteAnnouncementSourceError[];
  siteAnnouncementTruncatedSources: string[];
  unreadSiteAnnouncementCount: number;
  markingAnnouncementFingerprints: Set<string>;
  siteAnnouncementIsRead: (item: SiteAnnouncement) => boolean;
}>();

const emit = defineEmits<{
  openOnboardingAddProvider: [];
  importOnboardingData: [];
  probeOnboardingCli: [];
  openOnboardingAgentWorkspace: [];
  dismissOnboarding: [];
  completeOnboarding: [];
  refreshAvailableModels: [];
  copyAvailableModel: [model: string];
  copyAvailableModels: [models: string[]];
  switchAvailableModel: [provider: Provider, cliKind: AgentCliKind, model: string];
  launchAvailableModel: [provider: Provider, cliKind: AgentCliKind, model: string];
  openUsageReconciliation: [];
  cancelBatchRefresh: [];
  refreshUsageSummary: [];
  searchRequestLogs: [filters: ProviderRequestLogFilters];
  resetRequestLogFilters: [];
  loadRequestLogs: [];
  setRequestLogsPage: [page: number];
  setRequestLogsPageSize: [pageSize: number];
  submitPasswordChange: [originalPassword: string, password: string];
  submitAccountProfile: [username: string, displayName: string];
  submitRedeemCode: [code: string];
  refreshCliRuntime: [];
  activateCliInstance: [instance: AgentRuntimeSession];
  loadCheckInRecords: [options?: { force?: boolean }];
  resumeCheckInTask: [task: CheckInTask];
  cancelCheckInTask: [task: CheckInTask];
  showCheckInWindow: [task: CheckInTask];
  retryCapabilityProbe: [];
  dismissUpdate: [];
  cancelUpdate: [];
  installUpdate: [];
  refreshSiteAnnouncements: [];
  markSiteAnnouncementRead: [item: SiteAnnouncement];
  markAllSiteAnnouncementsRead: [];
}>();

const availableModelsVisible = defineModel<boolean>("availableModelsVisible", { required: true });
const usageVisible = defineModel<boolean>("usageVisible", { required: true });
const usagePeriod = defineModel<UsagePeriod>("usagePeriod", { required: true });
const requestLogsVisible = defineModel<boolean>("requestLogsVisible", { required: true });
const passwordChangeVisible = defineModel<boolean>("passwordChangeVisible", { required: true });
const accountProfileVisible = defineModel<boolean>("accountProfileVisible", { required: true });
const livenessDetailsVisible = defineModel<boolean>("livenessDetailsVisible", { required: true });
const cliInstancesVisible = defineModel<boolean>("cliInstancesVisible", { required: true });
const checkInRecordsVisible = defineModel<boolean>("checkInRecordsVisible", { required: true });
const checkInRecordsMonth = defineModel<string>("checkInRecordsMonth", { required: true });
const capabilityProbeVisible = defineModel<boolean>("capabilityProbeVisible", { required: true });
const batchOperationVisible = defineModel<boolean>("batchOperationVisible", { required: true });
const checkInBatchVisible = defineModel<boolean>("checkInBatchVisible", { required: true });
const siteAnnouncementsVisible = defineModel<boolean>("siteAnnouncementsVisible", { required: true });

// 每个弹窗首次打开时再加载对应代码；打开过之后保持挂载，保留窗口内的
// 草稿、筛选和滚动状态，也避免每次开关都重新请求资源。
const AppOnboardingModal = defineAsyncComponent(() => import("./AppOnboardingModal.vue"));
const AppUpdateModal = defineAsyncComponent(() => import("./AppUpdateModal.vue"));
const AvailableModelsModal = defineAsyncComponent(() => import("./AvailableModelsModal.vue"));
const BatchOperationProgressModal = defineAsyncComponent(() => import("./BatchOperationProgressModal.vue"));
const CheckInCalendarModal = defineAsyncComponent(() => import("./CheckInCalendarModal.vue"));
const CapabilityProbeModal = defineAsyncComponent(() => import("./CapabilityProbeModal.vue"));
const LivenessDetailsModal = defineAsyncComponent(() => import("./LivenessDetailsModal.vue"));
const AccountProfileModal = defineAsyncComponent(() => import("./AccountProfileModal.vue"));
const PasswordChangeModal = defineAsyncComponent(() => import("./PasswordChangeModal.vue"));
const RequestLogsModal = defineAsyncComponent(() => import("./RequestLogsModal.vue"));
const SiteAnnouncementsModal = defineAsyncComponent(() => import("./SiteAnnouncementsModal.vue"));
const AgentRuntimeModal = defineAsyncComponent(() => import("./AgentRuntimeModal.vue"));
const UsageTrendModal = defineAsyncComponent(() => import("./UsageTrendModal.vue"));

const onboardingOpened = useFirstOpenLatch(() => props.onboardingVisible);
const updateOpened = useFirstOpenLatch(() => props.updateDialogVisible);
const availableModelsOpened = useFirstOpenLatch(() => availableModelsVisible.value);
const batchProgressOpened = useFirstOpenLatch(() => batchOperationVisible.value || checkInBatchVisible.value);
const usageOpened = useFirstOpenLatch(() => usageVisible.value);
const requestLogsOpened = useFirstOpenLatch(() => requestLogsVisible.value);
const passwordChangeOpened = useFirstOpenLatch(() => passwordChangeVisible.value);
const accountProfileOpened = useFirstOpenLatch(() => accountProfileVisible.value);
const livenessDetailsOpened = useFirstOpenLatch(() => livenessDetailsVisible.value);
const agentRuntimeOpened = useFirstOpenLatch(() => cliInstancesVisible.value);
const siteAnnouncementsOpened = useFirstOpenLatch(() => siteAnnouncementsVisible.value);
const checkInCalendarOpened = useFirstOpenLatch(() => checkInRecordsVisible.value);
const capabilityProbeOpened = useFirstOpenLatch(() => capabilityProbeVisible.value);
</script>

<template>
  <AppOnboardingModal
    v-if="onboardingOpened"
    :visible="onboardingVisible"
    :provider-count="onboardingProviderCount"
    :cli-configured="onboardingCliConfigured"
    :cli-detecting="onboardingCliDetecting"
    :importing-app-data="importingAppData"
    @add-provider="emit('openOnboardingAddProvider')"
    @import-data="emit('importOnboardingData')"
    @probe-cli="emit('probeOnboardingCli')"
    @open-agent-workspace="emit('openOnboardingAgentWorkspace')"
    @later="emit('dismissOnboarding')"
    @finish="emit('completeOnboarding')"
  />

  <AppUpdateModal
    v-if="updateOpened"
    :visible="updateDialogVisible"
    :current-version="availableUpdateCurrentVersion"
    :version="availableUpdateVersion"
    :release-notes="availableUpdateReleaseNotes"
    :installing="installingUpdate"
    :can-cancel="updateCanCancel"
    :cancelling="cancellingUpdate"
    :download-progress="updateDownloadProgress"
    :install-status="updateInstallStatus"
    @dismiss="emit('dismissUpdate')"
    @cancel="emit('cancelUpdate')"
    @install="emit('installUpdate')"
  />

  <BatchOperationProgressModal
    v-if="batchProgressOpened"
    v-model:visible="batchOperationVisible"
    :operation="batchOperation"
    :running="batchOperationRunning"
    :items="batchOperationItems"
    :waiting-reason="batchOperationWaitingReason"
    :error="batchOperationError"
    :started-at="batchOperationStartedAt"
    :finished-at="batchOperationFinishedAt"
    :completed="batchOperationCompleted"
    :canceling="batchOperationCanceling"
    :cancelled="batchOperationCancelled"
    @cancel-refresh="emit('cancelBatchRefresh')"
  />

  <BatchOperationProgressModal
    v-if="batchProgressOpened"
    v-model:visible="checkInBatchVisible"
    operation="checkIn"
    :running="checkInBatchProgress.running"
    :submitting="checkInBatchProgress.submitting"
    :items="[]"
    :check-in-tasks="checkInBatchProgress.tasks"
    :check-in-pending="checkInPending"
    :skipped-count="checkInBatchProgress.skipped"
    :error="checkInBatchProgress.error"
    :started-at="checkInBatchProgress.startedAt"
    :finished-at="checkInBatchProgress.finishedAt"
    :completed="checkInBatchProgress.completed"
    @resume-check-in="emit('resumeCheckInTask', $event)"
    @cancel-check-in="emit('cancelCheckInTask', $event)"
    @show-check-in-window="emit('showCheckInWindow', $event)"
  />

  <UsageTrendModal
    v-if="usageOpened"
    v-model:visible="usageVisible"
    v-model:period="usagePeriod"
    :provider="usageProvider"
    :loading="usageLoading"
    :error="usageError"
    :summary="usageSummary"
    :overview="usageOverview"
    @refresh="emit('refreshUsageSummary')"
  />

  <AvailableModelsModal
    v-if="availableModelsOpened"
    v-model:visible="availableModelsVisible"
    :provider="availableModelsProvider"
    :providers="providers"
    :loading="availableModelsLoading"
    :error="availableModelsError"
    @refresh="emit('refreshAvailableModels')"
    @copy="emit('copyAvailableModel', $event)"
    @copy-all="emit('copyAvailableModels', $event)"
    @switch="(provider, cliKind, model) => emit('switchAvailableModel', provider, cliKind, model)"
    @launch="(provider, cliKind, model) => emit('launchAvailableModel', provider, cliKind, model)"
  />

  <RequestLogsModal
    v-if="requestLogsOpened"
    v-model:visible="requestLogsVisible"
    :provider="requestLogsProvider"
    :loading="requestLogsLoading"
    :error="requestLogsError"
    :result="requestLogsResult"
    :filters="requestLogsFilters"
    :protocols="providerProtocols"
    :page="requestLogsPage"
    :page-size="requestLogsPageSize"
    @search="emit('searchRequestLogs', $event)"
    @reset="emit('resetRequestLogFilters')"
    @refresh="emit('loadRequestLogs')"
    @page-change="emit('setRequestLogsPage', $event)"
    @page-size-change="emit('setRequestLogsPageSize', $event)"
    @reconcile="requestLogsVisible = false; emit('openUsageReconciliation')"
  />

  <PasswordChangeModal
    v-if="passwordChangeOpened"
    v-model:visible="passwordChangeVisible"
    :provider="passwordChangeProvider"
    :loading="passwordChangeLoading"
    :error="passwordChangeError"
    @submit="(originalPassword, password) => emit('submitPasswordChange', originalPassword, password)"
  />

  <AccountProfileModal
    v-if="accountProfileOpened"
    v-model:visible="accountProfileVisible"
    :provider="accountProfileProvider"
    :loading="accountProfileLoading"
    :error="accountProfileError"
    :redeem-loading="redeemLoading"
    :redeem-error="redeemError"
    @submit-profile="(username, displayName) => emit('submitAccountProfile', username, displayName)"
    @submit-redeem="(code) => emit('submitRedeemCode', code)"
  />

  <LivenessDetailsModal
    v-if="livenessDetailsOpened"
    v-model:visible="livenessDetailsVisible"
    :provider="livenessDetailsProvider"
  />

  <AgentRuntimeModal
    v-if="agentRuntimeOpened"
    v-model:visible="cliInstancesVisible"
    :provider="cliInstancesProvider"
    :cli-kind="cliInstancesKind"
    :cli-runtime="cliRuntime"
    :loading="cliRuntimeLoading"
    :instances="cliInstances"
    :activating-id="activatingCliInstanceId"
    @refresh="emit('refreshCliRuntime')"
    @activate="emit('activateCliInstance', $event)"
  />

  <SiteAnnouncementsModal
    v-if="siteAnnouncementsOpened"
    v-model:visible="siteAnnouncementsVisible"
    :loading="siteAnnouncementsLoading"
    :fatal-error="siteAnnouncementsFatalError"
    :announcements="siteAnnouncements"
    :errors="siteAnnouncementErrors"
    :truncated-sources="siteAnnouncementTruncatedSources"
    :unread-count="unreadSiteAnnouncementCount"
    :marking-fingerprints="markingAnnouncementFingerprints"
    :provider-protocols="providerProtocols"
    :is-read="siteAnnouncementIsRead"
    @refresh="emit('refreshSiteAnnouncements')"
    @mark-read="emit('markSiteAnnouncementRead', $event)"
    @mark-all-read="emit('markAllSiteAnnouncementsRead')"
  />

  <CheckInCalendarModal
    v-if="checkInCalendarOpened"
    v-model:visible="checkInRecordsVisible"
    v-model:month="checkInRecordsMonth"
    :provider="checkInRecordsProvider"
    :loading="checkInRecordsLoading"
    :result="checkInRecordsResult"
    :error="checkInRecordsError"
    @refresh="emit('loadCheckInRecords', { force: true })"
  />

  <CapabilityProbeModal
    v-if="capabilityProbeOpened"
    v-model:visible="capabilityProbeVisible"
    :provider-protocols="providerProtocols"
    :provider="capabilityProbeProvider"
    :running="capabilityProbeRunning"
    :error="capabilityProbeError"
    :result-message="capabilityProbeResultMessage"
    :started-at="capabilityProbeStartedAt"
    :finished-at="capabilityProbeFinishedAt"
    @retry="emit('retryCapabilityProbe')"
  />
</template>
