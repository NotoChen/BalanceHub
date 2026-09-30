<script setup lang="ts">
import { computed, reactive, toRef } from "vue";
import { useAgentSessionResume } from "../../composables/useAgentSessionResume";
import type { AgentCliDescriptor, AgentInstallation, Provider, ProviderApiKeyOption } from "../../stores/provider-types";
import type { AgentSessionRow } from "../../stores/agent-session-types";
import { maskApiKey, providerApiKeyDisplayName, providerSafeDisplayText } from "../../utils/provider-display";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentCliTerminalFields from "./AgentCliTerminalFields.vue";
import AgentProviderOptions from "./AgentProviderOptions.vue";
import "../../styles/modules/agent-launch-provider.css";

const props = defineProps<{
  visible: boolean; row: AgentSessionRow | null; scopeRevision: string;
  providers: readonly Provider[]; installations: readonly AgentInstallation[]; agents: readonly AgentCliDescriptor[];
}>();
const emit = defineEmits<{ close: [] }>();
const model = reactive(useAgentSessionResume({ visible: toRef(props, "visible"), row: toRef(props, "row"), scopeRevision: toRef(props, "scopeRevision"),
  providers: toRef(props, "providers"), installations: toRef(props, "installations"), close: () => emit("close") }));
const agentLabel = computed(() => props.agents.find((agent) => agent.kind === props.row?.session.cliKind)?.label || props.row?.session.cliKind || "Agent");
const cliChoices = computed(() => model.cliOptions.map((choice) => ({ ...choice, value: choice.path, available: true, reason: null })));
function keyLabel(key: ProviderApiKeyOption) {
  const provider = model.selectedProvider;
  const text = `${providerApiKeyDisplayName(key)} · ${maskApiKey(key.key || key.maskedKey)}`;
  return provider ? providerSafeDisplayText(provider, text) : "API Key";
}
</script>

<template>
  <a-modal :visible="visible" :width="640" modal-class="surface-modal agent-session-resume-modal" :footer="false" closable mask-closable esc-to-close unmount-on-close @cancel="emit('close')">
    <template #title><div class="surface-modal-title"><AgentCliIcon v-if="row" :kind="row.session.cliKind" :size="20" /><strong>{{ agentLabel }} · 继续会话</strong></div></template>
    <div v-if="row" class="agent-session-resume-body">
      <div class="agent-session-resume-target"><strong>{{ row.session.title }}</strong><code>{{ row.session.id }}</code><span :title="row.session.workdir">{{ row.session.workdir }}</span></div>
      <div class="agent-session-resume-intents" role="radiogroup" aria-label="继续方式">
        <label><input v-model="model.intentKind" type="radio" value="native" /><span><strong>沿用 Agent 当前配置</strong><small>使用原生配置继续原会话，无需选择中转站。</small></span></label>
        <label><input v-model="model.intentKind" type="radio" value="provider" /><span><strong>选择中转站继续</strong><small>临时使用所选中转站，继续同一条原生会话。</small></span></label>
      </div>
      <section v-if="model.intentKind === 'provider'" class="agent-session-resume-providers">
        <p v-if="!providers.length" class="agent-workspace-note">尚未添加中转站，可以沿用 Agent 当前配置继续。</p>
        <AgentProviderOptions v-else :providers="providers" :selected-id="model.providerId" @select="model.selectProvider" />
        <label v-if="model.selectedProvider" class="agent-session-form-field"><span>使用的 API Key</span><a-select v-model="model.apiKeyLocalId" aria-label="继续会话使用的 API Key"><a-option value="">使用中转站当前 Key</a-option><a-option v-for="key in model.apiKeys" :key="key.localId" :value="key.localId">{{ keyLabel(key) }}</a-option></a-select></label>
      </section>
      <AgentCliTerminalFields :agent-kind="row.session.cliKind" :label="agentLabel" v-model:cli-value="model.cliPath" :cli-options="cliChoices" v-model:terminal-kind="model.terminalKind" :terminal-options="model.terminalOptions" :probing="model.probing" :error="model.probeError" cli-aria-label="继续会话使用的 CLI 路径" @refresh="model.refreshEnvironment" />
      <footer class="agent-session-resume-footer"><p role="status">{{ model.unavailableReason || '确认后在后台启动，可在任务中心查看结果。' }}</p><a-button type="primary" :disabled="!model.canConfirm" @click="model.confirm">确认继续</a-button></footer>
    </div>
  </a-modal>
</template>
