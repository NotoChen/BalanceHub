<script setup lang="ts">
import { computed } from "vue";
import { Message } from "@arco-design/web-vue";
import {
  IconClockCircle,
  IconCopy,
  IconLaunch,
  IconRefresh,
} from "@arco-design/web-vue/es/icon";
import { Cpu, FolderOpen, Terminal } from "@lucide/vue";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import {
  type AgentCliKind,
  type Provider,
  type TemporaryCliInstance,
} from "../stores/providers";
import { agentCliLabel } from "../utils/cli-environment";
import { copyText } from "../composables/useClipboard";
import AgentCliIcon from "./AgentCliIcon.vue";
import TerminalBrandIcon from "./TerminalBrandIcon.vue";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  cliKind: AgentCliKind | null;
  loading: boolean;
  instances: TemporaryCliInstance[];
  activatingId: string | null;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  refresh: [];
  activate: [instance: TemporaryCliInstance];
}>();
const store = useCliRuntimeStore();

const title = computed(() => props.provider?.identity.name || "活动 CLI");
const selectedCliLabel = computed(() => (props.cliKind ? cliLabel(props.cliKind) : "CLI"));

function cliLabel(kind: TemporaryCliInstance["cliKind"]) {
  return agentCliLabel(store.cliEnvironmentProbe, kind);
}

function statusLabel(status: TemporaryCliInstance["status"]) {
  if (status === "starting") return "正在启动";
  return "运行中";
}

function directoryName(value: string) {
  const path = value.trim();
  if (!path) return "--";
  const normalized = path.replace(/[\\/]+$/, "");
  if (!normalized) return path;
  const segments = normalized.split(/[\\/]/).filter(Boolean);
  return segments[segments.length - 1] || normalized;
}

function formatDateTime(value: string | null) {
  const timestamp = Number(value);
  if (!Number.isFinite(timestamp) || timestamp <= 0) {
    return "--";
  }
  const date = new Date(timestamp);
  const pad = (item: number) => String(item).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

async function copyWorkdir(instance: TemporaryCliInstance) {
  try {
    await copyText(instance.workdir);
    Message.success("已复制完整目录");
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal temporary-cli-modal"
    :footer="false"
    :width="780"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title temporary-cli-modal-title">
        <span class="surface-modal-title-icon"><Terminal :size="18" :stroke-width="1.8" /></span>
        <span class="surface-modal-title-copy">
          <span>活动 {{ selectedCliLabel }}</span>
          <strong>{{ title }}</strong>
        </span>
        <span class="surface-modal-title-meta" :class="{ ready: instances.length > 0 }">
          <i aria-hidden="true"></i>
          {{ instances.length }} 个活动实例
        </span>
      </div>
    </template>

    <div class="temporary-cli-modal-content">
      <div class="temporary-cli-toolbar">
        <div class="temporary-cli-summary">
          <strong>{{ instances.length }}</strong>
          <span>个临时 {{ selectedCliLabel }} 正在使用此中转站</span>
        </div>
        <a-tooltip content="刷新实例状态">
          <a-button
            class="temporary-cli-refresh"
            shape="circle"
            :loading="loading"
            aria-label="刷新实例状态"
            @click="emit('refresh')"
          >
            <template #icon><icon-refresh /></template>
          </a-button>
        </a-tooltip>
      </div>

      <a-spin :loading="loading" class="temporary-cli-loading">
        <a-empty
          v-if="instances.length === 0"
          :description="`暂无正在使用的临时 ${selectedCliLabel}`"
        />
        <div v-else class="temporary-cli-list">
          <article
            v-for="instance in instances"
            :key="instance.id"
            class="temporary-cli-instance"
            :class="`temporary-cli-instance-${instance.status}`"
          >
            <header class="temporary-cli-instance-header">
              <div class="temporary-cli-runtime-pair">
                <div class="temporary-cli-runtime-item">
                  <span class="temporary-cli-agent-icon">
                    <AgentCliIcon :kind="instance.cliKind" :size="22" />
                  </span>
                  <span class="temporary-cli-runtime-copy">
                    <small>智能体</small>
                    <strong>{{ cliLabel(instance.cliKind) }}</strong>
                  </span>
                </div>
                <div class="temporary-cli-runtime-item">
                  <span class="temporary-cli-terminal-icon">
                    <TerminalBrandIcon
                      :kind="instance.terminalKind"
                      :name="instance.terminalName"
                      :size="22"
                    />
                  </span>
                  <span class="temporary-cli-runtime-copy">
                    <small>终端</small>
                    <strong>{{ instance.terminalName }}</strong>
                  </span>
                </div>
              </div>
              <span class="temporary-cli-status" :class="`temporary-cli-status-${instance.status}`">
                {{ statusLabel(instance.status) }}
              </span>
            </header>

            <div class="temporary-cli-workdir">
              <FolderOpen :size="17" :stroke-width="1.8" aria-hidden="true" />
              <div>
                <span>工作目录</span>
                <strong :title="instance.workdir">{{ directoryName(instance.workdir) }}</strong>
              </div>
              <a-tooltip content="复制完整目录">
                <button
                  type="button"
                  class="temporary-cli-copy"
                  aria-label="复制完整目录"
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
                <dd>{{ instance.pid ?? "等待终端启动" }}</dd>
              </div>
            </dl>

            <footer class="temporary-cli-instance-actions">
              <a-tooltip
                :content="instance.canActivate
                  ? '定位对应的终端窗口'
                  : '当前终端未提供可定位的窗口信息'"
              >
                <span class="temporary-cli-activate-action">
                  <a-button
                    type="primary"
                    size="small"
                    :disabled="!instance.canActivate"
                    :loading="activatingId === instance.id"
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
  </a-modal>
</template>
