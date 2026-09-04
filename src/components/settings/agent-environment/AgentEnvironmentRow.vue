<script setup lang="ts">
import { computed } from "vue";
import {
  IconCheckCircle,
  IconDelete,
  IconMore,
  IconRefresh,
  IconSettings,
} from "@arco-design/web-vue/es/icon";
import AgentCliIcon from "../../AgentCliIcon.vue";
import AgentVersionStatus from "./AgentVersionStatus.vue";
import type {
  AgentHookActionKind,
  AgentHookHealthState,
  AgentHookInspection,
  AgentInstallation,
} from "../../../stores/provider-types";

const props = defineProps<{
  installations: AgentInstallation[];
  inspection: AgentHookInspection | null;
  busy: boolean;
  error: string | null;
}>();
const emit = defineEmits<{
  detail: [installationId: string];
  inspect: [];
  verify: [];
  repair: [];
  mutate: [mutation: "install" | "enable" | "disable" | "remove"];
}>();

const primary = computed(() => props.installations[0]);
const actionMap = computed(() => new Map((props.inspection?.actions ?? []).map((item) => [item.action, item])));
const hookStateLabels: Record<AgentHookHealthState, string> = {
  not_installed: "未安装",
  installed_untrusted: "待信任",
  installed_unverified: "待验证",
  healthy: "运行正常",
  disabled: "已停用",
  conflict: "配置冲突",
  helper_missing: "辅助程序缺失",
  spool_blocked: "事件存储不可用",
  unsupported: "暂不支持",
};
const hookStateTone: Record<AgentHookHealthState, string> = {
  not_installed: "muted",
  installed_untrusted: "warning",
  installed_unverified: "pending",
  healthy: "healthy",
  disabled: "muted",
  conflict: "warning",
  helper_missing: "warning",
  spool_blocked: "warning",
  unsupported: "muted",
};
const hookState = computed(() => props.inspection?.state ?? null);
const isAvailable = computed(() => props.installations.some((item) => item.availability === "available"));
const installAction = computed(() => actionMap.value.get("install"));
const enableAction = computed(() => actionMap.value.get("enable"));
const disableAction = computed(() => actionMap.value.get("disable"));
const repairAction = computed(() => actionMap.value.get("repair"));
const removeAction = computed(() => actionMap.value.get("remove"));
const healthAction = computed(() => actionMap.value.get("health"));
const verifyAction = computed(() => actionMap.value.get("verify"));
const hasSecondaryActions = computed(() => Boolean(
  verifyAction.value?.available || repairAction.value?.available || removeAction.value?.available,
));

function titleFor(action: AgentHookActionKind) {
  return actionMap.value.get(action)?.reason ?? undefined;
}
</script>

<template>
  <article class="agent-environment-row" :class="{ 'is-unavailable': !isAvailable }">
    <div class="agent-row-identity">
      <AgentCliIcon :kind="primary.agentKind" :size="28" :label="primary.label" :decorative="false" />
      <div class="agent-row-identity-copy">
        <strong>{{ primary.label }}</strong>
        <span :title="primary.executablePath || ''">
          {{ installations.length > 1 ? `${installations.length} 个安装实例` : primary.executablePath || "未找到可执行文件" }}
        </span>
      </div>
    </div>

    <div class="agent-row-version">
      <AgentVersionStatus :installation="primary" :checking="false" />
      <span v-if="primary.diagnostic" class="agent-row-diagnostic" :title="primary.diagnostic">{{ primary.diagnostic }}</span>
    </div>

    <div class="agent-row-hook">
      <span v-if="busy" class="agent-hook-row-state is-pending">处理中</span>
      <span v-else-if="hookState" class="agent-hook-row-state" :class="`is-${hookStateTone[hookState]}`">
        {{ hookStateLabels[hookState] }}
      </span>
      <span v-else class="agent-hook-row-state is-muted">等待读取</span>
      <a-switch
        v-if="inspection?.installed && (enableAction || disableAction)"
        size="small"
        :model-value="Boolean(inspection.enabled)"
        :loading="busy"
        :disabled="busy || !(inspection.enabled ? disableAction?.available : enableAction?.available)"
        :title="titleFor(inspection.enabled ? 'disable' : 'enable')"
        aria-label="启用或停用 BalanceHub Hook"
        @update:model-value="(value: string | number | boolean) => emit('mutate', value === true ? 'enable' : 'disable')"
      />
      <a-button
        v-else-if="installAction?.available"
        size="small"
        type="primary"
        :loading="busy"
        title="安装 BalanceHub Hook"
        @click="emit('mutate', 'install')"
      >安装 Hook</a-button>
      <span v-else-if="inspection?.actions.length" class="agent-row-action-reason" :title="installAction?.reason || inspection.actions.find((item) => !item.available)?.reason || undefined">
        {{ installAction?.reason || "不可操作" }}
      </span>
    </div>

    <div class="agent-row-actions">
      <a-button
        type="text"
        size="small"
        :disabled="busy || !healthAction?.available"
        :title="healthAction?.reason || '健康检查'"
        aria-label="健康检查"
        @click="emit('inspect')"
      >
        <template #icon><IconRefresh /></template>
      </a-button>
      <a-button type="text" size="small" title="查看 Agent 详情" aria-label="查看 Agent 详情" @click="emit('detail', primary.id)">
        详情
      </a-button>
      <a-dropdown v-if="hasSecondaryActions" trigger="click">
        <a-button type="text" size="small" :disabled="busy" title="更多 Hook 操作" aria-label="更多 Hook 操作">
          <template #icon><IconMore /></template>
        </a-button>
        <template #content>
          <a-doption v-if="verifyAction?.available" @click="emit('verify')">
            <IconCheckCircle /> 验证事件
          </a-doption>
          <a-doption v-if="repairAction?.available" @click="emit('repair')">
            <IconSettings /> 修复 Hook
          </a-doption>
          <a-doption v-if="removeAction?.available" class="is-danger" @click="emit('mutate', 'remove')">
            <IconDelete /> 删除 Hook
          </a-doption>
        </template>
      </a-dropdown>
    </div>
    <p v-if="error" class="agent-row-error">{{ error }}</p>
  </article>
</template>
