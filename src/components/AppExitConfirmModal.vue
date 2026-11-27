<script setup lang="ts">
import { computed } from "vue";
import { CircleAlert, LogOut, X } from "@lucide/vue";
import type { BackgroundTask } from "../composables/useBackgroundTaskCenter";
import { exitTaskSummary } from "../utils/app-exit";
import "../styles/modules/app-exit-confirm.css";

const props = withDefaults(
  defineProps<{
    visible: boolean;
    tasks: BackgroundTask[];
    /// 设置保存队列仍停在 pending / saving / error 时，退出会丢掉这次修改。
    settingsPending?: boolean;
  }>(),
  { tasks: () => [], settingsPending: false },
);

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  confirm: [];
  cancel: [];
}>();

const summary = computed(() => {
  if (props.tasks.length > 0 && props.settingsPending) {
    return `${exitTaskSummary(props.tasks)} · 有设置尚未保存`;
  }
  if (props.tasks.length > 0) return exitTaskSummary(props.tasks);
  return props.settingsPending ? "有设置修改尚未保存" : "当前没有运行中的任务";
});
const visibleTasks = computed(() => props.tasks.slice(0, 6));
const hiddenTaskCount = computed(() => Math.max(0, props.tasks.length - 6));

function cancel() {
  emit("cancel");
  emit("update:visible", false);
}
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal app-exit-confirm-modal"
    :footer="false"
    :width="520"
    @update:visible="(value) => !value && cancel()"
  >
    <template #title>
      <div class="surface-modal-title app-exit-confirm-title">
        <span class="surface-modal-title-icon"><CircleAlert :size="18" :stroke-width="1.9" /></span>
        <span class="surface-modal-title-copy">
          <strong>退出 BalanceHub？</strong>
          <small>{{ summary }}</small>
        </span>
      </div>
    </template>

    <div class="app-exit-confirm-body">
      <p v-if="tasks.length > 0" class="app-exit-confirm-lead">
        退出会中断下面这些任务；已经完成的站点结果会保留，未完成的部分需要重新发起。
      </p>
      <ul v-if="visibleTasks.length > 0" class="app-exit-confirm-tasks" aria-label="运行中的任务">
        <li v-for="task in visibleTasks" :key="task.id">
          <span class="app-exit-confirm-task-title">{{ task.title }}</span>
          <span v-if="task.detail" class="app-exit-confirm-task-detail">{{ task.detail }}</span>
        </li>
      </ul>
      <p v-if="settingsPending" class="app-exit-confirm-lead app-exit-confirm-settings">
        应用设置还有修改没有保存；现在退出，这次修改会丢失。
      </p>
      <p v-if="hiddenTaskCount > 0" class="app-exit-confirm-more">
        还有 {{ hiddenTaskCount }} 个任务未展开
      </p>
    </div>

    <footer class="app-exit-confirm-footer">
      <a-button @click="cancel">
        <template #icon><X :size="15" /></template>
        继续运行
      </a-button>
      <a-button type="primary" status="danger" @click="emit('confirm')">
        <template #icon><LogOut :size="15" /></template>
        仍然退出
      </a-button>
    </footer>
  </a-modal>
</template>
