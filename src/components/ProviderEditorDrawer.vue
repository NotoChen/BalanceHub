<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  IconCheckCircle,
  IconCloud,
  IconExperiment,
  IconLock,
  IconSave,
  IconTool,
} from "@arco-design/web-vue/es/icon";
import ProviderEditorAdvancedSection from "./provider-editor/ProviderEditorAdvancedSection.vue";
import ProviderEditorBasicsSection from "./provider-editor/ProviderEditorBasicsSection.vue";
import ProviderEditorCredentialsSection from "./provider-editor/ProviderEditorCredentialsSection.vue";
import ProviderCredentialAssistant from "./provider-editor/ProviderCredentialAssistant.vue";
import type { AppSettings, ProviderApiKeyOption, ProviderInput, ProviderSiteProbeResult } from "../stores/providers";
import type {
  CredentialCompletionState,
  CredentialCompletionStep,
} from "../composables/useProviderCredentialCompletion";

type EditorStep = "basics" | "credentials" | "advanced";

const props = defineProps<{
  visible: boolean;
  title: string;
  draft: ProviderInput;
  apiKeyOptions: ProviderApiKeyOption[];
  availableModels: string[];
  siteProbeResult: ProviderSiteProbeResult | null;
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
  "select-api-key": [option: ProviderApiKeyOption];
  "run-credential-assistant": [];
  "test-connection": [];
  "probe-site": [];
  save: [];
}>();

const activeStep = ref<EditorStep>("basics");

const steps: Record<EditorStep, { label: string; icon: typeof IconCloud }> = {
  basics: { label: "基础信息", icon: IconCloud },
  credentials: { label: "认证凭据", icon: IconLock },
  advanced: { label: "运行策略", icon: IconTool },
};

const activeStepMeta = computed(() => steps[activeStep.value]);
const activeStepIndex = computed(() => Object.keys(steps).indexOf(activeStep.value) + 1);
const stepKeys = Object.keys(steps) as EditorStep[];

const authLabel = computed(() => {
  if (props.draft.auth.mode === "session") return "Cookie";
  if (props.draft.auth.mode === "accessToken") return "访问令牌";
  if (props.draft.auth.mode === "password") return "账号密码";
  return "API Key";
});

const credentialReady = computed(() => {
  if (props.draft.auth.mode === "session") {
    return Boolean(props.draft.auth.sessionCookie.trim());
  }
  if (props.draft.auth.mode === "accessToken") {
    return Boolean(props.draft.auth.accessToken.trim() && props.draft.auth.apiUser.trim());
  }
  if (props.draft.auth.mode === "password") {
    return Boolean(props.draft.auth.loginUsername.trim() && props.draft.auth.loginPassword.trim());
  }
  return Boolean(props.draft.auth.apiKey.trim());
});

const connectionReady = computed(() =>
  Boolean(props.draft.identity.baseUrl.trim()) && credentialReady.value,
);

function selectStep(step: EditorStep) {
  activeStep.value = step;
}

function stepComplete(step: EditorStep) {
  if (step === "basics") return Boolean(props.draft.identity.baseUrl.trim());
  if (step === "credentials") return credentialReady.value;
  return true;
}

function goPrevious() {
  const index = Math.max(0, stepKeys.indexOf(activeStep.value) - 1);
  activeStep.value = stepKeys[index];
}

function goNext() {
  const index = Math.min(stepKeys.length - 1, stepKeys.indexOf(activeStep.value) + 1);
  activeStep.value = stepKeys[index];
}

watch(
  () => props.visible,
  (visible) => {
    if (visible) {
      activeStep.value = "basics";
    }
  },
);
</script>

<template>
  <a-modal
    :visible="visible"
    :width="1020"
    modal-class="surface-modal provider-editor-modal provider-editor-modal-v3"
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
        <span class="surface-modal-title-meta" :class="{ ready: connectionReady }">{{ authLabel }}</span>
      </div>
    </template>

    <div class="provider-editor-studio">
      <aside class="provider-editor-rail" aria-label="中转站配置步骤">
        <nav class="provider-editor-stepbar provider-editor-stepbar-v3">
          <button
            v-for="(step, key) in steps"
            :key="key"
            type="button"
            class="provider-editor-step-tab"
            :class="{ active: activeStep === key, complete: stepComplete(key as EditorStep) }"
            :aria-current="activeStep === key ? 'step' : undefined"
            @click="selectStep(key as EditorStep)"
          >
            <span class="provider-editor-step-tab-icon"><component :is="step.icon" /></span>
            <span class="provider-editor-step-tab-copy"><strong>{{ step.label }}</strong></span>
            <IconCheckCircle v-if="stepComplete(key as EditorStep)" class="provider-editor-step-tab-complete" />
          </button>
        </nav>
      </aside>

      <section class="provider-editor-main">
        <header class="provider-editor-workflow-header">
          <div>
            <h2>{{ activeStepMeta.label }}</h2>
          </div>
          <div class="provider-editor-workflow-status" :class="{ ready: connectionReady }">
            <i />
            <span>{{ connectionReady ? "可以测试连接" : "等待补全必填项" }}</span>
          </div>
        </header>

        <div class="provider-editor-stage">
          <main class="provider-editor-canvas">
            <div class="provider-editor-main-scroll">
              <a-form :model="draft" layout="vertical">
                <ProviderEditorBasicsSection
                  v-if="activeStep === 'basics'"
                  :draft="draft"
                  :site-probe-result="siteProbeResult"
                  :probing-site="probingSite"
                  :site-name-source-base-url="siteNameSourceBaseUrl"
                  @probe-site="emit('probe-site')"
                />
                <template v-else-if="activeStep === 'credentials'">
                  <ProviderEditorCredentialsSection
                    :draft="draft"
                    :api-key-options="apiKeyOptions"
                    @copy-api-key="emit('copy-api-key')"
                    @select-api-key="emit('select-api-key', $event)"
                  />
                  <ProviderCredentialAssistant
                    :draft="draft"
                    :state="credentialAssistantState"
                    :steps="credentialAssistantSteps"
                    :message="credentialAssistantMessage"
                    :busy="credentialAssistantBusy"
                    :can-run="canRunCredentialAssistant"
                    :saved="credentialAssistantSaved"
                    @run="emit('run-credential-assistant')"
                  />
                </template>
                <ProviderEditorAdvancedSection
                  v-else
                  :draft="draft"
                  :settings="settings"
                  :available-models="availableModels"
                  :initially-expanded="true"
                />
              </a-form>
            </div>
          </main>

        </div>

        <footer class="provider-editor-main-footer">
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
        <a-button v-if="activeStepIndex > 1" type="text" @click="goPrevious">上一步</a-button>
        <a-button v-if="activeStepIndex < 3" type="secondary" @click="goNext">下一步</a-button>
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
      </section>
    </div>
  </a-modal>
</template>
