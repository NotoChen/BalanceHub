<script setup lang="ts">
import { computed } from "vue";
import { IconRefresh } from "@arco-design/web-vue/es/icon";
import type {
  AgentCliKind,
  AgentRuntimeSession,
  CliRuntimeSnapshot,
  Provider,
} from "../stores/providers";
import { isConfirmedAgentRuntimeSession } from "../utils/agent-runtime";
import AgentRuntimeCard from "./AgentRuntimeCard.vue";

const props = defineProps<{
  provider: Provider | null;
  cliKind: AgentCliKind | null;
  cliRuntime: CliRuntimeSnapshot;
  loading: boolean;
  instances: AgentRuntimeSession[];
  activatingId: string | null;
}>();

const emit = defineEmits<{
  refresh: [];
  activate: [instance: AgentRuntimeSession];
}>();

function agentLabel(kind: AgentCliKind) {
  return props.cliRuntime.agents.find((agent) => agent.kind === kind)?.label || kind;
}

const selectedCliLabel = computed(() => props.cliKind ? agentLabel(props.cliKind) : "Agent");
const confirmedCount = computed(() => props.instances.filter(isConfirmedAgentRuntimeSession).length);
const unknownCount = computed(() => props.instances.filter((session) => session.state === "unknown").length);
const summaryText = computed(() => {
  if (props.provider) return "个 " + selectedCliLabel.value + " 会话正在使用此中转站";
  return props.cliKind ? "个活动 " + selectedCliLabel.value + " 会话" : "个活动 Agent 会话";
});
</script>

<template>
  <div class="agent-runtime-content">
    <div class="agent-runtime-toolbar">
      <div class="agent-runtime-summary">
        <strong>{{ confirmedCount }}</strong>
        <span>{{ summaryText }}</span>
        <span v-if="unknownCount > 0" class="agent-runtime-summary-unknown">· 另有 {{ unknownCount }} 个状态未知</span>
      </div>
      <a-tooltip content="刷新会话状态">
        <a-button
          class="agent-runtime-refresh"
          shape="circle"
          :loading="loading"
          aria-label="刷新会话状态"
          @click="emit('refresh')"
        >
          <template #icon><IconRefresh /></template>
        </a-button>
      </a-tooltip>
    </div>

    <a-spin :loading="loading" class="agent-runtime-loading">
      <a-empty
        v-if="instances.length === 0"
        :description="'暂无正在使用的 ' + selectedCliLabel + ' 会话'"
      />
      <div v-else class="agent-runtime-list">
        <AgentRuntimeCard
          v-for="instance in instances"
          :key="instance.runtimeId"
          :instance="instance"
          :agent-label="agentLabel(instance.agentKind)"
          :activating="activatingId === instance.runtimeId"
          @activate="emit('activate', $event)"
        />
      </div>
    </a-spin>
  </div>
</template>
