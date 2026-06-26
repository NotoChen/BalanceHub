<script setup lang="ts">
import AppOnboardingModal from "./AppOnboardingModal.vue";
import ApiKeyManagerModal from "./ApiKeyManagerModal.vue";
import CheckInCalendarModal from "./CheckInCalendarModal.vue";
import LivenessDetailsModal from "./LivenessDetailsModal.vue";
import PasswordChangeModal from "./PasswordChangeModal.vue";
import RequestLogsModal from "./RequestLogsModal.vue";
import UsageTrendModal from "./UsageTrendModal.vue";
import type {
  Provider,
  ProviderApiKeyOption,
  ProviderCheckInRecordsResult,
  ProviderRequestLogsResult,
  ProviderUsageSummary,
} from "../stores/providers";
import type { UsagePeriod } from "../utils/usage-trend";

defineProps<{
  onboardingVisible: boolean;
  onboardingProviderCount: number;
  onboardingCliConfigured: boolean;
  importingAppData: boolean;
  probingCodexCliPath: boolean;
  apiKeyManagerProvider: Provider | null;
  apiKeyManagerLoading: boolean;
  apiKeyManagerKeys: ProviderApiKeyOption[];
  usageProvider: Provider | null;
  usageLoading: boolean;
  usageSummary: ProviderUsageSummary | null;
  requestLogsProvider: Provider | null;
  requestLogsLoading: boolean;
  requestLogsResult: ProviderRequestLogsResult | null;
  requestLogsKeyword: string;
  requestLogsPage: number;
  requestLogsPageSize: number;
  passwordChangeProvider: Provider | null;
  passwordChangeLoading: boolean;
  livenessDetailsProvider: Provider | null;
  checkInRecordsProvider: Provider | null;
  checkInRecordsLoading: boolean;
  checkInRecordsResult: ProviderCheckInRecordsResult | null;
  checkInRecordsError: string;
}>();

const emit = defineEmits<{
  openOnboardingAddProvider: [];
  importOnboardingData: [];
  openOnboardingSettings: [];
  probeOnboardingCodexCli: [];
  completeOnboarding: [];
  refreshApiKeyManager: [];
  openApiKeyCreateModal: [];
  createManagedApiKey: [];
  useManagedApiKey: [option: ProviderApiKeyOption];
  copyManagedApiKey: [option: ProviderApiKeyOption];
  deleteManagedApiKey: [option: ProviderApiKeyOption];
  refreshUsageSummary: [];
  searchRequestLogs: [keyword: string];
  loadRequestLogs: [];
  setRequestLogsPage: [page: number];
  setRequestLogsPageSize: [pageSize: number];
  submitPasswordChange: [originalPassword: string, password: string];
  loadCheckInRecords: [options?: { force?: boolean }];
}>();

const apiKeyManagerVisible = defineModel<boolean>("apiKeyManagerVisible", { required: true });
const apiKeyCreateVisible = defineModel<boolean>("apiKeyCreateVisible", { required: true });
const apiKeyCreateName = defineModel<string>("apiKeyCreateName", { required: true });
const usageVisible = defineModel<boolean>("usageVisible", { required: true });
const usagePeriod = defineModel<UsagePeriod>("usagePeriod", { required: true });
const requestLogsVisible = defineModel<boolean>("requestLogsVisible", { required: true });
const passwordChangeVisible = defineModel<boolean>("passwordChangeVisible", { required: true });
const livenessDetailsVisible = defineModel<boolean>("livenessDetailsVisible", { required: true });
const checkInRecordsVisible = defineModel<boolean>("checkInRecordsVisible", { required: true });
const checkInRecordsMonth = defineModel<string>("checkInRecordsMonth", { required: true });
</script>

<template>
  <AppOnboardingModal
    :visible="onboardingVisible"
    :provider-count="onboardingProviderCount"
    :cli-configured="onboardingCliConfigured"
    :importing-app-data="importingAppData"
    :probing-codex-cli="probingCodexCliPath"
    @add-provider="emit('openOnboardingAddProvider')"
    @import-data="emit('importOnboardingData')"
    @open-settings="emit('openOnboardingSettings')"
    @probe-codex-cli="emit('probeOnboardingCodexCli')"
    @finish="emit('completeOnboarding')"
  />

  <ApiKeyManagerModal
    v-model:visible="apiKeyManagerVisible"
    v-model:create-visible="apiKeyCreateVisible"
    v-model:create-name="apiKeyCreateName"
    :provider="apiKeyManagerProvider"
    :loading="apiKeyManagerLoading"
    :keys="apiKeyManagerKeys"
    @refresh="emit('refreshApiKeyManager')"
    @show-create="emit('openApiKeyCreateModal')"
    @create="emit('createManagedApiKey')"
    @use="emit('useManagedApiKey', $event)"
    @copy="emit('copyManagedApiKey', $event)"
    @delete="emit('deleteManagedApiKey', $event)"
  />

  <UsageTrendModal
    v-model:visible="usageVisible"
    v-model:period="usagePeriod"
    :provider="usageProvider"
    :loading="usageLoading"
    :summary="usageSummary"
    @refresh="emit('refreshUsageSummary')"
  />

  <RequestLogsModal
    v-model:visible="requestLogsVisible"
    :provider="requestLogsProvider"
    :loading="requestLogsLoading"
    :result="requestLogsResult"
    :keyword="requestLogsKeyword"
    :page="requestLogsPage"
    :page-size="requestLogsPageSize"
    @search="emit('searchRequestLogs', $event)"
    @refresh="emit('loadRequestLogs')"
    @page-change="emit('setRequestLogsPage', $event)"
    @page-size-change="emit('setRequestLogsPageSize', $event)"
  />

  <PasswordChangeModal
    v-model:visible="passwordChangeVisible"
    :provider="passwordChangeProvider"
    :loading="passwordChangeLoading"
    @submit="(originalPassword, password) => emit('submitPasswordChange', originalPassword, password)"
  />

  <LivenessDetailsModal
    v-model:visible="livenessDetailsVisible"
    :provider="livenessDetailsProvider"
  />

  <CheckInCalendarModal
    v-model:visible="checkInRecordsVisible"
    v-model:month="checkInRecordsMonth"
    :provider="checkInRecordsProvider"
    :loading="checkInRecordsLoading"
    :result="checkInRecordsResult"
    :error="checkInRecordsError"
    @refresh="emit('loadCheckInRecords', { force: true })"
  />
</template>
