<script setup lang="ts">
import { computed } from "vue";
import {
  IconClockCircle,
  IconCode,
  IconLaunch,
  IconRedo,
  IconRefresh,
} from "@arco-design/web-vue/es/icon";
import type { Provider, TemporaryCliInstance } from "../stores/providers";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  loading: boolean;
  instances: TemporaryCliInstance[];
  activatingId: string | null;
  relaunchingId: string | null;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  refresh: [];
  activate: [instance: TemporaryCliInstance];
  relaunch: [instance: TemporaryCliInstance];
}>();

const title = computed(() =>
  props.provider ? `${props.provider.identity.name} · 临时 CLI` : "临时 CLI",
);

const activeCount = computed(() =>
  props.instances.filter((instance) => instance.status !== "exited").length,
);

function cliLabel(kind: TemporaryCliInstance["cliKind"]) {
  return kind === "codex" ? "Codex" : "Claude Code";
}

function statusLabel(status: TemporaryCliInstance["status"]) {
  if (status === "starting") return "正在启动";
  if (status === "running") return "运行中";
  return "已退出";
}

function terminalLabel(kind: TemporaryCliInstance["terminalKind"]) {
  const labels: Record<TemporaryCliInstance["terminalKind"], string> = {
    auto: "自动选择",
    systemDefault: "系统默认",
    terminal: "Terminal",
    iTerm2: "iTerm2",
    warp: "Warp",
    wezTerm: "WezTerm",
    ghostty: "Ghostty",
    kitty: "Kitty",
    alacritty: "Alacritty",
    kaku: "Kaku",
    windowsTerminal: "Windows Terminal",
    commandPrompt: "命令提示符",
    powerShell: "PowerShell",
    custom: "自定义终端",
  };
  return labels[kind];
}

function formatDateTime(value: string | null) {
  const timestamp = Number(value);
  if (!Number.isFinite(timestamp) || timestamp <= 0) {
    return "--";
  }
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(timestamp));
}
</script>

<template>
  <a-drawer
    :visible="visible"
    :width="520"
    :title="title"
    :footer="false"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <div class="temporary-cli-drawer">
      <div class="temporary-cli-toolbar">
        <div class="temporary-cli-summary">
          <strong>{{ activeCount }}</strong>
          <span>个实例运行中</span>
          <small>共 {{ instances.length }} 条记录</small>
        </div>
        <a-tooltip content="刷新实例状态">
          <a-button
            shape="circle"
            :loading="loading"
            aria-label="刷新实例状态"
            @click="emit('refresh')"
          >
            <template #icon><icon-refresh /></template>
          </a-button>
        </a-tooltip>
      </div>

      <a-spin :loading="loading">
        <a-empty v-if="instances.length === 0" description="暂无临时 CLI 实例" />
        <div v-else class="temporary-cli-list">
          <section
            v-for="instance in instances"
            :key="instance.id"
            class="temporary-cli-instance"
            :class="`temporary-cli-instance-${instance.status}`"
          >
            <div class="temporary-cli-instance-header">
              <div class="temporary-cli-instance-name">
                <span class="temporary-cli-kind-icon"><icon-code /></span>
                <div>
                  <strong>{{ cliLabel(instance.cliKind) }}</strong>
                  <span>{{ terminalLabel(instance.terminalKind) }}</span>
                </div>
              </div>
              <span class="temporary-cli-status" :class="`temporary-cli-status-${instance.status}`">
                {{ statusLabel(instance.status) }}
              </span>
            </div>

            <dl class="temporary-cli-details">
              <div class="temporary-cli-workdir">
                <dt>工作目录</dt>
                <dd :title="instance.workdir">{{ instance.workdir }}</dd>
              </div>
              <div>
                <dt><icon-clock-circle /> 启动时间</dt>
                <dd>{{ formatDateTime(instance.startedAt) }}</dd>
              </div>
              <div>
                <dt>{{ instance.status === "exited" ? "退出时间" : "进程" }}</dt>
                <dd>
                  {{ instance.status === "exited"
                    ? formatDateTime(instance.endedAt)
                    : instance.pid ? `PID ${instance.pid}` : "等待终端启动" }}
                </dd>
              </div>
              <div v-if="instance.status === 'exited' && instance.exitCode !== null">
                <dt>退出码</dt>
                <dd>{{ instance.exitCode }}</dd>
              </div>
            </dl>

            <div class="temporary-cli-instance-actions">
              <a-tooltip
                v-if="instance.status !== 'exited'"
                :content="instance.canActivate
                  ? '打开对应的终端窗口'
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
                    打开窗口
                  </a-button>
                </span>
              </a-tooltip>
              <a-button
                size="small"
                :loading="relaunchingId === instance.id"
                @click="emit('relaunch', instance)"
              >
                <template #icon><icon-redo /></template>
                重新启动
              </a-button>
            </div>
          </section>
        </div>
      </a-spin>
    </div>
  </a-drawer>
</template>
