<script setup lang="ts">
import { computed } from "vue";
import { useAgentMaintenanceStore } from "../../stores/agent-maintenance";
const props = defineProps<{ id: string }>();
const maintenance = useAgentMaintenanceStore();
const operation = computed(() => maintenance.operations[props.id]);
const labels = {
  pending: "等待处理",
  running: "处理中",
  completed: "已完成",
  skipped: "已跳过",
  failed: "未完成",
  canceled: "已取消",
};
</script>
<template>
  <section v-if="operation" class="agent-operation-details">
    <header class="agent-section-toolbar">
      <strong>{{ operation.title }}</strong
      ><a-button
        v-if="operation.canCancel"
        size="small"
        :loading="maintenance.canceling[id]"
        @click="maintenance.cancel(id)"
        >取消剩余操作</a-button
      ><a-button
        v-else-if="operation.phase !== 'completed'"
        size="small"
        @click="maintenance.recover"
        >刷新状态</a-button
      >
    </header>
    <p role="status">
      {{ operation.message }} · {{ operation.completed }} /
      {{ operation.total }}
    </p>
    <a-progress
      :percent="operation.total ? operation.completed / operation.total : 0"
      :show-text="false"
    />
    <p v-if="maintenance.errors[id]" class="agent-workspace-error">
      {{ maintenance.errors[id] }}
    </p>
    <article
      v-for="item in operation.items"
      :key="item.id"
      class="agent-operation-target"
    >
      <strong>{{ item.label }}</strong
      ><span>{{ labels[item.state] }}</span>
      <p
        v-if="item.message"
        :class="{ 'agent-workspace-error': item.state === 'failed' }"
      >
        {{ item.message }}
      </p>
    </article>
  </section>
  <p v-else class="agent-workspace-note">
    正在读取任务状态。<a-button type="text" @click="maintenance.recover"
      >刷新</a-button
    >
  </p>
</template>
