<script setup lang="ts">
import ProviderEditorAdvancedSection from "./provider-editor/ProviderEditorAdvancedSection.vue";
import ProviderEditorBasicsSection from "./provider-editor/ProviderEditorBasicsSection.vue";
import ProviderEditorCredentialsSection from "./provider-editor/ProviderEditorCredentialsSection.vue";
import ProviderCredentialAssistant from "./provider-editor/ProviderCredentialAssistant.vue";
import type { AppSettings, ProviderInput } from "../stores/providers";
import type {
  CredentialCompletionState,
  CredentialCompletionStep,
} from "../composables/useProviderCredentialCompletion";

defineProps<{
  visible: boolean;
  title: string;
  draft: ProviderInput;
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
  "run-credential-assistant": [];
  "test-connection": [];
  save: [];
}>();
</script>

<template>
  <a-drawer
    :visible="visible"
    :width="520"
    :title="title"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <a-form :model="draft" layout="vertical">
      <ProviderEditorBasicsSection :draft="draft" />
      <ProviderEditorCredentialsSection
        :draft="draft"
        @copy-api-key="emit('copy-api-key')"
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
      <ProviderEditorAdvancedSection :draft="draft" :settings="settings" />
    </a-form>

    <template #footer>
      <div class="drawer-footer">
        <a-tooltip content="按 Cookie、访问令牌、API 密钥顺序测试">
          <a-button
            :loading="testingConnection"
            :disabled="!draft.identity.baseUrl"
            @click="emit('test-connection')"
          >
            测试连接
          </a-button>
        </a-tooltip>
        <a-button @click="emit('update:visible', false)">取消</a-button>
        <a-button
          type="primary"
          :disabled="!draft.identity.baseUrl"
          @click="emit('save')"
        >
          保存
        </a-button>
      </div>
    </template>
  </a-drawer>
</template>
