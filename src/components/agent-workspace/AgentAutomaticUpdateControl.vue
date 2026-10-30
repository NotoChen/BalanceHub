<script setup lang="ts">
import { computed } from "vue";
import { useAgentLifecycleStore } from "../../stores/agent-lifecycle";
import { useAgentWorkspaceStore } from "../../stores/agent-workspace";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import type { AgentAutomaticUpdate, AgentLifecycleTarget } from "../../stores/agent-lifecycle-types";
import { agentCliLabel } from "../../utils/cli-environment";

const props = defineProps<{ entry: AgentAutomaticUpdate; target?: AgentLifecycleTarget }>();
const store = useAgentLifecycleStore();
const navigation = useAgentWorkspaceStore();
const cli = useCliRuntimeStore();
const saving = computed(() => Boolean(props.entry.key && store.automaticSaving[props.entry.key]));
const error = computed(() => props.entry.key ? store.automaticErrors[props.entry.key] : undefined);
const labels: Record<AgentAutomaticUpdate["state"], string> = {
  disabled: "未开启", scheduled: "已开启", waitingForIdle: "等待结束使用", updating: "正在更新", paused: "已暂停", unavailable: "暂不可用",
};
const showMessage = computed(() => Boolean(error.value) || ["waitingForIdle", "paused", "unavailable"].includes(props.entry.state));
function change(enabled: string | number | boolean) { void store.setAutomatic(props.entry, Boolean(enabled), props.target); }
function details() {
  if (!props.entry.operationId) return;
  void store.recover();
  navigation.openOperation("lifecycle", props.entry.operationId);
}
</script>

<template>
  <section class="agent-automatic-update" :data-state="entry.state" aria-label="自动更新设置">
    <div class="agent-automatic-update-row">
      <a-tooltip :content="entry.message"><span class="agent-automatic-update-label">自动更新</span></a-tooltip>
      <a-switch size="small" :model-value="entry.enabled" :loading="saving" :disabled="saving || (!entry.enabled && !entry.canEnable)"
        :aria-label="`${agentCliLabel(cli.cliEnvironmentProbe, entry.agentKind)} ${entry.channelLabel} 自动更新`" @change="change" />
      <span v-if="entry.enabled" class="agent-automatic-update-state" role="status">{{ labels[entry.state] }}</span>
      <span v-else-if="entry.canEnable" class="agent-automatic-update-hint">每 6 小时检查 · 结束使用后更新</span>
      <a-button v-if="entry.canResume" type="text" size="mini" :loading="saving" @click="change(true)">恢复自动更新</a-button>
      <a-button v-if="entry.operationId" type="text" size="mini" @click="details">更新记录</a-button>
    </div>
    <p v-if="showMessage" class="agent-automatic-update-message" :class="{ 'agent-workspace-error': error }" :role="error ? 'alert' : 'status'">{{ error || entry.message }}</p>
  </section>
</template>

<style scoped>
.agent-automatic-update { min-width: 0; padding-top: 10px; border-top: 1px solid var(--color-border-2); }
.agent-automatic-update-row { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; min-height: 24px; }
.agent-automatic-update-label { font-size: 12px; font-weight: 500; color: var(--color-text-1); }
.agent-automatic-update-state, .agent-automatic-update-hint { font-size: 12px; color: var(--color-text-3); }
[data-state="updating"] .agent-automatic-update-state { color: rgb(var(--primary-6)); }
.agent-automatic-update-message { margin: 6px 0 0; font-size: 12px; line-height: 1.6; color: var(--color-text-2); overflow-wrap: anywhere; }
.agent-automatic-update-message.agent-workspace-error { color: rgb(var(--danger-6)); }
</style>
