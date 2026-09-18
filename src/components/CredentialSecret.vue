<script setup lang="ts">
import { onUnmounted, shallowRef, watch } from "vue";
import { IconCopy, IconEye, IconEyeInvisible } from "@arco-design/web-vue/es/icon";
import { copyText } from "../composables/useClipboard";
import { createSecretController, type SecretView } from "../utils/credential-secret";
const props = defineProps<{ scope: string; label: string; read: () => Promise<string> }>();
const state = shallowRef<SecretView>({ value: "", revealed: false, pending: false, copied: false, error: "" });
const controller = createSecretController(() => props.read(), copyText, (value) => { state.value = value; });
watch(() => props.scope, controller.reset);
onUnmounted(controller.reset);
</script>

<template>
  <div class="credential-secret">
    <div class="credential-secret-controls">
      <span v-if="!state.revealed" class="credential-mask" aria-label="凭据已隐藏">••••••••••••••••</span>
      <a-button size="mini" :loading="state.pending" :aria-label="`${state.revealed ? '隐藏' : '显示'}${label}`" @click="controller.reveal">
        <template #icon><IconEyeInvisible v-if="state.revealed" /><IconEye v-else /></template>{{ state.revealed ? '隐藏' : '显示' }}
      </a-button>
      <a-button size="mini" :disabled="state.pending" :aria-label="`复制${label}`" @click="controller.copy">
        <template #icon><IconCopy /></template>{{ state.copied ? '已复制' : '复制' }}
      </a-button>
    </div>
    <pre v-if="state.revealed" class="credential-plaintext">{{ state.value }}</pre>
    <span v-if="state.error" class="credential-secret-error">{{ state.error }}</span>
  </div>
</template>

<style scoped>
.credential-secret { display: grid; gap: 7px; min-width: 0; }
.credential-secret-controls { display: flex; align-items: center; gap: 8px; }
.credential-mask { flex: 1; color: var(--color-text-3); letter-spacing: 2px; }
.credential-plaintext { margin: 0; padding: 10px; background: var(--color-fill-2); border-radius: 6px; white-space: pre-wrap; overflow-wrap: anywhere; font: 12px/1.7 ui-monospace, monospace; max-height: 180px; overflow-y: auto; user-select: text; }
.credential-secret-error { color: rgb(var(--danger-6)); font-size: 12px; }
</style>
