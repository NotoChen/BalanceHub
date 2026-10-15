<script setup lang="ts">
import { computed } from "vue";
import { Message } from "@arco-design/web-vue";
import { IconCopy, IconLaunch, IconLoading } from "@arco-design/web-vue/es/icon";
import { Building2, Cpu, FolderOpen, Terminal } from "@lucide/vue";
import type { AgentRuntimeSession } from "../stores/provider-types";
import { copyText } from "../composables/useClipboard";
import {
  runtimeOriginLabel,
  runtimeSessionTitle,
  runtimeStateLabel,
  runtimeTerminalLabel,
  runtimeWorkdirName,
} from "../utils/agent-runtime";
import { sessionTime } from "../utils/agent-session-display";
import AgentCliIcon from "./AgentCliIcon.vue";
import TerminalBrandIcon from "./TerminalBrandIcon.vue";
import CardIconButton from "./workspace-card/CardIconButton.vue";
import WorkspaceCard from "./workspace-card/WorkspaceCard.vue";
import WorkspaceCardFooter from "./workspace-card/WorkspaceCardFooter.vue";

const props = defineProps<{
  instance: AgentRuntimeSession;
  agentLabel: string;
  activating: boolean;
}>();
const emit = defineEmits<{ activate: [instance: AgentRuntimeSession] }>();

const title = computed(() => runtimeSessionTitle(props.instance));
const terminalLabel = computed(() => runtimeTerminalLabel(props.instance.terminal?.kind));
const source = computed(() => {
  const provider = props.instance.provider;
  const providerLabel = [provider?.providerName.trim(), provider?.accountLabel.trim()]
    .filter(Boolean).join(" · ");
  const origin = runtimeOriginLabel(props.instance.origin);
  return {
    hasProvider: Boolean(providerLabel),
    label: providerLabel || origin,
    description: providerLabel ? origin + " · " + providerLabel : origin + "；未记录中转站与账号",
  };
});
const startedAt = computed(() => {
  const timestamp = props.instance.startedAt;
  if (timestamp === null || !Number.isFinite(timestamp) || timestamp <= 0) return null;
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime())) return null;
  const iso = date.toISOString();
  return {
    iso,
    label: date.toLocaleString("zh-CN", {
      month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hour12: false,
    }),
    fullLabel: sessionTime(iso),
  };
});
const activationHint = computed(() => props.instance.actions.canActivateTerminal
  ? "定位对应的终端窗口" : "当前终端未提供可定位的窗口信息");

async function copyWorkdir() {
  if (!props.instance.workdir) return;
  try {
    await copyText(props.instance.workdir);
    Message.success("已复制完整目录");
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}
</script>

<template>
  <WorkspaceCard
    class="agent-runtime-card"
    :class="'agent-runtime-card-' + instance.state"
    :fixed-height="false"
    :aria-label="agentLabel + '：' + title"
  >
    <template #header>
      <header class="agent-runtime-card-header">
        <div class="agent-runtime-agent">
          <AgentCliIcon :kind="instance.agentKind" :size="22" />
          <strong :title="agentLabel">{{ agentLabel }}</strong>
        </div>
        <span class="agent-runtime-terminal" :title="terminalLabel">
          <TerminalBrandIcon
            v-if="instance.terminal"
            :kind="instance.terminal.kind"
            :name="terminalLabel"
            :size="14"
            aria-hidden="true"
          />
          <Terminal v-else :size="14" :stroke-width="1.8" aria-hidden="true" />
          <span>{{ terminalLabel }}</span>
        </span>
        <span class="agent-runtime-status">
          <i aria-hidden="true"></i>{{ runtimeStateLabel(instance.state) }}
        </span>
      </header>
    </template>

    <h3 class="agent-runtime-title" :title="title">{{ title }}</h3>

    <dl class="agent-runtime-facts">
      <div class="agent-runtime-fact agent-runtime-directory">
        <dt aria-label="工作目录"><FolderOpen :size="15" :stroke-width="1.8" aria-hidden="true" /></dt>
        <dd>
          <span :title="instance.workdir || undefined">{{ runtimeWorkdirName(instance.workdir) }}</span>
          <CardIconButton
            v-if="instance.workdir"
            class="agent-runtime-copy"
            tone="copy"
            title="复制完整目录"
            aria-label="复制完整目录"
            @click="copyWorkdir"
          ><IconCopy aria-hidden="true" /></CardIconButton>
        </dd>
      </div>
      <div v-if="instance.model" class="agent-runtime-fact">
        <dt aria-label="模型"><Cpu :size="14" :stroke-width="1.8" aria-hidden="true" /></dt>
        <dd><span :title="instance.model">{{ instance.model }}</span></dd>
      </div>
      <div class="agent-runtime-fact agent-runtime-source" :title="source.description">
        <dt :aria-label="source.hasProvider ? '中转站与账号' : '启动来源'">
          <Building2 v-if="source.hasProvider" :size="14" :stroke-width="1.8" aria-hidden="true" />
          <Terminal v-else :size="14" :stroke-width="1.8" aria-hidden="true" />
        </dt>
        <dd><span>{{ source.label }}</span></dd>
      </div>
    </dl>

    <WorkspaceCardFooter class="agent-runtime-footer" :label="title + '的操作'">
      <template #meta>
        <dl class="agent-runtime-process">
          <div :title="startedAt ? '启动时间：' + startedAt.fullLabel : '启动时间未记录'">
            <dt>启动</dt>
            <dd><time v-if="startedAt" :datetime="startedAt.iso">{{ startedAt.label }}</time><span v-else>未记录</span></dd>
          </div>
          <div v-if="instance.process">
            <dt>PID</dt><dd>{{ instance.process.pid }}</dd>
          </div>
        </dl>
      </template>
      <a-tooltip :content="activationHint">
        <span class="agent-runtime-activate-wrapper">
          <CardIconButton
            class="agent-runtime-activate"
            tone="launch"
            :disabled="!instance.actions.canActivateTerminal || activating"
            :aria-label="'定位会话窗口：' + title"
            :aria-busy="activating || undefined"
            @click="emit('activate', instance)"
          >
            <IconLoading v-if="activating" class="agent-runtime-activating" aria-hidden="true" />
            <IconLaunch v-else aria-hidden="true" />
            <span>{{ activating ? "正在定位" : "定位窗口" }}</span>
          </CardIconButton>
        </span>
      </a-tooltip>
    </WorkspaceCardFooter>
  </WorkspaceCard>
</template>
