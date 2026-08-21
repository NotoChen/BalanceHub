<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";
import { Message } from "@arco-design/web-vue";
import {
  IconCheck,
  IconClockCircle,
  IconClose,
  IconCloud,
  IconCommand,
  IconCopy,
  IconFolder,
  IconLink,
  IconLock,
} from "@arco-design/web-vue/es/icon";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import type { TemporaryCliLaunchPreview } from "../stores/providers";
import { agentCliLabel } from "../utils/cli-environment";
import { copyText } from "../composables/useClipboard";
import AgentCliIcon from "./AgentCliIcon.vue";
import TerminalBrandIcon from "./TerminalBrandIcon.vue";

const props = defineProps<{
  visible: boolean;
  preview: TemporaryCliLaunchPreview | null;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  confirm: [];
}>();
const store = useCliRuntimeStore();

const cliLabel = computed(() =>
  props.preview ? agentCliLabel(store.cliEnvironmentProbe, props.preview.cliKind) : "Agent CLI",
);
const sessionLabel = computed(() => {
  if (!props.preview) return "";
  return props.preview.sessionMode === "history" ? "继续历史会话" : "新会话";
});
const environmentEntries = computed(() =>
  Object.entries(props.preview?.environment ?? {}).sort(([left], [right]) => left.localeCompare(right)),
);
const copiedCommand = ref(false);
let copyResetTimer: ReturnType<typeof setTimeout> | null = null;

const copyLabel = computed(() => (copiedCommand.value ? "已复制" : "复制命令"));

function scheduleCopyReset() {
  if (copyResetTimer) clearTimeout(copyResetTimer);
  copyResetTimer = setTimeout(() => {
    copiedCommand.value = false;
    copyResetTimer = null;
  }, 1800);
}

function handleVisibleChange(visible: boolean) {
  emit("update:visible", visible);
}

async function copyCommand() {
  const command = props.preview?.command.trim();
  if (!command) return;
  try {
    await copyText(command);
    copiedCommand.value = true;
    scheduleCopyReset();
    Message.success("已复制 CLI 命令");
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}

onBeforeUnmount(() => {
  if (copyResetTimer) clearTimeout(copyResetTimer);
});
</script>

<template>
  <a-modal
    :visible="visible"
    width="min(900px, calc(100vw - 32px))"
    modal-class="surface-modal temporary-cli-preview-modal"
    title-align="start"
    :footer="false"
    closable
    mask-closable
    esc-to-close
    unmount-on-close
    @update:visible="handleVisibleChange"
  >
    <template #title>
      <div class="surface-modal-title temporary-cli-preview-title">
        <span class="surface-modal-title-icon temporary-cli-preview-title-icon">
          <AgentCliIcon v-if="preview" :kind="preview.cliKind" :size="20" />
          <icon-command v-else aria-hidden="true" />
        </span>
        <span class="surface-modal-title-copy">
          <span>临时 CLI</span>
          <strong>确认启动 {{ cliLabel }}</strong>
        </span>
      </div>
    </template>

    <div v-if="preview" class="temporary-cli-preview">
      <header class="temporary-cli-preview-context">
        <div class="temporary-cli-preview-context-copy">
          <span>启动目标</span>
          <strong :title="preview.providerName">{{ preview.providerName }}</strong>
        </div>
        <span class="temporary-cli-preview-session-badge">
          <icon-clock-circle aria-hidden="true" />
          {{ sessionLabel }}
        </span>
      </header>

      <section class="temporary-cli-preview-runtime" aria-label="运行环境">
        <div class="temporary-cli-preview-runtime-item">
          <span class="temporary-cli-preview-runtime-icon temporary-cli-preview-runtime-icon-cli">
            <AgentCliIcon :kind="preview.cliKind" :size="24" />
          </span>
          <span class="temporary-cli-preview-runtime-copy">
            <small>CLI</small>
            <strong>{{ cliLabel }}</strong>
            <code :title="preview.cliPath">{{ preview.cliPath }}</code>
          </span>
        </div>
        <div class="temporary-cli-preview-runtime-item">
          <span class="temporary-cli-preview-runtime-icon temporary-cli-preview-runtime-icon-terminal">
            <TerminalBrandIcon :kind="preview.terminalKind" :name="preview.terminalName" :size="24" />
          </span>
          <span class="temporary-cli-preview-runtime-copy">
            <small>终端</small>
            <strong>{{ preview.terminalName }}</strong>
            <code>{{ preview.terminalKind }}</code>
          </span>
        </div>
      </section>

      <section class="temporary-cli-preview-details" aria-label="启动配置">
        <div class="temporary-cli-preview-detail-item">
          <span><icon-cloud aria-hidden="true" />中转站</span>
          <strong :title="preview.providerName">{{ preview.providerName }}</strong>
        </div>
        <div class="temporary-cli-preview-detail-item">
          <span><icon-command aria-hidden="true" />模型</span>
          <strong :title="preview.model || undefined">{{ preview.model || "由 CLI 或历史会话决定" }}</strong>
        </div>
        <div class="temporary-cli-preview-detail-item temporary-cli-preview-detail-item-wide">
          <span><icon-folder aria-hidden="true" />工作目录</span>
          <strong :title="preview.workdir">{{ preview.workdir }}</strong>
        </div>
        <div v-if="preview.sessionName" class="temporary-cli-preview-detail-item">
          <span><icon-command aria-hidden="true" />会话名称</span>
          <strong :title="preview.sessionName">{{ preview.sessionName }}</strong>
        </div>
        <div v-if="preview.resumeId" class="temporary-cli-preview-detail-item">
          <span><icon-clock-circle aria-hidden="true" />Resume ID</span>
          <strong :title="preview.resumeId">{{ preview.resumeId }}</strong>
        </div>
        <div class="temporary-cli-preview-detail-item temporary-cli-preview-detail-item-wide">
          <span><icon-lock aria-hidden="true" />API Key</span>
          <strong :title="preview.apiKeyLabel">{{ preview.apiKeyLabel }}</strong>
          <code class="temporary-cli-preview-secret" :title="preview.apiKey">{{ preview.apiKey }}</code>
        </div>
        <div class="temporary-cli-preview-detail-item temporary-cli-preview-detail-item-wide">
          <span><icon-link aria-hidden="true" />Base URL</span>
          <strong :title="preview.baseUrl">{{ preview.baseUrl }}</strong>
        </div>
      </section>

      <section class="temporary-cli-preview-command" aria-label="CLI 命令">
        <div class="temporary-cli-preview-section-header">
          <div class="temporary-cli-preview-section-title">
            <icon-command aria-hidden="true" />
            <strong>命令预览</strong>
            <span>将按以下参数启动</span>
          </div>
          <a-tooltip content="复制完整命令">
            <a-button
              class="temporary-cli-preview-copy-command"
              type="text"
              size="small"
              aria-label="复制完整命令"
              @click="copyCommand"
            >
              <template #icon>
                <icon-check v-if="copiedCommand" />
                <icon-copy v-else />
              </template>
              {{ copyLabel }}
            </a-button>
          </a-tooltip>
        </div>
        <pre>{{ preview.command }}</pre>
      </section>

      <details v-if="environmentEntries.length > 0" class="temporary-cli-preview-disclosure">
        <summary>
          <span><icon-command aria-hidden="true" />环境变量</span>
          <small>{{ environmentEntries.length }} 项</small>
        </summary>
        <div class="temporary-cli-preview-kv-list">
          <code v-for="([name, value]) in environmentEntries" :key="name">{{ name }}={{ value }}</code>
        </div>
      </details>

      <details v-if="preview.settingsContent" class="temporary-cli-preview-disclosure" open>
        <summary>
          <span><icon-link aria-hidden="true" />{{ cliLabel }} 临时配置</span>
          <small v-if="preview.settingsPath" :title="preview.settingsPath">{{ preview.settingsPath }}</small>
        </summary>
        <pre>{{ preview.settingsContent }}</pre>
      </details>

      <footer class="temporary-cli-preview-actions">
        <a-button @click="emit('update:visible', false)">
          <template #icon><icon-close /></template>
          取消
        </a-button>
        <a-button type="primary" @click="emit('confirm')">
          <template #icon><icon-check /></template>
          确认并启动
        </a-button>
      </footer>
    </div>
  </a-modal>
</template>
