<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import {
  IconCloud,
  IconExperiment,
  IconSave,
} from "@arco-design/web-vue/es/icon";
import ProviderEditorAutomationSection from "./provider-editor/ProviderEditorAutomationSection.vue";
import ProviderEditorBasicsSection from "./provider-editor/ProviderEditorBasicsSection.vue";
import ProviderEditorCredentialsSection from "./provider-editor/ProviderEditorCredentialsSection.vue";
import ProviderEditorModelsSection from "./provider-editor/ProviderEditorModelsSection.vue";
import ProviderEditorNav from "./provider-editor/ProviderEditorNav.vue";
import ProviderEditorNetworkSection from "./provider-editor/ProviderEditorNetworkSection.vue";
import ProviderEditorQuotaSpecSection from "./provider-editor/ProviderEditorQuotaSpecSection.vue";
import ProviderEditorRequestSection from "./provider-editor/ProviderEditorRequestSection.vue";
import ProviderCredentialAssistant from "./provider-editor/ProviderCredentialAssistant.vue";
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
  ProtocolSelectionSource,
  ProviderEditorSection,
} from "../composables/provider-editor-shared";
import type {
  CredentialCompletionState,
  CredentialCompletionStep,
} from "../composables/useProviderCredentialCompletion";
import { useProviderEditorSections } from "../composables/useProviderEditorSections";
import { providerScopeNoun } from "../utils/provider-protocol";

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
  connectionTestResult: ProviderConnectionTestResult | null;
  saving: boolean;
  saveError: string;
  startingBrowserLogin: boolean;
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
  "update:api-key-add-visible": [visible: boolean];
  "update:api-key-add-remark": [remark: string];
  "update:api-key-add-value": [value: string];
  "update:api-key-remark-visible": [visible: boolean];
  "update:api-key-remark-value": [remark: string];
  "sync-remote-api-keys": [];
  "open-api-key-create-editor": [];
  "open-api-key-settings-editor": [option: ProviderInput["auth"]["apiKeyOptions"][number]];
  "open-api-key-add-panel": [];
  "open-api-key-remark-editor": [option: ProviderApiKeyOption];
  "add-local-api-key": [];
  "save-managed-api-key-remark": [];
  "set-default-managed-api-key": [option: ProviderApiKeyOption];
  "copy-managed-api-key": [option: ProviderApiKeyOption];
  "delete-managed-api-key": [option: ProviderApiKeyOption];
  "run-credential-assistant": [];
  "test-connection": [];
  "login-and-import": [];
  "probe-site": [options?: { force?: boolean }];
  "select-protocol": [protocol: ProviderProtocol];
  save: [];
}>();

const { navItems } = useProviderEditorSections(props);
const panelScroll = ref<HTMLElement | null>(null);

function availableSection(section: ProviderEditorSection) {
  return navItems.value.some((item) => item.id === section) ? section : "basics";
}

/// 打开窗口或切换分区都从这里进入：只渲染当前分区，滚动位置回到顶部。
/// 整页平铺时深链要按元素位置猜落点，换成分区状态后不再需要。
const activeSection = ref<ProviderEditorSection>(availableSection(props.initialSection));
const activeItem = computed(
  () => navItems.value.find((item) => item.id === activeSection.value) ?? navItems.value[0],
);

function activate(section: ProviderEditorSection) {
  activeSection.value = availableSection(section);
  nextTick(() => panelScroll.value?.scrollTo({ top: 0 }));
}

watch(() => props.editorSession, () => activate(props.initialSection), { flush: "post" });
watch(
  () => props.visible,
  (visible) => {
    if (visible) activate(props.initialSection);
  },
);
/// 草稿里换了协议后，原分区可能不再适用（例如换到只支持 API Key 的通用
/// 中转站后，额度接口分区就消失了）；回到基础信息，而不是停在空面板上。
watch(
  () => props.draft.identity.protocol,
  () => {
    if (!navItems.value.some((item) => item.id === activeSection.value)) {
      activate("basics");
    }
  },
);

/// 保存按钮和窗口标题用同一个称呼：正在编辑的是中转站、账号还是网关。
const scopeNoun = computed(() => providerScopeNoun(props.draft.identity.protocol));
</script>

<template>
  <a-modal
    :visible="visible"
    :width="1020"
    modal-class="surface-modal provider-editor-modal provider-editor-modal-v3 provider-editor-unified"
    :footer="false"
    unmount-on-close
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
      <div class="provider-editor-workspace">
        <ProviderEditorNav :items="navItems" :active="activeSection" @select="activate" />
        <main class="provider-editor-panel">
          <div ref="panelScroll" class="provider-editor-scroll">
            <div class="provider-editor-panel-content">
              <header class="provider-editor-pane-heading">
                <h2 :id="`provider-editor-pane-${activeSection}`">{{ activeItem.title }}</h2>
                <p>{{ activeItem.description }}</p>
              </header>
              <a-form :key="editorSession" :model="draft" :disabled="saving" layout="vertical" class="provider-editor-form">
                <section
                  class="provider-editor-pane"
                  :class="{ 'is-rows': activeItem.rows }"
                  :aria-labelledby="`provider-editor-pane-${activeSection}`"
                >
                  <ProviderEditorBasicsSection
                    v-if="activeSection === 'basics'"
                    :draft="draft"
                    :disabled="saving"
                    :provider-protocols="providerProtocols"
                    :site-probe-result="siteProbeResult"
                    :protocol-detection-result="protocolDetectionResult"
                    :protocol-selection-source="protocolSelectionSource"
                    :probing-site="probingSite"
                    :site-name-source-base-url="siteNameSourceBaseUrl"
                    @probe-site="emit('probe-site', $event)"
                    @select-protocol="emit('select-protocol', $event)"
                  />
                  <ProviderEditorCredentialsSection
                    v-else-if="activeSection === 'credentials'"
                    :starting-browser-login="startingBrowserLogin"
                    :disabled="saving"
                    @login-and-import="emit('login-and-import')"
                    :draft="draft"
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
                    @copy-api-key="emit('copy-api-key')"
                    @update:api-key-add-visible="emit('update:api-key-add-visible', $event)"
                    @update:api-key-add-remark="emit('update:api-key-add-remark', $event)"
                    @update:api-key-add-value="emit('update:api-key-add-value', $event)"
                    @update:api-key-remark-visible="emit('update:api-key-remark-visible', $event)"
                    @update:api-key-remark-value="emit('update:api-key-remark-value', $event)"
                    @sync-remote-api-keys="emit('sync-remote-api-keys')"
                    @open-api-key-create-editor="emit('open-api-key-create-editor')"
                    @open-api-key-settings-editor="emit('open-api-key-settings-editor', $event)"
                    @open-api-key-add-panel="emit('open-api-key-add-panel')"
                    @open-api-key-remark-editor="emit('open-api-key-remark-editor', $event)"
                    @add-local-api-key="emit('add-local-api-key')"
                    @save-managed-api-key-remark="emit('save-managed-api-key-remark')"
                    @set-default-managed-api-key="emit('set-default-managed-api-key', $event)"
                    @copy-managed-api-key="emit('copy-managed-api-key', $event)"
                    @delete-managed-api-key="emit('delete-managed-api-key', $event)"
                  >
                    <template #assistant>
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
                    </template>
                  </ProviderEditorCredentialsSection>
                  <ProviderEditorQuotaSpecSection
                    v-else-if="activeSection === 'quota'"
                    :draft="draft"
                    :provider-protocols="providerProtocols"
                    :testing-connection="testingConnection"
                    :connection-test-result="connectionTestResult"
                    @test-connection="emit('test-connection')"
                  />
                  <ProviderEditorModelsSection
                    v-else-if="activeSection === 'models'"
                    :draft="draft"
                    :available-models="availableModels"
                    :provider-protocols="providerProtocols"
                  />
                  <ProviderEditorRequestSection
                    v-else-if="activeSection === 'request'"
                    :draft="draft"
                    :provider-protocols="providerProtocols"
                  />
                  <ProviderEditorAutomationSection
                    v-else-if="activeSection === 'automation'"
                    :draft="draft"
                    :settings="settings"
                    :available-models="availableModels"
                    :provider-protocols="providerProtocols"
                  />
                  <ProviderEditorNetworkSection
                    v-else-if="activeSection === 'network'"
                    :draft="draft"
                    :settings="settings"
                  />
                </section>
              </a-form>
            </div>
          </div>
        </main>
      </div>
      <div v-if="saveError || connectionTestResult" class="provider-editor-feedback" aria-live="polite">
        <a-alert v-if="saveError" type="error" show-icon>{{ saveError }}</a-alert>
        <a-alert v-else-if="connectionTestResult" :type="connectionTestResult.ok ? 'success' : 'error'" show-icon>
          <strong>{{ connectionTestResult.ok ? '连接测试通过' : '连接测试未通过' }}</strong>
          <span v-if="connectionTestResult.message"> · {{ connectionTestResult.message }}</span>
          <details v-if="connectionTestResult.steps.length" class="provider-test-steps">
            <summary>查看测试过程</summary>
            <div v-for="(step, index) in connectionTestResult.steps" :key="index" :class="{ 'is-error': !step.ok }">
              <strong>{{ step.name }}</strong><span>{{ step.message }}</span>
            </div>
          </details>
        </a-alert>
      </div>
      <footer class="provider-editor-footer">
        <a-tooltip content="测试当前认证方式">
          <a-button
            :loading="testingConnection"
            :disabled="!draft.identity.baseUrl.trim() || saving || credentialAssistantBusy || startingBrowserLogin"
            @click="emit('test-connection')"
          >
            <template #icon><IconExperiment /></template>
            测试连接
          </a-button>
        </a-tooltip>
        <span class="provider-editor-footer-spacer" />
        <a-button @click="emit('update:visible', false)">{{ saving ? '关闭' : '取消' }}</a-button>
        <a-button
          type="primary"
          :loading="saving"
          :disabled="!draft.identity.baseUrl.trim() || credentialAssistantBusy || startingBrowserLogin"
          @click="emit('save')"
        >
          <template #icon><IconSave /></template>
          {{ saving ? (credentialAssistantBusy ? '正在补全凭据' : '正在保存') : `保存${scopeNoun}` }}
        </a-button>
      </footer>
    </div>
  </a-modal>
</template>
