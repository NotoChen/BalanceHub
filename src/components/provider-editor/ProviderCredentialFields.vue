<script setup lang="ts">
import { IconCopy } from "@arco-design/web-vue/es/icon";
import type {
  AuthMode,
  ProviderAuthFieldDescriptor,
  ProviderInput,
} from "../../stores/providers";
import ProviderApiKeyPicker from "./ProviderApiKeyPicker.vue";

const props = defineProps<{
  mode: AuthMode;
  fields: ProviderAuthFieldDescriptor[];
  requiredFields: string[];
  draft: ProviderInput;
  apiKeyOptions: ProviderInput["auth"]["apiKeyOptions"];
  remoteManaged: boolean;
}>();

const emit = defineEmits<{
  "copy-api-key": [];
  "select-api-key": [option: ProviderInput["auth"]["apiKeyOptions"][number]];
  "update-field": [field: ProviderAuthFieldDescriptor, value: string];
}>();

function fieldValue(field: ProviderAuthFieldDescriptor) {
  const value = props.draft.auth[field.field as keyof ProviderInput["auth"]];
  return typeof value === "string" ? value : "";
}

function shouldRenderField(field: ProviderAuthFieldDescriptor) {
  return field.showWhenEmpty || Boolean(fieldValue(field).trim());
}

function isApiKeyField(field: ProviderAuthFieldDescriptor) {
  return field.field === "apiKey";
}
</script>

<template>
  <template v-for="field in fields" :key="field.field">
    <a-form-item
      v-if="shouldRenderField(field)"
      class="provider-field"
      :class="{ 'provider-field-wide': field.wide }"
      :field="`auth.${field.field}`"
      :label="field.label"
      :required="requiredFields.includes(field.field)"
    >
      <div v-if="isApiKeyField(field)" class="input-action-row">
        <a-input-password
          :model-value="fieldValue(field)"
          :placeholder="field.placeholder"
          :readonly="field.readonly"
          allow-clear
          @update:model-value="emit('update-field', field, $event)"
        />
        <a-button :disabled="!draft.auth.apiKey.trim()" aria-label="复制 API Key" @click="emit('copy-api-key')">
          <template #icon><IconCopy /></template>
        </a-button>
      </div>
      <a-input-password
        v-else-if="field.secret"
        :model-value="fieldValue(field)"
        :placeholder="field.placeholder"
        :readonly="field.readonly"
        allow-clear
        @update:model-value="emit('update-field', field, $event)"
      />
      <a-input
        v-else
        :model-value="fieldValue(field)"
        :placeholder="field.placeholder"
        :readonly="field.readonly"
        allow-clear
        @update:model-value="emit('update-field', field, $event)"
      />
    </a-form-item>
  </template>
  <ProviderApiKeyPicker
    v-if="mode === 'apiKey' && apiKeyOptions.length > 0"
    class="provider-field-wide"
    :options="apiKeyOptions"
    :current-key="draft.auth.apiKey"
    :current-token-id="draft.auth.apiKeyTokenId"
    :remote-managed="remoteManaged"
    :selectable="apiKeyOptions.length > 1"
    @select="emit('select-api-key', $event)"
  />
</template>
