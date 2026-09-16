<script setup lang="ts">
import { computed, inject } from "vue";
import { CHECK_IN_CONTEXT } from "../../composables/useCheckInActions";
const props = defineProps<{ providerId: string; interactive: boolean }>();
const context = inject(CHECK_IN_CONTEXT);
const task = computed(() => context?.tasks.value.find((task) => task.providerId === props.providerId && !task.finished));
const busy = computed(() => context?.pending.value.some((key) => key.endsWith(`:${task.value?.runId}`)));
</script>
<template>
  <div v-if="task" class="provider-check-in-state" @click.stop @pointerdown.stop @keydown.enter.stop>
    <span :title="task.message">{{ task.message }}</span>
    <button v-if="interactive && task.canResume" type="button" :disabled="busy" @click="context?.resume(task)">{{ task.phase === 'waitingBrowser' ? '安装 / 继续' : '继续验证' }}</button>
    <button v-if="interactive && task.canCancel" type="button" :disabled="busy" @click="context?.cancel(task)">取消</button>
  </div>
</template>
<style scoped>
.provider-check-in-state { display: flex; align-items: center; gap: 8px; padding: 6px 0; font-size: 11px; color: var(--color-text-2); }
.provider-check-in-state span { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.provider-check-in-state button { border: 0; padding: 3px 0; background: transparent; color: rgb(var(--primary-6)); font: inherit; cursor: pointer; white-space: nowrap; }
.provider-check-in-state button:disabled { opacity: 0.5; cursor: default; }
</style>
