<script setup lang="ts">
import { computed, inject, onUnmounted, ref, watch } from "vue";
import { CalendarCheck2, CircleAlert, LoaderCircle } from "@lucide/vue";
import { CHECK_IN_CONTEXT } from "../../composables/useCheckInActions";
import { checkInResumeLabel } from "../../utils/check-in-tasks";
import CardIconButton from "../workspace-card/CardIconButton.vue";

const props = defineProps<{ providerId: string; checkedInToday: boolean; checkingIn: boolean }>();
const emit = defineEmits<{ checkIn: []; interaction: [active: boolean] }>();
const context = inject(CHECK_IN_CONTEXT);
const task = computed(() => context?.tasks.value.find((item) => item.providerId === props.providerId && !item.finished));
const pending = computed(() => context?.pending.value.some((key) => key.endsWith(`:${task.value?.runId}`)) ?? false);
const visible = ref(false);
const label = computed(() => task.value?.canResume
  ? checkInResumeLabel(task.value)
  : task.value ? "查看签到进度" : props.checkingIn ? "签到中" : props.checkedInToday ? "今日已签到" : "签到");
watch(visible, (value) => emit("interaction", value));
watch(() => task.value?.runId, () => { visible.value = false; });
onUnmounted(() => emit("interaction", false));

function resume() {
  const current = task.value;
  if (!current || pending.value) return;
  visible.value = false;
  void context?.resume(current);
}

function showWindow() {
  const current = task.value;
  if (!current || pending.value) return;
  visible.value = false;
  void context?.showWindow(current);
}
</script>

<template>
  <a-popover v-if="task" v-model:popup-visible="visible" trigger="click" position="top" content-class="provider-check-in-popover">
    <CardIconButton :tone="task.canResume ? 'warning' : 'success'" :title="label" :aria-label="label" :aria-busy="!task.canResume" @pointerdown.stop>
      <CircleAlert v-if="task.canResume" :size="15" :stroke-width="1.9" />
      <LoaderCircle v-else class="spinning" :size="15" :stroke-width="1.9" />
    </CardIconButton>
    <template #content>
      <div class="provider-check-in-detail" @pointerdown.stop @click.stop>
        <strong>{{ task.canResume ? label : '签到进度' }}</strong>
        <p role="status">{{ task.message }}</p>
        <div class="provider-check-in-controls">
          <a-button v-if="task.canShowWindow" size="small" type="primary" :disabled="pending" @click="showWindow">显示签到窗口</a-button>
          <a-button v-if="task.canResume" size="small" type="primary" :disabled="pending" @click="resume">{{ label }}</a-button>
          <a-button v-if="task.canCancel" size="small" :disabled="pending" @click="context?.cancel(task)">取消签到</a-button>
        </div>
      </div>
    </template>
  </a-popover>
  <CardIconButton v-else tone="success" :disabled="checkedInToday || checkingIn" :aria-busy="checkingIn" :title="label" :aria-label="label" @click="emit('checkIn')" @pointerdown.stop>
    <LoaderCircle v-if="checkingIn" class="spinning" :size="15" :stroke-width="1.9" />
    <CalendarCheck2 v-else :size="15" :stroke-width="1.9" />
  </CardIconButton>
</template>

<style scoped>
.provider-check-in-detail { width: 248px; max-width: calc(100vw - 64px); color: var(--color-text-1); }
.provider-check-in-detail strong { font-size: 13px; }
.provider-check-in-detail p { margin: 8px 0 12px; color: var(--color-text-2); font-size: 12px; line-height: 1.7; overflow-wrap: anywhere; }
.provider-check-in-controls { display: flex; gap: 8px; }
</style>
