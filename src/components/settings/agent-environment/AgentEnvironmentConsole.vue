<script setup lang="ts">
import { computed } from "vue";
import { IconLoading, IconRefresh } from "@arco-design/web-vue/es/icon";
import AgentEnvironmentRow from "./AgentEnvironmentRow.vue";
import { agentHookTargetKey, type AgentHookTargetKey } from "../../../utils/agent-runtime";
import type {
  AgentEnvironmentInventory,
  AgentHookInspection,
  AgentInstallation,
  AgentRuntimeScope,
} from "../../../stores/provider-types";

type Row = {
  key: AgentHookTargetKey;
  scope: AgentRuntimeScope;
  installations: AgentInstallation[];
  inspection: AgentHookInspection | null;
};
const props = defineProps<{
  inventory: AgentEnvironmentInventory | null;
  loading: boolean;
  versionChecking: boolean;
  error: string | null;
  versionError: string | null;
  inspections: Partial<Record<AgentHookTargetKey, AgentHookInspection>>;
  isRowBusy: (targetKey: AgentHookTargetKey) => boolean;
  rowError: (targetKey: AgentHookTargetKey) => string | null;
  inspect: (agentKind: AgentInstallation["agentKind"], scope: AgentRuntimeScope) => void | Promise<unknown>;
  verify: (agentKind: AgentInstallation["agentKind"], scope: AgentRuntimeScope) => void | Promise<unknown>;
  repair: (agentKind: AgentInstallation["agentKind"], scope: AgentRuntimeScope) => void | Promise<unknown>;
  mutate: (agentKind: AgentInstallation["agentKind"], scope: AgentRuntimeScope, mutation: "install" | "enable" | "disable" | "remove") => void | Promise<unknown>;
}>();
const emit = defineEmits<{ detail: [id: string]; refresh: []; refreshVersions: [] }>();

const rows = computed<Row[]>(() => {
  const scope: AgentRuntimeScope = { kind: "native" };
  const groups = new Map<AgentHookTargetKey, AgentInstallation[]>();
  for (const installation of props.inventory?.installations ?? []) {
    const key = agentHookTargetKey(installation.agentKind, scope);
    groups.set(key, [...(groups.get(key) ?? []), installation]);
  }
  return [...groups.entries()].map(([key, installations]) => ({
    key,
    scope,
    installations,
    inspection: props.inspections[key] ?? null,
  }));
});

function action(
  agentKind: AgentInstallation["agentKind"],
  scope: AgentRuntimeScope,
  handler: (kind: AgentInstallation["agentKind"], runtimeScope: AgentRuntimeScope) => void | Promise<unknown>,
) {
  void handler(agentKind, scope);
}
</script>

<template>
  <div class="agent-environment-overview">
    <header class="agent-environment-toolbar">
      <div>
        <strong>Agent 环境</strong>
        <span v-if="inventory">
          {{ inventory.installations.filter((item) => item.availability === 'available').length }} 个可用 ·
          {{ rows.length }} 个运行目标 · {{ inventory.environment.displayName }}
        </span>
        <span v-else>读取本机可用的 Agent 安装</span>
      </div>
      <div class="agent-environment-toolbar-actions">
        <a-button size="small" :disabled="versionChecking" @click="emit('refreshVersions')">
          <template #icon><IconLoading v-if="versionChecking" class="agent-version-loading" /><IconRefresh v-else /></template>
          检查版本
        </a-button>
        <a-button size="small" type="text" :loading="loading" aria-label="刷新 Agent 环境" @click="emit('refresh')">
          <template #icon><IconRefresh /></template>
        </a-button>
      </div>
    </header>
    <div v-if="loading && !inventory" class="agent-environment-empty"><IconLoading class="agent-version-loading" /> 正在盘点 Agent 环境</div>
    <div v-else-if="error && !inventory" class="agent-environment-empty is-error">{{ error }}</div>
    <div v-else-if="rows.length === 0" class="agent-environment-empty">未注册 Agent CLI</div>
    <div v-else class="agent-environment-console" role="list" aria-label="Agent 环境列表">
      <div class="agent-environment-console-head" aria-hidden="true">
        <span>Agent</span><span>安装 / 版本</span><span>会话 Hook</span><span>操作</span>
      </div>
      <AgentEnvironmentRow
        v-for="row in rows"
        :key="row.key"
        role="listitem"
        :installations="row.installations"
        :inspection="row.inspection"
        :busy="props.isRowBusy(row.key)"
        :error="props.rowError(row.key)"
        @detail="emit('detail', $event)"
        @inspect="action(row.installations[0].agentKind, row.scope, props.inspect)"
        @verify="action(row.installations[0].agentKind, row.scope, props.verify)"
        @repair="action(row.installations[0].agentKind, row.scope, props.repair)"
        @mutate="props.mutate(row.installations[0].agentKind, row.scope, $event)"
      />
    </div>
    <p v-if="error && inventory" class="agent-environment-stale-error">刷新失败，保留上次成功结果：{{ error }}</p>
    <p v-if="versionError && inventory" class="agent-environment-stale-error">版本检查失败，保留上次成功结果：{{ versionError }}</p>
  </div>
</template>
