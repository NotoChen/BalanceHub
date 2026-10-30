<script setup lang="ts">
import { computed } from "vue";
import { CircleCheck, CircleAlert, LoaderCircle } from "@lucide/vue";
import {
  agentAssetOperationOutcomeLabels,
  agentAssetOperationPhaseLabels,
} from "../../composables/useAgentAssetConsole";
import { agentConfigurationFileName } from "../../utils/agent-configuration-display";
import AgentConfigurationDiagnostics from "./AgentConfigurationDiagnostics.vue";
import type {
  AgentConfigurationDiagnostic,
  AgentConfigurationFileResult,
  AgentConfigurationOperation,
} from "../../stores/agent-configuration-types";
import type { AgentConfigurationEditableDocument } from "../../stores/agent-configuration-types";

const props = defineProps<{
  documents: AgentConfigurationEditableDocument[];
  operation: AgentConfigurationOperation | null;
  starting: boolean;
  error: string;
  diagnostics: AgentConfigurationDiagnostic[];
}>();
const emit = defineEmits<{ close: []; cancel: [] }>();

const done = computed(
  () =>
    !props.starting &&
    (Boolean(props.error) || props.operation?.phase === "completed"),
);
const failed = computed(() => Boolean(props.error));
const headline = computed(() => {
  if (props.error) return "配置未保存";
  if (props.starting) return "正在提交保存请求";
  const operation = props.operation;
  if (!operation) return "正在提交保存请求";
  if (operation.phase !== "completed")
    return agentAssetOperationPhaseLabels[operation.phase];
  return operation.outcome
    ? agentAssetOperationOutcomeLabels[operation.outcome]
    : "已完成";
});
const unverified = computed(
  () =>
    props.operation?.outcome === "appliedUnverified" ||
    props.operation?.outcome === "outcomeUnknown",
);
const results = computed(() => {
  const files = new Map<string, AgentConfigurationFileResult>(
    (props.operation?.files ?? []).map((file) => [file.sourceId, file]),
  );
  return props.documents.map((document) => {
    const result = files.get(document.sourceId);
    return {
      sourceId: document.sourceId,
      label: agentConfigurationFileName(document),
      path: document.path,
      state: result?.state ?? null,
      message: result?.message ?? null,
    };
  });
});
const stateLabels = {
  unchanged: "未改动",
  applied: "已写入",
  unknown: "结果未知",
} as const;
</script>

<template>
  <section class="agent-configuration-apply" aria-live="polite">
    <header>
      <span
        :class="[
          'agent-configuration-apply-icon',
          { 'is-failed': failed, 'is-pending': !done },
        ]"
      >
        <CircleAlert v-if="failed" :size="20" />
        <CircleCheck v-else-if="done && !unverified" :size="20" />
        <CircleAlert v-else-if="done" :size="20" />
        <LoaderCircle v-else class="agent-configuration-apply-spin" :size="20" />
      </span>
      <div>
        <strong>{{ headline }}</strong>
        <p>
          {{
            done
              ? "写入已经结束，结果以每个文件为准。"
              : "正在按顺序写入这些文件，关闭面板不会中断保存。"
          }}
        </p>
      </div>
    </header>
    <ul class="agent-configuration-apply-files">
      <li v-for="file in results" :key="file.sourceId">
        <span class="agent-configuration-apply-file">
          <strong>{{ file.label }}</strong>
          <code>{{ file.path }}</code>
        </span>
        <span class="agent-configuration-apply-state" :data-state="file.state">
          {{ file.state ? stateLabels[file.state] : "等待写入" }}
        </span>
        <span v-if="file.message" class="agent-configuration-apply-message">{{
          file.message
        }}</span>
      </li>
    </ul>
    <p v-if="unverified" class="agent-workspace-note">
      文件已写入，但结果尚未核对。重新读取可以确认磁盘上的实际内容。
    </p>
    <p v-if="error" class="agent-workspace-error" role="status">{{ error }}</p>
    <AgentConfigurationDiagnostics :diagnostics="diagnostics" />
    <footer>
      <a-button v-if="!done" @click="emit('close')">在后台继续</a-button>
      <a-button v-if="!done && operation?.canCancel" @click="emit('cancel')"
        >取消写入</a-button
      >
      <a-button v-if="done" type="primary" @click="emit('close')">完成</a-button>
    </footer>
  </section>
</template>

<style scoped>
.agent-configuration-apply {
  display: grid;
  gap: 16px;
}
.agent-configuration-apply header {
  display: flex;
  gap: 12px;
  align-items: flex-start;
}
.agent-configuration-apply-icon {
  color: var(--color-success-6, #00b42a);
  display: flex;
  padding-top: 2px;
}
.agent-configuration-apply-icon.is-pending {
  color: rgb(var(--primary-6));
}
.agent-configuration-apply-icon.is-failed {
  color: var(--color-danger-6, #f53f3f);
}
.agent-configuration-apply header strong {
  font-size: 15px;
  font-weight: 550;
  color: var(--color-text-1);
}
.agent-configuration-apply header p {
  margin: 6px 0 0;
  font-size: 12px;
  line-height: 1.65;
  color: var(--color-text-3);
}
.agent-configuration-apply-files {
  margin: 0;
  padding: 0;
  list-style: none;
  border: 1px solid var(--color-border-2);
  border-radius: 9px;
  overflow: hidden;
}
.agent-configuration-apply-files li {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 4px 12px;
  padding: 11px 14px;
}
.agent-configuration-apply-files li + li {
  border-top: 1px solid var(--color-border-1, var(--color-border-2));
}
.agent-configuration-apply-file {
  display: grid;
  gap: 3px;
  min-width: 0;
}
.agent-configuration-apply-file strong {
  font-size: 13px;
  font-weight: 500;
  color: var(--color-text-1);
}
.agent-configuration-apply-file code {
  font-size: 11px;
  color: var(--color-text-3);
  overflow-wrap: anywhere;
}
.agent-configuration-apply-state {
  align-self: start;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  color: var(--color-text-3);
  background: var(--color-fill-2);
  white-space: nowrap;
}
.agent-configuration-apply-state[data-state="applied"] {
  color: var(--color-success-6, #00b42a);
  background: var(--color-success-light-1, var(--color-fill-2));
}
.agent-configuration-apply-state[data-state="unknown"] {
  color: var(--color-warning-6, #ff7d00);
  background: var(--color-warning-light-1, var(--color-fill-2));
}
.agent-configuration-apply-message {
  grid-column: 1 / -1;
  font-size: 11px;
  line-height: 1.6;
  color: var(--color-text-3);
  overflow-wrap: anywhere;
}
.agent-configuration-apply footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.agent-configuration-apply-spin {
  animation: agent-configuration-apply-spin 1s linear infinite;
}
@keyframes agent-configuration-apply-spin {
  to {
    transform: rotate(360deg);
  }
}
@media (prefers-reduced-motion: reduce) {
  .agent-configuration-apply-spin {
    animation: none;
  }
}
</style>
