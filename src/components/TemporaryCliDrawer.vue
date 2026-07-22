<script setup lang="ts">
import { computed } from "vue";
import {
  IconClockCircle,
  IconLaunch,
  IconRefresh,
} from "@arco-design/web-vue/es/icon";
import type { Provider, TemporaryCliInstance } from "../stores/providers";
import BrandIcon, { type BrandIconName } from "./BrandIcon.vue";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  loading: boolean;
  instances: TemporaryCliInstance[];
  activatingId: string | null;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  refresh: [];
  activate: [instance: TemporaryCliInstance];
}>();

const title = computed(() =>
  props.provider ? `${props.provider.identity.name} · 活动 CLI` : "活动 CLI",
);

function cliLabel(kind: TemporaryCliInstance["cliKind"]) {
  return kind === "codex" ? "Codex" : "Claude Code";
}

function cliBrand(kind: TemporaryCliInstance["cliKind"]): BrandIconName {
  return kind === "codex" ? "codex" : "claude";
}

function statusLabel(status: TemporaryCliInstance["status"]) {
  if (status === "starting") return "正在启动";
  return "运行中";
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
    class="surface-drawer temporary-cli-surface"
    body-class="temporary-cli-drawer-body"
    :width="520"
    :title="title"
    :footer="false"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <div class="temporary-cli-drawer">
      <div class="temporary-cli-toolbar">
        <div class="temporary-cli-summary">
          <strong>{{ instances.length }}</strong>
          <span>个 CLI 正在使用</span>
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
        <a-empty v-if="instances.length === 0" description="暂无正在使用的 CLI" />
        <div v-else class="temporary-cli-list">
          <section
            v-for="instance in instances"
            :key="instance.id"
            class="temporary-cli-instance"
            :class="`temporary-cli-instance-${instance.status}`"
          >
            <div class="temporary-cli-instance-header">
              <div class="temporary-cli-instance-name">
                <span class="temporary-cli-kind-icon">
                  <BrandIcon :brand="cliBrand(instance.cliKind)" :size="20" />
                </span>
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
                <dt>进程</dt>
                <dd>
                  {{ instance.pid ? `PID ${instance.pid}` : "等待终端启动" }}
                </dd>
              </div>
            </dl>

            <div class="temporary-cli-instance-actions">
              <a-tooltip
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
            </div>
          </section>
        </div>
      </a-spin>
    </div>
  </a-drawer>
</template>
