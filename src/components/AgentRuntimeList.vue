<script setup lang="ts">
import { computed } from "vue";
import { Message } from "@arco-design/web-vue";
import {
  IconClockCircle,
  IconCopy,
  IconLaunch,
  IconRefresh,
} from "@arco-design/web-vue/es/icon";
import { Building2, Cpu, FolderOpen, Terminal } from "@lucide/vue";
import {
  type AgentCliKind,
  type CliRuntimeSnapshot,
  type Provider,
  type AgentRuntimeSession,
} from "../stores/providers";
import { copyText } from "../composables/useClipboard";
import AgentCliIcon from "./AgentCliIcon.vue";
import TerminalBrandIcon from "./TerminalBrandIcon.vue";
import {
  isConfirmedAgentRuntimeSession,
  runtimeOriginLabel,
  runtimeSessionTitle,
  runtimeStateLabel,
  runtimeTerminalLabel,
  runtimeWorkdirName,
} from "../utils/agent-runtime";

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
const selectedCliLabel = computed(() => (props.cliKind ? agentLabel(props.cliKind) : "Agent"));
const confirmedCount = computed(() => props.instances.filter(isConfirmedAgentRuntimeSession).length);
const unknownCount = computed(() => props.instances.filter((session) => session.state === "unknown").length);

const summaryText = computed(() => {
  if (props.provider) {
    return `个 ${selectedCliLabel.value} 会话正在使用此中转站`;
  }
  return props.cliKind ? `个活动 ${selectedCliLabel.value} 会话` : "个活动 Agent 会话";
});

function formatDateTime(value: number | null) {
  const timestamp = Number(value);
  if (!Number.isFinite(timestamp) || timestamp <= 0) {
    return "--";
  }
  const date = new Date(timestamp);
  const pad = (item: number) => String(item).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

async function copyWorkdir(instance: AgentRuntimeSession) {
  if (!instance.workdir) return;
  try {
    await copyText(instance.workdir);
    Message.success("已复制完整目录");
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}
</script>

<template>
    <div class="temporary-cli-modal-content">
      <div class="temporary-cli-toolbar">
        <div class="temporary-cli-summary">
          <strong>{{ confirmedCount }}</strong>
          <span>{{ summaryText }}</span>
          <span v-if="unknownCount > 0">· 另有 {{ unknownCount }} 个状态未知</span>
        </div>
        <a-tooltip content="刷新会话状态">
          <a-button
            class="temporary-cli-refresh"
            shape="circle"
            :loading="loading"
            aria-label="刷新会话状态"
            @click="emit('refresh')"
          >
            <template #icon><icon-refresh /></template>
          </a-button>
        </a-tooltip>
      </div>

      <a-spin :loading="loading" class="temporary-cli-loading">
        <a-empty
          v-if="instances.length === 0"
          :description="`暂无正在使用的 ${selectedCliLabel} 会话`"
        />
        <div v-else class="temporary-cli-list">
          <article
            v-for="instance in instances"
            :key="instance.runtimeId"
            class="temporary-cli-instance"
            :class="`temporary-cli-instance-${instance.state}`"
          >
            <header class="temporary-cli-instance-header">
              <div class="temporary-cli-runtime-pair">
                <div class="temporary-cli-runtime-item">
                  <span class="temporary-cli-agent-icon">
                    <AgentCliIcon :kind="instance.agentKind" :size="22" />
                  </span>
                  <span class="temporary-cli-runtime-copy">
                    <small>智能体</small>
                    <strong>{{ agentLabel(instance.agentKind) }}</strong>
                  </span>
                </div>
                <div class="temporary-cli-runtime-item">
                  <span class="temporary-cli-terminal-icon">
                    <TerminalBrandIcon
                      v-if="instance.terminal"
                      :kind="instance.terminal.kind"
                      :name="runtimeTerminalLabel(instance.terminal.kind)"
                      :size="22"
                    />
                    <Terminal v-else :size="22" :stroke-width="1.8" />
                  </span>
                  <span class="temporary-cli-runtime-copy">
                    <small>终端</small>
                    <strong>{{ instance.terminal ? runtimeTerminalLabel(instance.terminal.kind) : "终端未知" }}</strong>
                  </span>
                </div>
              </div>
              <span class="temporary-cli-status" :class="`temporary-cli-status-${instance.state}`">
                {{ runtimeStateLabel(instance.state) }}
              </span>
            </header>

            <div class="temporary-cli-session">
              <span class="temporary-cli-session-icon" aria-hidden="true">
                <Terminal :size="14" :stroke-width="1.8" />
              </span>
              <div>
                <small>会话</small>
                <strong :title="runtimeSessionTitle(instance)">{{ runtimeSessionTitle(instance) }}</strong>
              </div>
            </div>

            <div class="temporary-cli-source">
              <Building2 :size="14" :stroke-width="1.8" aria-hidden="true" />
              <span class="temporary-cli-source-provider" :title="runtimeOriginLabel(instance.origin)">
                {{ runtimeOriginLabel(instance.origin) }}
              </span>
              <span class="temporary-cli-source-separator" aria-hidden="true">·</span>
              <span class="temporary-cli-source-account" :title="instance.provider?.providerName || undefined">
                {{ instance.provider?.providerName || "中转站未知" }}
              </span>
              <span class="temporary-cli-source-separator" aria-hidden="true">·</span>
              <span class="temporary-cli-source-account" :title="instance.provider?.accountLabel || undefined">
                {{ instance.provider?.accountLabel || "账号未知" }}
              </span>
            </div>

            <div v-if="instance.model" class="temporary-cli-source">
              <Cpu :size="14" :stroke-width="1.8" aria-hidden="true" />
              <span class="temporary-cli-source-provider" :title="instance.model">{{ instance.model }}</span>
            </div>

            <div class="temporary-cli-workdir">
              <FolderOpen :size="17" :stroke-width="1.8" aria-hidden="true" />
              <div>
                <span>工作目录</span>
                <strong :title="instance.workdir || undefined">{{ runtimeWorkdirName(instance.workdir) }}</strong>
              </div>
              <a-tooltip content="复制完整目录">
                <button
                  type="button"
                  class="temporary-cli-copy"
                  aria-label="复制完整目录"
                  :disabled="!instance.workdir"
                  @click="copyWorkdir(instance)"
                >
                  <icon-copy />
                </button>
              </a-tooltip>
            </div>

            <dl class="temporary-cli-details">
              <div>
                <dt><icon-clock-circle /> 启动时间</dt>
                <dd>{{ formatDateTime(instance.startedAt) }}</dd>
              </div>
              <div>
                <dt><Cpu :size="13" :stroke-width="1.8" /> 进程 PID</dt>
                <dd>{{ instance.process?.pid ?? "--" }}</dd>
              </div>
            </dl>

            <footer class="temporary-cli-instance-actions">
              <a-tooltip
                :content="instance.actions.canActivateTerminal
                  ? '定位对应的终端窗口'
                  : '当前终端未提供可定位的窗口信息'"
              >
                <span class="temporary-cli-activate-action">
                  <a-button
                    type="primary"
                    size="small"
                    :disabled="!instance.actions.canActivateTerminal"
                    :loading="activatingId === instance.runtimeId"
                    @click="emit('activate', instance)"
                  >
                    <template #icon><icon-launch /></template>
                    定位窗口
                  </a-button>
                </span>
              </a-tooltip>
            </footer>
          </article>
        </div>
      </a-spin>
    </div>
</template>
