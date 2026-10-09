<script setup lang="ts">
import ProviderEditorDrawer from "./ProviderEditorDrawer.vue";
import SettingsDrawer from "./SettingsDrawer.vue";
import type {
  AppSettings,
  Provider,
  ProviderApiKeyOption,
  ProviderConnectionTestResult,
  ProviderInput,
  ProviderProtocol,
  ProviderProtocolDescriptor,
  ProviderProtocolDetectionResult,
  ProviderSiteProbeResult,
} from "../stores/providers";
import type { ApiKeyManagerOperation } from "../composables/useApiKeyManager";
import type {
  CredentialCompletionState,
  CredentialCompletionStep,
} from "../composables/useProviderCredentialCompletion";
import type {
  ProtocolSelectionSource,
  ProviderEditorSection,
} from "../composables/provider-editor-shared";
import type { SettingsSaveState } from "../composables/useSettingsController";
import type { NotificationSendResult } from "../api/app";

defineProps<{
  settings: AppSettings;
  settingsSaveState: SettingsSaveState;
  settingsSaveError: string;
  testingNotification: boolean;
  notificationTestResult: NotificationSendResult | null;
  notificationTestError: string;
  livenessModelOptions: string[];
  selectedLivenessModelProviders: { id: string; name: string }[];
  exportingAppData: boolean;
  importingAppData: boolean;
  appVersion: string;
  checkingForUpdate: boolean;
  providerEditorTitle: string;
  providerEditorSession: number;
  providerEditorInitialSection: ProviderEditorSection;
  draftProvider: ProviderInput;
  providerProtocols: ProviderProtocolDescriptor[];
  apiKeyOptions: ProviderApiKeyOption[];
  apiKeyRemoteManaged: boolean;
  apiKeyManagerProvider: Provider | null;
  apiKeyManagerOperation: ApiKeyManagerOperation | null;
  apiKeyAddVisible: boolean;
  apiKeyAddRemark: string;
  apiKeyAddValue: string;
  apiKeyRemarkVisible: boolean;
  apiKeyRemarkValue: string;
  apiKeyRemarkTarget: ProviderApiKeyOption | null;
  availableModels: string[];
  siteProbeResult: ProviderSiteProbeResult | null;
  protocolDetectionResult: ProviderProtocolDetectionResult | null;
  protocolSelectionSource: ProtocolSelectionSource;
  probingSite: boolean;
  siteNameSourceBaseUrl: string;
  testingConnection: boolean;
  connectionTestResult: ProviderConnectionTestResult | null;
  savingProvider: boolean;
  providerSaveError: string;
  startingBrowserLogin: boolean;
  credentialAssistantState: CredentialCompletionState;
  credentialAssistantSteps: CredentialCompletionStep[];
  credentialAssistantMessage: string;
  credentialAssistantBusy: boolean;
  canRunCredentialAssistant: boolean;
  credentialAssistantSaved: boolean;
}>();

const emit = defineEmits<{
  retrySettingsSave: [];
  testNotification: [];
  exportAppData: [];
  importAppData: [];
  checkForUpdate: [];
  copyApiKey: [];
  syncRemoteApiKeys: [];
  openApiKeyCreateEditor: [];
  openApiKeySettingsEditor: [option: ProviderApiKeyOption];
  openApiKeyAddPanel: [];
  openApiKeyRemarkEditor: [option: ProviderApiKeyOption];
  addLocalApiKey: [];
  saveManagedApiKeyRemark: [];
  setDefaultManagedApiKey: [option: ProviderApiKeyOption];
  copyManagedApiKey: [option: ProviderApiKeyOption];
  deleteManagedApiKey: [option: ProviderApiKeyOption];
  runCredentialAssistant: [];
  testConnection: [];
  loginAndImport: [];
  probeSite: [options?: { force?: boolean }];
  selectProtocol: [protocol: ProviderProtocol];
  saveProvider: [];
}>();

const settingsVisible = defineModel<boolean>("settingsVisible", { required: true });
const providerEditorVisible = defineModel<boolean>("providerEditorVisible", { required: true });
const apiKeyAddVisible = defineModel<boolean>("apiKeyAddVisible", { required: true });
const apiKeyAddRemark = defineModel<string>("apiKeyAddRemark", { required: true });
const apiKeyAddValue = defineModel<string>("apiKeyAddValue", { required: true });
const apiKeyRemarkVisible = defineModel<boolean>("apiKeyRemarkVisible", { required: true });
const apiKeyRemarkValue = defineModel<string>("apiKeyRemarkValue", { required: true });
</script>

<template>
  <SettingsDrawer
    v-model:visible="settingsVisible"
    :settings="settings"
    :settings-save-state="settingsSaveState"
    :settings-save-error="settingsSaveError"
    :testing-notification="testingNotification"
    :notification-test-result="notificationTestResult"
    :notification-test-error="notificationTestError"
    @retry-save="emit('retrySettingsSave')"
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
    :initial-section="providerEditorInitialSection"
    :draft="draftProvider"
    :provider-protocols="providerProtocols"
    :api-key-options="apiKeyOptions"
    :api-key-remote-managed="apiKeyRemoteManaged"
    :api-key-manager-provider="apiKeyManagerProvider"
    :api-key-manager-operation="apiKeyManagerOperation"
    :api-key-add-visible="apiKeyAddVisible"
    :api-key-add-remark="apiKeyAddRemark"
    :api-key-add-value="apiKeyAddValue"
    :api-key-remark-visible="apiKeyRemarkVisible"
    :api-key-remark-value="apiKeyRemarkValue"
    :api-key-remark-target="apiKeyRemarkTarget"
    :available-models="availableModels"
    :site-probe-result="siteProbeResult"
    :protocol-detection-result="protocolDetectionResult"
    :protocol-selection-source="protocolSelectionSource"
    :probing-site="probingSite"
    :site-name-source-base-url="siteNameSourceBaseUrl"
    :settings="settings"
    :starting-browser-login="startingBrowserLogin"
    @login-and-import="emit('loginAndImport')"
    :testing-connection="testingConnection"
    :connection-test-result="connectionTestResult"
    :saving="savingProvider"
    :save-error="providerSaveError"
    :credential-assistant-state="credentialAssistantState"
    :credential-assistant-steps="credentialAssistantSteps"
    :credential-assistant-message="credentialAssistantMessage"
    :credential-assistant-busy="credentialAssistantBusy"
    :can-run-credential-assistant="canRunCredentialAssistant"
    :credential-assistant-saved="credentialAssistantSaved"
    @copy-api-key="emit('copyApiKey')"
    @update:api-key-add-visible="apiKeyAddVisible = $event"
    @update:api-key-add-remark="apiKeyAddRemark = $event"
    @update:api-key-add-value="apiKeyAddValue = $event"
    @update:api-key-remark-visible="apiKeyRemarkVisible = $event"
    @update:api-key-remark-value="apiKeyRemarkValue = $event"
    @sync-remote-api-keys="emit('syncRemoteApiKeys')"
    @open-api-key-create-editor="emit('openApiKeyCreateEditor')"
    @open-api-key-settings-editor="emit('openApiKeySettingsEditor', $event)"
    @open-api-key-add-panel="emit('openApiKeyAddPanel')"
    @open-api-key-remark-editor="emit('openApiKeyRemarkEditor', $event)"
    @add-local-api-key="emit('addLocalApiKey')"
    @save-managed-api-key-remark="emit('saveManagedApiKeyRemark')"
    @set-default-managed-api-key="emit('setDefaultManagedApiKey', $event)"
    @copy-managed-api-key="emit('copyManagedApiKey', $event)"
    @delete-managed-api-key="emit('deleteManagedApiKey', $event)"
    @run-credential-assistant="emit('runCredentialAssistant')"
    @test-connection="emit('testConnection')"
    @probe-site="emit('probeSite', $event)"
    @select-protocol="emit('selectProtocol', $event)"
    @save="emit('saveProvider')"
  />

</template>
