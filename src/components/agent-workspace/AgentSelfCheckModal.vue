<script setup lang="ts">
import { computed } from "vue";
import { RefreshCw } from "@lucide/vue";
import AgentCliIcon from "../AgentCliIcon.vue";
import type {
  AgentSelfCheckEntry,
  AgentSelfCheckHook,
  AgentSelfCheckReport,
  AgentSelfCheckStatus,
} from "../../stores/agent-self-check-types";

const props = defineProps<{
  visible: boolean;
  loading: boolean;
  error: string;
  report: AgentSelfCheckReport | null;
}>();
const emit = defineEmits<{ close: []; refresh: [] }>();

const statusLabels: Record<AgentSelfCheckStatus, string> = {
  ready: "可用",
  degraded: "部分可用",
  unavailable: "不可用",
  unknown: "未检查",
};
const hookStateLabels: Record<AgentSelfCheckHook["state"], string> = {
  not_installed: "未安装",
  installed_untrusted: "未信任",
  installed_unverified: "待校验",
  healthy: "正常",
  disabled: "已停用",
  conflict: "冲突",
  helper_missing: "助手缺失",
  spool_blocked: "数据目录不可用",
  unsupported: "不支持",
};
const hookProblemStates = new Set<AgentSelfCheckHook["state"]>([
  "installed_untrusted", "installed_unverified", "conflict", "helper_missing", "spool_blocked",
]);
function hookTitle(hook: AgentSelfCheckHook) {
  return [hook.configPath, hook.detail].filter(Boolean).join("\n");
}
const groupOrder: AgentSelfCheckStatus[] = ["degraded", "unavailable", "unknown", "ready"];
const groups = computed(() => groupOrder
  .map((status) => ({
    status,
    label: statusLabels[status],
    entries: (props.report?.entries ?? []).filter((entry) => entry.status === status),
  }))
  .filter((group) => group.entries.length > 0));
const summary = computed(() => {
  const report = props.report;
  if (!report) return "";
  return ([
    ["ready", "可用"],
    ["degraded", "部分可用"],
    ["unavailable", "不可用"],
    ["unknown", "未检查"],
  ] as const)
    .filter(([key]) => report[key] > 0)
    .map(([key, label]) => `${report[key]} 个${label}`)
    .join(" · ");
});
const checkedAt = computed(() => {
  const value = props.report?.checkedAt;
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString("zh-CN", { hour12: false });
});
function issueKey(entry: AgentSelfCheckEntry, index: number) {
  return `${entry.agentKind}:${index}`;
}
</script>

<template>
  <a-modal :visible="visible" width="min(780px, calc(100vw - 32px))"
    modal-class="surface-modal agent-workspace-modal agent-self-check-modal" :footer="false"
    closable mask-closable esc-to-close unmount-on-close @cancel="emit('close')">
    <template #title>
      <div class="agent-modal-heading"><strong>环境自检</strong><span v-if="summary" class="agent-self-check-summary">{{ summary }}</span></div>
    </template>
    <div class="agent-modal-body agent-self-check-body">
      <p class="agent-workspace-note">检查当前工作区下的 CLI 可执行文件与配置来源；先刷新 Agent 摘要再汇总结果，不改动任何配置。</p>
      <p v-if="error" class="agent-workspace-error" role="alert">{{ error }}</p>
      <p v-if="loading" role="status"><a-spin :size="14" /> 正在刷新 Agent 摘要并生成自检报告…</p>
      <template v-else-if="report">
        <div class="agent-self-check-results">
          <div v-for="group in groups" :key="group.status" class="agent-self-check-group">
            <h3 class="agent-self-check-group-title">
              <span class="agent-self-check-status" :class="`is-${group.status}`">{{ group.label }}</span>
              <span>{{ group.entries.length }}</span>
            </h3>
            <ul class="agent-self-check-list">
              <li v-for="entry in group.entries" :key="entry.agentKind" class="agent-self-check-entry" :class="`is-${group.status}`">
                <AgentCliIcon :kind="entry.agentKind" :size="20" decorative />
                <div class="agent-self-check-content">
                  <div class="agent-self-check-heading">
                    <strong>{{ entry.label }}</strong>
                    <span v-if="entry.version" class="agent-self-check-version">{{ entry.version }}</span>
                    <code v-if="entry.executable" class="agent-self-check-path" :title="entry.executable">{{ entry.executable }}</code>
                    <span v-else class="agent-self-check-path">未找到 CLI</span>
                  </div>
                  <div class="agent-self-check-facts">
                    <span>配置来源 {{ entry.configurationSources }} 项</span>
                    <span v-if="entry.sessionHistory">会话来源 {{ entry.sessionSourcesAvailable }}/{{ entry.sessionSourcesTotal }} 可读</span>
                    <span v-else>会话读取未接入</span>
                    <span v-if="entry.hook" class="agent-self-check-hook" :class="{ 'is-problem': hookProblemStates.has(entry.hook.state) }" :title="hookTitle(entry.hook)">Hook {{ hookStateLabels[entry.hook.state] }}</span>
                    <span v-else>{{ entry.hookInventory ? "Hook 清单已接入" : "Hook 清单未接入" }}</span>
                  </div>
                  <ul v-if="entry.issues.length" class="agent-self-check-issues">
                    <li v-for="(issue, index) in entry.issues" :key="issueKey(entry, index)" class="agent-self-check-issue" :class="`is-${issue.severity}`">
                      <span class="agent-self-check-issue-message">{{ issue.message }}</span>
                      <span class="agent-self-check-issue-detail">{{ issue.detail }}</span>
                    </li>
                  </ul>
                </div>
              </li>
            </ul>
          </div>
        </div>
      </template>
      <p v-else class="agent-workspace-note">尚未生成自检报告。</p>
      <footer class="agent-modal-actions agent-self-check-actions">
        <span v-if="checkedAt" class="agent-self-check-time">检查时间 {{ checkedAt }}</span>
        <a-button @click="emit('close')">关闭</a-button>
        <a-button type="primary" :loading="loading" @click="emit('refresh')">
          <template v-if="!loading" #icon><RefreshCw :size="14" /></template>
          重新自检
        </a-button>
      </footer>
    </div>
  </a-modal>
</template>

<style scoped>
.agent-self-check-summary { color: var(--color-text-3); font-size: 12px; font-weight: 400; }
.agent-self-check-body { display: grid; gap: 14px; }
.agent-self-check-results { display: grid; gap: 14px; max-height: min(62vh, 640px); margin-inline: -2px; padding-inline: 2px; overflow-y: auto; }
.agent-self-check-group { display: grid; gap: 8px; }
.agent-self-check-group-title { display: flex; align-items: center; gap: 8px; margin: 0; color: var(--color-text-3); font-size: 12px; font-weight: 500; }
.agent-self-check-list { display: grid; gap: 8px; padding: 0; margin: 0; list-style: none; }
.agent-self-check-entry { display: grid; grid-template-columns: 20px minmax(0, 1fr); gap: 10px; padding: 10px 12px; border: 1px solid var(--color-border-2); border-radius: 8px; background: var(--color-fill-1); }
.agent-self-check-entry.is-degraded { border-color: rgb(var(--warning-3)); }
.agent-self-check-entry.is-unavailable { border-color: rgb(var(--danger-3)); }
.agent-self-check-content { display: grid; gap: 6px; min-width: 0; }
.agent-self-check-heading { display: flex; align-items: baseline; flex-wrap: wrap; gap: 4px 10px; min-width: 0; }
.agent-self-check-version { color: var(--color-text-3); font-size: 12px; }
.agent-self-check-facts { display: flex; flex-wrap: wrap; gap: 2px 12px; color: var(--color-text-3); font-size: 11px; }
.agent-self-check-hook.is-problem { color: rgb(var(--warning-6)); font-weight: 600; }
.agent-self-check-path { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--color-text-3); font: 11px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; }
.agent-self-check-issues { display: grid; gap: 6px; padding: 0; margin: 0; list-style: none; }
.agent-self-check-issue { display: grid; gap: 2px; padding-left: 8px; border-left: 2px solid var(--color-border-2); }
.agent-self-check-issue.is-error { border-left-color: rgb(var(--danger-5)); }
.agent-self-check-issue.is-warning { border-left-color: rgb(var(--warning-5)); }
.agent-self-check-issue-message { font-size: 12px; font-weight: 600; }
.agent-self-check-issue-detail { color: var(--color-text-3); font-size: 12px; overflow-wrap: anywhere; }
.agent-self-check-status { display: inline-flex; align-items: center; padding: 0 6px; border-radius: 999px; background: var(--color-fill-2); font-size: 11px; font-weight: 600; }
.agent-self-check-status.is-ready { color: rgb(var(--success-6)); background: rgb(var(--success-1)); }
.agent-self-check-status.is-degraded { color: rgb(var(--warning-6)); background: rgb(var(--warning-1)); }
.agent-self-check-status.is-unavailable { color: rgb(var(--danger-6)); background: rgb(var(--danger-1)); }
.agent-self-check-actions { align-items: center; }
.agent-self-check-time { margin-right: auto; color: var(--color-text-3); font-size: 12px; }
</style>
