<script setup lang="ts">
import ProviderEditorDrawer from "./ProviderEditorDrawer.vue";
import SettingsDrawer from "./SettingsDrawer.vue";
import type {
  AppSettings,
  ProviderApiKeyOption,
  ProviderInput,
  ProviderProtocol,
  ProviderProtocolDescriptor,
  ProviderProtocolDetectionResult,
  ProviderSiteProbeResult,
} from "../stores/providers";
import type {
  CredentialCompletionState,
  CredentialCompletionStep,
} from "../composables/useProviderCredentialCompletion";
import type {
  ProtocolSelectionSource,
  ProviderEditorStep,
} from "../composables/provider-editor-shared";
import type { SettingsSaveState } from "../composables/useSettingsController";
import type { DurationUnit } from "../utils/duration";

defineProps<{
  settings: AppSettings;
  settingsSaveState: SettingsSaveState;
  livenessModelOptions: string[];
  selectedLivenessModelProviders: { id: string; name: string }[];
  exportingAppData: boolean;
  importingAppData: boolean;
  appVersion: string;
  checkingForUpdate: boolean;
  providerEditorTitle: string;
  providerEditorSession: number;
  providerEditorInitialStep: ProviderEditorStep;
  draftProvider: ProviderInput;
  providerProtocols: ProviderProtocolDescriptor[];
  apiKeyOptions: ProviderApiKeyOption[];
  availableModels: string[];
  siteProbeResult: ProviderSiteProbeResult | null;
  protocolDetectionResult: ProviderProtocolDetectionResult | null;
  protocolSelectionSource: ProtocolSelectionSource;
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
  copyApiKey: [];
  selectApiKey: [option: ProviderApiKeyOption];
  runCredentialAssistant: [];
  testConnection: [];
  probeSite: [options?: { force?: boolean }];
  selectProtocol: [protocol: ProviderProtocol];
  saveProvider: [];
}>();

const settingsVisible = defineModel<boolean>("settingsVisible", { required: true });
const globalRefreshAmount = defineModel<number>("globalRefreshAmount", { required: true });
const globalRefreshUnit = defineModel<DurationUnit>("globalRefreshUnit", { required: true });
const providerEditorVisible = defineModel<boolean>("providerEditorVisible", { required: true });
</script>

<template>
  <SettingsDrawer
    v-model:visible="settingsVisible"
    v-model:global-refresh-amount="globalRefreshAmount"
    v-model:global-refresh-unit="globalRefreshUnit"
    :settings="settings"
    :settings-save-state="settingsSaveState"
    :liveness-model-options="livenessModelOptions"
    :selected-liveness-model-providers="selectedLivenessModelProviders"
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
    :editor-session="providerEditorSession"
    :initial-step="providerEditorInitialStep"
    :draft="draftProvider"
    :provider-protocols="providerProtocols"
    :api-key-options="apiKeyOptions"
    :available-models="availableModels"
    :site-probe-result="siteProbeResult"
    :protocol-detection-result="protocolDetectionResult"
    :protocol-selection-source="protocolSelectionSource"
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
    @probe-site="emit('probeSite', $event)"
    @select-protocol="emit('selectProtocol', $event)"
    @save="emit('saveProvider')"
  />

</template>
