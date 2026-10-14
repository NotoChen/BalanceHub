<script setup lang="ts">
import { computed } from "vue";
import type { AgentInstructionSet } from "../../api/agent-instructions";
import type { WslDistribution } from "../../api/agent-wsl";
import type { AgentCliDescriptor, AgentCliKind } from "../../stores/provider-types";
import type { AgentConfigurationSource } from "../../stores/agent-configuration-types";
import { agentAssetScopeLabels } from "../../composables/useAgentAssetCatalog";
import { agentConfigurationSourceStatus } from "../../utils/agent-configuration-display";
import AgentCliIcon from "../AgentCliIcon.vue";
import ContextDetails from "../ContextDetails.vue";

const props = defineProps<{
  set: AgentInstructionSet | null;
  agents: AgentCliDescriptor[];
  distributions?: WslDistribution[];
  environment: string;
  agentKind: AgentCliKind | null;
  targets: AgentConfigurationSource[];
  source?: AgentConfigurationSource;
  loading: boolean;
  error: string;
  writable: boolean;
}>();
const emit = defineEmits<{
  "update:environment": [value: string];
  "update:agentKind": [value: AgentCliKind];
  "update:sourceId": [value: string];
  preview: [remove: boolean];
  edit: [];
  retry: [];
  close: [];
}>();
const selectedAgent = computed(() => props.agents.find((agent) => agent.kind === props.agentKind));
const hasContent = computed(() => Boolean(
  props.set?.common.trim() || (props.agentKind && props.set?.additions[props.agentKind]?.trim()),
));
const writeReason = computed(() => props.source?.actions.find(
  (action) => action.action === "edit" || action.action === "create",
)?.reason || "此规则文件当前不可写，请选择其他文件。");
</script>

<template>
  <a-modal
    :visible="Boolean(set)"
    title="应用到 Agent"
    title-align="start"
    width="min(860px, calc(100vw - 48px))"
    modal-class="surface-modal"
    :footer="false"
    closable mask-closable esc-to-close
    @cancel="emit('close')"
  >
    <div v-if="set" class="instruction-target-flow">
      <div class="instruction-target-scroll">
        <div class="instruction-target-summary">
          <span>选用的规则模板</span>
          <strong>{{ set.name }}</strong>
        </div>
        <label v-if="distributions?.length" class="instruction-environment">
          <span>运行环境</span>
          <a-select :model-value="environment" @update:model-value="emit('update:environment', $event as string)" aria-label="运行环境">
            <a-option value="">本机</a-option>
            <a-option v-for="distro in distributions" :key="distro.id" :value="distro.id" :disabled="!distro.running || Boolean(distro.error)">
              WSL · {{ distro.name }}{{ distro.running ? '' : '（已停止）' }}
            </a-option>
          </a-select>
        </label>
        <fieldset class="instruction-target-section">
          <legend>选择 Agent</legend>
          <div class="instruction-agent-choices">
            <label v-for="agent in agents" :key="agent.kind" class="instruction-choice" :class="{ 'is-selected': agentKind === agent.kind }">
              <input type="radio" name="instruction-target-agent" :value="agent.kind" :checked="agentKind === agent.kind" @change="emit('update:agentKind', agent.kind)" />
              <AgentCliIcon :kind="agent.kind" :size="22" />
              <span>{{ agent.label }}</span>
            </label>
          </div>
          <p v-if="!agents.length" class="instruction-target-note">当前没有可选 Agent。</p>
        </fieldset>
        <fieldset class="instruction-target-section">
          <legend>写入哪个规则文件</legend>
          <div v-if="loading" class="instruction-target-loading" role="status">正在读取规则文件…</div>
          <div v-else-if="error" class="agent-workspace-error" role="alert">
            {{ error }} <a-button size="mini" @click="emit('retry')">重新读取</a-button>
          </div>
          <p v-else-if="!targets.length" class="instruction-target-note">当前 Agent 没有可识别的规则文件。</p>
          <div v-else class="instruction-file-choices">
            <label v-for="item in targets" :key="item.sourceId" class="instruction-choice instruction-file-choice" :class="{ 'is-selected': source?.sourceId === item.sourceId }">
              <input type="radio" name="instruction-target-file" :value="item.sourceId" :checked="source?.sourceId === item.sourceId" @change="emit('update:sourceId', item.sourceId)" />
              <span class="instruction-file-info">
                <span><strong>{{ item.label }}</strong><small>{{ agentAssetScopeLabels[item.scope] }}级 · {{ agentConfigurationSourceStatus(item).presence }}</small></span>
                <code>{{ item.path }}</code>
              </span>
            </label>
          </div>
          <p v-if="source && !loading" class="instruction-target-note">{{ source.effect.message }}</p>
          <p v-if="source && !loading && !writable" class="agent-workspace-error" role="alert">{{ writeReason }}</p>
        </fieldset>
        <div v-if="selectedAgent && source && !loading" class="instruction-target-composition">
          <strong>本次应用</strong>
          <span v-if="set.common.trim()">通用规则</span>
          <span v-if="set.additions[selectedAgent.kind]?.trim()">{{ set.common.trim() ? '＋ ' : '' }}{{ selectedAgent.label }} 专用补充</span>
          <template v-if="!hasContent">
            <span>尚未填写这个 Agent 可用的规则内容。</span>
            <a-button size="mini" type="text" @click="emit('edit')">编辑规则</a-button>
          </template>
        </div>
        <ContextDetails v-if="source && !loading" label="移除文件中已写入的共享规则">
          <p>移除所选文件中由 BalanceHub 写入的规则，文件其余内容保留。下一步可预览移除内容。</p>
          <div><a-button size="small" status="danger" :disabled="!writable" @click="emit('preview', true)">预览移除</a-button></div>
        </ContextDetails>
      </div>
      <footer class="instruction-target-footer">
        <span>核对变更并确认保存后，才会写入所选文件。</span>
        <div>
          <a-button @click="emit('close')">取消</a-button>
          <a-button type="primary" :disabled="!writable || loading || !hasContent" @click="emit('preview', false)">预览并应用</a-button>
        </div>
      </footer>
    </div>
  </a-modal>
</template>

<style scoped>
.instruction-target-flow { display: flex; max-height: calc(100dvh - 190px); min-width: 0; flex-direction: column; gap: 18px; color: var(--surface-ink); }
.instruction-target-scroll { min-height: 0; overflow: auto; overscroll-behavior: contain; padding: 2px 6px 4px 2px; scrollbar-gutter: stable; }
.instruction-target-summary { display: flex; flex-wrap: wrap; align-items: baseline; gap: 8px 12px; margin-bottom: 22px; }
.instruction-target-summary > span { color: var(--surface-ink-soft); font-size: 12px; }
.instruction-target-summary > strong { min-width: 0; overflow-wrap: anywhere; font-size: 16px; }
.instruction-environment { display: grid; gap: 8px; margin-bottom: 20px; font-size: 13px; font-weight: 600; }
.instruction-target-section { min-width: 0; margin: 0 0 22px; border: 0; padding: 0; }
.instruction-target-section > legend { margin-bottom: 10px; padding: 0; font-size: 13px; font-weight: 600; }
.instruction-agent-choices { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 150px), 1fr)); gap: 8px; }
.instruction-choice { display: flex; min-width: 0; align-items: center; gap: 10px; border: 1px solid var(--surface-line); border-radius: 7px; padding: 12px; background: var(--surface-panel); color: var(--surface-ink); font-size: 13px; cursor: pointer; transition: background 150ms ease, border-color 150ms ease; }
.instruction-choice > span { min-width: 0; overflow-wrap: anywhere; }
.instruction-choice input { flex: 0 0 auto; width: 15px; height: 15px; margin: 0; accent-color: var(--surface-accent); cursor: pointer; }
.instruction-choice:hover { border-color: var(--surface-line-strong); background: var(--surface-panel-soft); }
.instruction-choice.is-selected { border-color: var(--surface-accent); background: var(--surface-accent-soft); }
.instruction-choice:has(input:focus-visible) { outline: 2px solid var(--surface-accent); outline-offset: 2px; }
.instruction-file-choices { display: grid; gap: 8px; }
.instruction-file-info { display: grid; flex: 1; gap: 7px; }
.instruction-file-info > span { display: flex; flex-wrap: wrap; gap: 6px 12px; justify-content: space-between; align-items: baseline; }
.instruction-file-info strong { font-weight: 550; }
.instruction-file-info small, .instruction-file-info code { color: var(--surface-ink-soft); font-size: 12px; line-height: 1.6; }
.instruction-file-info code { display: block; white-space: normal; overflow-wrap: anywhere; }
.instruction-target-note { margin: 10px 0 0; color: var(--surface-ink-soft); font-size: 12px; line-height: 1.7; overflow-wrap: anywhere; }
.instruction-target-loading { border-radius: 7px; padding: 22px 12px; background: var(--surface-panel-soft); color: var(--surface-ink-soft); font-size: 13px; }
.instruction-target-composition { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 18px; color: var(--surface-ink-soft); font-size: 12px; line-height: 1.6; }
.instruction-target-composition > strong { margin-right: 8px; color: var(--surface-ink); font-weight: 550; }
.instruction-target-footer { display: flex; flex: 0 0 auto; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px; border-top: 1px solid var(--surface-line); padding-top: 14px; }
.instruction-target-footer > span { color: var(--surface-ink-soft); font-size: 12px; }
.instruction-target-footer > div { display: flex; gap: 8px; margin-left: auto; }
@media (prefers-reduced-motion: reduce) { .instruction-choice { transition: none; } }
</style>
