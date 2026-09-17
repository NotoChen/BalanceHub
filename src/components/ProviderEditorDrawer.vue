<script setup lang="ts">
import { ref } from "vue";
import {
  IconCloud,
  IconExperiment,
  IconSave,
} from "@arco-design/web-vue/es/icon";
import ProviderEditorAdvancedSection from "./provider-editor/ProviderEditorAdvancedSection.vue";
import ProviderEditorBasicsSection from "./provider-editor/ProviderEditorBasicsSection.vue";
import ProviderEditorCredentialsSection from "./provider-editor/ProviderEditorCredentialsSection.vue";
import ProviderCredentialAssistant from "./provider-editor/ProviderCredentialAssistant.vue";
import type {
  AppSettings,
  Provider,
  ProviderApiKeyOption,
  ProviderInput,
  ProviderProtocol,
  ProviderProtocolDescriptor,
  ProviderProtocolDetectionResult,
  ProviderSiteProbeResult,
} from "../stores/providers";
import type { ApiKeyManagerOperation } from "../composables/useApiKeyManager";
import type {
  ProtocolSelectionSource,
  ProviderEditorSection,
} from "../composables/provider-editor-shared";
import type {
  CredentialCompletionState,
  CredentialCompletionStep,
} from "../composables/useProviderCredentialCompletion";

const props = defineProps<{
  visible: boolean;
  editorSession: number;
  initialSection: ProviderEditorSection;
  title: string;
  draft: ProviderInput;
  providerProtocols: ProviderProtocolDescriptor[];
  apiKeyOptions: ProviderApiKeyOption[];
  apiKeyRemoteManaged: boolean;
  apiKeyManagerProvider: Provider | null;
  apiKeyManagerOperation: ApiKeyManagerOperation | null;
  apiKeyCreateVisible: boolean;
  apiKeyCreateName: string;
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
  settings: AppSettings;
  testingConnection: boolean;
  credentialAssistantState: CredentialCompletionState;
  credentialAssistantSteps: CredentialCompletionStep[];
  credentialAssistantMessage: string;
  credentialAssistantBusy: boolean;
  canRunCredentialAssistant: boolean;
  credentialAssistantSaved: boolean;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  "copy-api-key": [];
  "update:api-key-create-visible": [visible: boolean];
  "update:api-key-create-name": [name: string];
  "update:api-key-add-visible": [visible: boolean];
  "update:api-key-add-remark": [remark: string];
  "update:api-key-add-value": [value: string];
  "update:api-key-remark-visible": [visible: boolean];
  "update:api-key-remark-value": [remark: string];
  "sync-remote-api-keys": [];
  "open-api-key-create-panel": [];
  "open-api-key-add-panel": [];
  "open-api-key-remark-editor": [option: ProviderApiKeyOption];
  "create-managed-api-key": [];
  "add-local-api-key": [];
  "save-managed-api-key-remark": [];
  "set-default-managed-api-key": [option: ProviderApiKeyOption];
  "copy-managed-api-key": [option: ProviderApiKeyOption];
  "delete-managed-api-key": [option: ProviderApiKeyOption];
  "run-credential-assistant": [];
  "test-connection": [];
  "probe-site": [options?: { force?: boolean }];
  "select-protocol": [protocol: ProviderProtocol];
  save: [];
}>();

const formScroll = ref<HTMLElement | null>(null);

function scrollToInitialSection() {
  const container = formScroll.value;
  if (!container) return;
  container.scrollTop = 0;
  if (props.initialSection !== "basics") {
    container.querySelector<HTMLElement>(`[data-provider-section="${props.initialSection}"]`)
      ?.scrollIntoView({ block: "start" });
  }
}
</script>

<template>
  <a-modal
    :visible="visible"
    :width="1020"
    modal-class="surface-modal provider-editor-modal provider-editor-modal-v3 provider-editor-unified"
    :footer="false"
    unmount-on-close
    @open="scrollToInitialSection"
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title provider-editor-title">
        <span class="surface-modal-title-icon"><IconCloud /></span>
        <span class="surface-modal-title-copy">
          <strong>{{ title }}</strong>
        </span>
      </div>
    </template>

    <div class="provider-editor-layout">
      <div ref="formScroll" class="provider-editor-scroll">
        <a-form :key="editorSession" :model="draft" layout="vertical" class="provider-editor-form">
          <ProviderEditorBasicsSection
            data-provider-section="basics"
            :draft="draft"
            :provider-protocols="providerProtocols"
            :site-probe-result="siteProbeResult"
            :protocol-detection-result="protocolDetectionResult"
            :protocol-selection-source="protocolSelectionSource"
            :probing-site="probingSite"
            :site-name-source-base-url="siteNameSourceBaseUrl"
            @probe-site="emit('probe-site', $event)"
            @select-protocol="emit('select-protocol', $event)"
          />
          <div data-provider-section="credentials" class="provider-editor-credentials">
            <ProviderEditorCredentialsSection
              :draft="draft"
              :provider-protocols="providerProtocols"
              :api-key-options="apiKeyOptions"
              :api-key-remote-managed="apiKeyRemoteManaged"
              :api-key-manager-provider="apiKeyManagerProvider"
              :api-key-manager-operation="apiKeyManagerOperation"
              :api-key-create-visible="apiKeyCreateVisible"
              :api-key-create-name="apiKeyCreateName"
              :api-key-add-visible="apiKeyAddVisible"
              :api-key-add-remark="apiKeyAddRemark"
              :api-key-add-value="apiKeyAddValue"
              :api-key-remark-visible="apiKeyRemarkVisible"
              :api-key-remark-value="apiKeyRemarkValue"
              :api-key-remark-target="apiKeyRemarkTarget"
              @copy-api-key="emit('copy-api-key')"
              @update:api-key-create-visible="emit('update:api-key-create-visible', $event)"
              @update:api-key-create-name="emit('update:api-key-create-name', $event)"
              @update:api-key-add-visible="emit('update:api-key-add-visible', $event)"
              @update:api-key-add-remark="emit('update:api-key-add-remark', $event)"
              @update:api-key-add-value="emit('update:api-key-add-value', $event)"
              @update:api-key-remark-visible="emit('update:api-key-remark-visible', $event)"
              @update:api-key-remark-value="emit('update:api-key-remark-value', $event)"
              @sync-remote-api-keys="emit('sync-remote-api-keys')"
              @open-api-key-create-panel="emit('open-api-key-create-panel')"
              @open-api-key-add-panel="emit('open-api-key-add-panel')"
              @open-api-key-remark-editor="emit('open-api-key-remark-editor', $event)"
              @create-managed-api-key="emit('create-managed-api-key')"
              @add-local-api-key="emit('add-local-api-key')"
              @save-managed-api-key-remark="emit('save-managed-api-key-remark')"
              @set-default-managed-api-key="emit('set-default-managed-api-key', $event)"
              @copy-managed-api-key="emit('copy-managed-api-key', $event)"
              @delete-managed-api-key="emit('delete-managed-api-key', $event)"
            />
            <ProviderCredentialAssistant
              :draft="draft"
              :provider-protocols="providerProtocols"
              :state="credentialAssistantState"
              :steps="credentialAssistantSteps"
              :message="credentialAssistantMessage"
              :busy="credentialAssistantBusy"
              :can-run="canRunCredentialAssistant"
              :saved="credentialAssistantSaved"
              @run="emit('run-credential-assistant')"
            />
          </div>
          <ProviderEditorAdvancedSection
            data-provider-section="advanced"
            :draft="draft"
            :settings="settings"
            :available-models="availableModels"
          />
        </a-form>
      </div>
      <footer class="provider-editor-footer">
        <a-tooltip content="测试当前认证方式">
          <a-button
            :loading="testingConnection"
            :disabled="!draft.identity.baseUrl"
            @click="emit('test-connection')"
          >
            <template #icon><IconExperiment /></template>
            测试连接
          </a-button>
        </a-tooltip>
        <span class="provider-editor-footer-spacer" />
        <a-button @click="emit('update:visible', false)">取消</a-button>
        <a-button
          type="primary"
          :disabled="!draft.identity.baseUrl"
          @click="emit('save')"
        >
          <template #icon><IconSave /></template>
          保存中转站
        </a-button>
      </footer>
    </div>
  </a-modal>
</template>
