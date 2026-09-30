<script setup lang="ts">
import type { AgentHookChangeKind, AgentHookMutation, AgentHookPlan } from "../../../stores/provider-types";
import ContentChange from "../../ContentChange.vue";

defineProps<{
  visible: boolean;
  plan: AgentHookPlan | null;
  canApply: boolean;
}>();
const emit = defineEmits<{ close: []; confirm: [] }>();

const mutationLabels: Record<AgentHookMutation, string> = {
  install: "安装",
  remove: "删除",
  enable: "启用",
  disable: "停用",
};
const changeKindLabels: Record<AgentHookChangeKind, string> = { add: "新增", remove: "删除", keep: "保留" };
</script>

<template>
  <a-modal
    :visible="visible"
    width="min(960px, calc(100vw - 32px))"
    modal-class="surface-modal agent-hook-plan-modal"
    title-align="start"
    closable
    mask-closable
    esc-to-close
    unmount-on-close
    :footer="false"
    @update:visible="(value: boolean) => !value && emit('close')"
  >
    <template #title>{{ plan ? `${mutationLabels[plan.mutation]} Hook` : "Hook 变更计划" }}</template>
    <div v-if="plan" class="agent-hook-plan">
      <p>{{ plan.summary }}</p>
      <code :title="plan.configPath">{{ plan.configPath }}</code>
      <ContentChange v-for="(change, index) in plan.contentChanges" :key="index" v-bind="change" />
      <details v-if="plan.changes.length"><summary>受影响事件 · {{ plan.changes.length }}</summary><div class="agent-hook-change-list">
        <div v-for="change in plan.changes" :key="change.structuralIdentity" class="agent-hook-change">
          <span :class="`is-${change.kind}`">{{ changeKindLabels[change.kind] }}</span>
          <div><strong>{{ change.eventName }}</strong><small :title="change.structuralIdentity">{{ change.structuralIdentity }}</small></div>
        </div>
      </div></details>
      <a-alert v-if="plan.conflict" type="warning" show-icon>当前配置存在冲突，BalanceHub 不会覆盖文件。</a-alert>
      <a-alert v-else-if="!plan.supported" type="info" show-icon>当前 Agent 不满足此操作条件，配置不会被修改。</a-alert>
      <div class="agent-hook-plan-actions">
        <a-button @click="emit('close')">取消</a-button>
        <a-button type="primary" :disabled="!canApply" @click="emit('confirm')">确认应用</a-button>
      </div>
    </div>
  </a-modal>
</template>
