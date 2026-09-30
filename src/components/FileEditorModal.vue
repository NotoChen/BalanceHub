<script setup lang="ts">
import { ref, watch } from "vue";
import { Maximize2, Minimize2 } from "@lucide/vue";
import "../styles/modules/file-editor-modal.css";
const props = defineProps<{ visible: boolean; title?: string; fill?: boolean }>();
const emit = defineEmits<{ close: [] }>();
const maximized = ref(false);
watch(() => props.visible, (visible) => { if (!visible) maximized.value = false; });
</script>

<template>
  <a-modal :visible="visible" width="min(1440px, calc(100vw - 48px))" :fullscreen="maximized" :modal-class="['surface-modal', 'file-editor-modal', { 'file-editor-modal-fill': fill }]" title-align="start" :footer="false" closable mask-closable esc-to-close unmount-on-close @cancel="emit('close')">
    <template #title>
      <div class="file-editor-modal-title"><div><slot name="title">{{ title }}</slot></div><button type="button" :aria-label="maximized ? '还原窗口' : '最大化编辑窗口'" :title="maximized ? '还原窗口' : '最大化编辑窗口'" @click="maximized = !maximized"><Minimize2 v-if="maximized" :size="17" /><Maximize2 v-else :size="17" /></button></div>
    </template>
    <slot />
  </a-modal>
</template>
