<script setup lang="ts">
import { computed } from "vue";
import { ArrowRight, ChevronRight, Copy, FolderOpen, Terminal } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import type { AgentCliDescriptor, AgentRuntimeSession } from "../../stores/provider-types";
import type { AgentSessionRow } from "../../stores/agent-session-types";
import { copyText } from "../../composables/useClipboard";
import { sessionDayLabel, sessionModelLabel, sessionTime } from "../../utils/agent-session-display";
import AgentCliIcon from "../AgentCliIcon.vue";
import WorkspaceCard from "../workspace-card/WorkspaceCard.vue";
import AgentSessionRelation from "./AgentSessionRelation.vue";

const props = withDefaults(defineProps<{
  rows: AgentSessionRow[];
  agents?: readonly AgentCliDescriptor[];
  selectedSessionRef?: string;
  allowResume?: boolean;
  resumeBusy?: (sessionRef: string) => boolean;
  runtimeSessions?: AgentRuntimeSession[];
  activatingId?: string | null;
  disabled?: boolean;
}>(), { agents: () => [], selectedSessionRef: "", allowResume: false, runtimeSessions: () => [], activatingId: null, disabled: false });
const emit = defineEmits<{ detail: [row: AgentSessionRow]; resume: [row: AgentSessionRow]; parent: [sessionRef: string]; activate: [session: AgentRuntimeSession] }>();
const entries = computed(() => {
  const labels = new Map(props.agents.map((agent) => [agent.kind, agent.label]));
  const terminals = new Map(props.runtimeSessions
    .filter((session) => session.actions.canActivateTerminal)
    .map((session) => [session.runtimeId, session]));
  let previousDay = "";
  return props.rows.map((row) => {
    const day = sessionDayLabel(row.session.updatedAt);
    const dayHeading = day !== previousDay ? day : "";
    previousDay = day;
    return {
      row,
      dayHeading,
      label: labels.get(row.session.cliKind) || row.session.cliKind,
      terminals: [...new Set(row.runtimeIds)].flatMap((id) => {
        const terminal = terminals.get(id);
        return terminal ? [terminal] : [];
      }),
    };
  });
});
async function copy(value: string, label: string) {
  try { await copyText(value); Message.success(`已复制${label}`); }
  catch (failure) { Message.error(failure instanceof Error ? failure.message : String(failure)); }
}
</script>

<template>
  <div class="agent-session-list">
    <template v-for="{ row, label, terminals, dayHeading } in entries" :key="row.sessionRef">
      <h3 v-if="dayHeading" class="agent-session-day-heading">{{ dayHeading }}</h3>
      <WorkspaceCard
        class="agent-session-row"
        :class="{ 'is-selected': row.sessionRef === selectedSessionRef }"
        :fixed-height="false"
        :interacting="row.sessionRef === selectedSessionRef"
        :data-session-ref="row.sessionRef"
        :data-session-role="row.role"
      >
        <div class="agent-session-heading">
          <AgentCliIcon :kind="row.session.cliKind" :size="28" />
          <div class="agent-session-row-body">
            <div class="agent-session-title-line">
              <button type="button" class="agent-session-open" :disabled="disabled" :title="`查看会话详情：${row.session.title}`" @click="emit('detail', row)">
                <strong>{{ row.session.title }}</strong><ChevronRight :size="14" aria-hidden="true" />
              </button>
              <time :datetime="row.session.updatedAt || undefined" :title="`最后更新：${sessionTime(row.session.updatedAt)}`">{{ sessionTime(row.session.updatedAt, true) }}</time>
            </div>
            <div class="agent-session-meta">
              <span class="agent-session-agent-label">{{ label }}</span>
              <AgentSessionRelation :row="row" @parent="emit('parent', $event)" />
              <span v-if="row.session.archived" class="agent-session-archived">已归档</span>
              <span>{{ sessionModelLabel(row.session) }}</span>
              <span class="agent-session-activity" :class="{ 'is-active': row.activityState === 'active' }">{{ row.activityState === 'active' ? `活动中${row.runtimeIds.length > 1 ? ` · ${row.runtimeIds.length} 个实例` : ''}` : '待确认活动' }}</span>
            </div>
          </div>
        </div>
        <p v-if="row.session.preview" class="agent-session-preview">{{ row.session.preview }}</p>
        <p v-if="!row.session.canResume && row.resumeReason" class="agent-session-readonly">{{ row.resumeReason }}</p>
        <div class="agent-session-row-footer">
          <div class="agent-session-locations">
            <button v-if="row.session.workdir" type="button" class="agent-session-workdir" :title="`复制完整目录：${row.session.workdir}`" aria-label="复制会话目录" @click="copy(row.session.workdir, '会话目录')"><FolderOpen :size="13" aria-hidden="true" /><span>{{ row.session.workdir }}</span></button>
            <button type="button" class="agent-session-copy-id" :title="`复制会话 ID：${row.session.id}`" aria-label="复制会话 ID" @click="copy(row.session.id, '会话 ID')"><Copy :size="12" aria-hidden="true" /><span>会话 ID</span></button>
          </div>
          <div v-if="terminals.length || allowResume && row.activityState !== 'active'" class="agent-session-row-actions">
            <a-button v-for="(runtime, index) in terminals" :key="runtime.runtimeId" size="small" :disabled="disabled" :loading="activatingId === runtime.runtimeId" @click="emit('activate', runtime)"><template #icon><Terminal :size="13" /></template>切回终端{{ terminals.length > 1 ? ` ${index + 1}` : '' }}</a-button>
            <a-button v-if="allowResume && row.activityState !== 'active'" size="small" :disabled="disabled || !row.session.canResume || resumeBusy?.(row.sessionRef)" :title="row.resumeReason || undefined" @click="emit('resume', row)"><template #icon><ArrowRight :size="13" /></template>{{ resumeBusy?.(row.sessionRef) ? '结果待确认' : '继续会话' }}</a-button>
          </div>
        </div>
      </WorkspaceCard>
    </template>
  </div>
</template>
