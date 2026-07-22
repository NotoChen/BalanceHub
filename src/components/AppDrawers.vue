<script setup lang="ts">
import ProviderEditorDrawer from "./ProviderEditorDrawer.vue";
import SettingsDrawer from "./SettingsDrawer.vue";
import TemporaryCliDrawer from "./TemporaryCliDrawer.vue";
import type {
  AppSettings,
  Provider,
  ProviderApiKeyOption,
  ProviderInput,
  ProviderSiteProbeResult,
  TemporaryCliInstance,
} from "../stores/providers";
import type {
  CredentialCompletionState,
  CredentialCompletionStep,
} from "../composables/useProviderCredentialCompletion";
import type { SettingsSaveState } from "../composables/useSettingsController";
import type { DurationUnit } from "../utils/duration";

interface ModelProviderIndexItem {
  model: string;
  providers: { id: string; name: string }[];
}

defineProps<{
  settings: AppSettings;
  settingsSaveState: SettingsSaveState;
  livenessModelOptions: string[];
  modelProviderIndex: ModelProviderIndexItem[];
  exportingAppData: boolean;
  importingAppData: boolean;
  appVersion: string;
  checkingForUpdate: boolean;
  cliRuntimeLoading: boolean;
  cliInstancesProvider: Provider | null;
  cliInstances: TemporaryCliInstance[];
  activatingCliInstanceId: string | null;
  providerEditorTitle: string;
  draftProvider: ProviderInput;
  apiKeyOptions: ProviderApiKeyOption[];
  availableModels: string[];
  siteProbeResult: ProviderSiteProbeResult | null;
  probingSite: boolean;
  siteNameSourceBaseUrl: string;
  testingConnection: boolean;
  credentialAssistantState: CredentialCompletionState;
  credentialAssistantSteps: CredentialCompletionStep[];
  credentialAssistantMessage: string;
  credentialAssistantBusy: boolean;
  canRunCredentialAssistant: boolean;
  credentialAssistantSaved: boolean;
}>();

const emit = defineEmits<{
  testNotification: [];
  exportAppData: [];
  importAppData: [];
  checkForUpdate: [];
  refreshCliRuntime: [];
  activateCliInstance: [instance: TemporaryCliInstance];
  copyApiKey: [];
  selectApiKey: [option: ProviderApiKeyOption];
  runCredentialAssistant: [];
  testConnection: [];
  probeSite: [];
  saveProvider: [];
}>();

const settingsVisible = defineModel<boolean>("settingsVisible", { required: true });
const globalRefreshAmount = defineModel<number>("globalRefreshAmount", { required: true });
const globalRefreshUnit = defineModel<DurationUnit>("globalRefreshUnit", { required: true });
const providerEditorVisible = defineModel<boolean>("providerEditorVisible", { required: true });
const cliInstancesVisible = defineModel<boolean>("cliInstancesVisible", { required: true });
</script>

<template>
  <SettingsDrawer
    v-model:visible="settingsVisible"
    v-model:global-refresh-amount="globalRefreshAmount"
    v-model:global-refresh-unit="globalRefreshUnit"
    :settings="settings"
    :settings-save-state="settingsSaveState"
    :liveness-model-options="livenessModelOptions"
    :model-provider-index="modelProviderIndex"
    :exporting-app-data="exportingAppData"
    :importing-app-data="importingAppData"
    :app-version="appVersion"
    :checking-for-update="checkingForUpdate"
    @test-notification="emit('testNotification')"
    @export-app-data="emit('exportAppData')"
    @import-app-data="emit('importAppData')"
    @check-for-update="emit('checkForUpdate')"
  />

  <ProviderEditorDrawer
    v-model:visible="providerEditorVisible"
    :title="providerEditorTitle"
    :draft="draftProvider"
    :api-key-options="apiKeyOptions"
    :available-models="availableModels"
    :site-probe-result="siteProbeResult"
    :probing-site="probingSite"
    :site-name-source-base-url="siteNameSourceBaseUrl"
    :settings="settings"
    :testing-connection="testingConnection"
    :credential-assistant-state="credentialAssistantState"
    :credential-assistant-steps="credentialAssistantSteps"
    :credential-assistant-message="credentialAssistantMessage"
    :credential-assistant-busy="credentialAssistantBusy"
    :can-run-credential-assistant="canRunCredentialAssistant"
    :credential-assistant-saved="credentialAssistantSaved"
    @copy-api-key="emit('copyApiKey')"
    @select-api-key="emit('selectApiKey', $event)"
    @run-credential-assistant="emit('runCredentialAssistant')"
    @test-connection="emit('testConnection')"
    @probe-site="emit('probeSite')"
    @save="emit('saveProvider')"
  />

  <TemporaryCliDrawer
    v-model:visible="cliInstancesVisible"
    :provider="cliInstancesProvider"
    :loading="cliRuntimeLoading"
    :instances="cliInstances"
    :activating-id="activatingCliInstanceId"
    @refresh="emit('refreshCliRuntime')"
    @activate="emit('activateCliInstance', $event)"
  />
</template>
