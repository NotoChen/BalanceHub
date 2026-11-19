<script setup lang="ts">
import { IconCopy } from "@arco-design/web-vue/es/icon";
import type {
  ProviderAuthFieldDescriptor,
  ProviderInput,
} from "../../stores/providers";

const props = defineProps<{
  fields: ProviderAuthFieldDescriptor[];
  requiredFields: string[];
  /// 当前认证方式仍缺的字段；由凭据助手同一份缺口结论传入。
  invalidFields?: string[];
  draft: ProviderInput;
}>();

const emit = defineEmits<{
  "copy-api-key": [];
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

function isInvalid(field: ProviderAuthFieldDescriptor) {
  return (props.invalidFields ?? []).includes(field.field);
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
      :data-credential-field="field.field"
      :validate-status="isInvalid(field) ? 'error' : undefined"
      :help="isInvalid(field) ? '还缺这个字段' : undefined"
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
</template>
