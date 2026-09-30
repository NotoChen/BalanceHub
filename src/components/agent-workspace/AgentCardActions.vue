<script setup lang="ts">
import { IconBook, IconPlayArrow, IconRefresh } from "@arco-design/web-vue/es/icon";
import CardIconButton from "../workspace-card/CardIconButton.vue";
import WorkspaceCardFooter from "../workspace-card/WorkspaceCardFooter.vue";
import WorkspaceCardActionGroup from "../workspace-card/WorkspaceCardActionGroup.vue";

defineProps<{
  label: string; canLaunch: boolean; showInstallationGuide: boolean; refreshing: boolean;
}>();
const emit = defineEmits<{
  installationGuide: []; launch: []; refresh: [];
}>();
</script>

<template>
  <WorkspaceCardFooter class="agent-card-footer" :label="`${label} 快捷操作`">
    <WorkspaceCardActionGroup label="Agent 工具">
      <CardIconButton tone="refresh" :title="refreshing ? '正在刷新' : '刷新'" :aria-label="`刷新 ${label}`" :disabled="refreshing" :aria-busy="refreshing || undefined" @click="emit('refresh')"><IconRefresh :class="{ 'agent-card-spinning': refreshing }" aria-hidden="true" /></CardIconButton>
    </WorkspaceCardActionGroup>
    <WorkspaceCardActionGroup label="启动 Agent">
      <CardIconButton v-if="showInstallationGuide" class="agent-card-launch" tone="launch" :aria-label="`查看 ${label} 官方安装说明`" title="打开官方安装说明" @click="emit('installationGuide')"><IconBook aria-hidden="true" /><span>安装说明</span></CardIconButton>
      <CardIconButton v-else class="agent-card-launch" tone="launch" :disabled="!canLaunch" :aria-label="`启动 ${label}`" :title="canLaunch ? '选择中转站并启动' : '当前安装没有可用的启动能力'" @click="emit('launch')"><IconPlayArrow aria-hidden="true" /><span>启动</span></CardIconButton>
    </WorkspaceCardActionGroup>
  </WorkspaceCardFooter>
</template>
