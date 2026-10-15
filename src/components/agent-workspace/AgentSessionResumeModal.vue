<script setup lang="ts">
import { computed, reactive, toRef } from "vue";
import { useAgentSessionResume } from "../../composables/useAgentSessionResume";
import type {
  AgentCliDescriptor,
  AgentInstallation,
  Provider,
  ProviderApiKeyOption,
} from "../../stores/provider-types";
import type { AgentSessionRow } from "../../stores/agent-session-types";
import {
  maskApiKey,
  providerApiKeyDisplayName,
  providerSafeDisplayText,
} from "../../utils/provider-display";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentModelSelect from "../AgentModelSelect.vue";
import AgentCliTerminalFields from "./AgentCliTerminalFields.vue";
import AgentProviderOptions from "./AgentProviderOptions.vue";
import "../../styles/modules/agent-launch-provider.css";

const props = defineProps<{
  visible: boolean;
  row: AgentSessionRow | null;
  scopeRevision: string;
  providers: readonly Provider[];
  installations: readonly AgentInstallation[];
  agents: readonly AgentCliDescriptor[];
}>();
const emit = defineEmits<{ close: [] }>();
const model = reactive(
  useAgentSessionResume({
    visible: toRef(props, "visible"),
    row: toRef(props, "row"),
    scopeRevision: toRef(props, "scopeRevision"),
    providers: toRef(props, "providers"),
    installations: toRef(props, "installations"),
    close: () => emit("close"),
  }),
);
const agentLabel = computed(
  () =>
    props.agents.find((agent) => agent.kind === props.row?.session.cliKind)
      ?.label ||
    props.row?.session.cliKind ||
    "Agent",
);
const supportsModelSelection = computed(() =>
  Boolean(
    props.agents.find((agent) => agent.kind === props.row?.session.cliKind)
      ?.capabilities.modelSelection,
  ),
);
const cliChoices = computed(() =>
  model.cliOptions.map((choice) => ({
    ...choice,
    value: choice.path,
    available: true,
    reason: null,
  })),
);
function keyLabel(key: ProviderApiKeyOption) {
  const provider = model.selectedProvider;
  const text = `${providerApiKeyDisplayName(key)} · ${maskApiKey(key.key || key.maskedKey)}`;
  return provider ? providerSafeDisplayText(provider, text) : "API Key";
}
</script>

<template>
  <a-modal
    :visible="visible"
    :width="640"
    modal-class="surface-modal agent-session-resume-modal"
    :footer="false"
    closable
    mask-closable
    esc-to-close
    unmount-on-close
    @cancel="emit('close')"
  >
    <template #title
      ><div class="surface-modal-title">
        <AgentCliIcon
          v-if="row"
          :kind="row.session.cliKind"
          :size="20"
        /><strong>{{ agentLabel }} · 继续会话</strong>
      </div></template
    >
    <div v-if="row" class="agent-session-resume-body">
      <div class="agent-session-resume-target">
        <strong>{{ row.session.title }}</strong
        ><code>{{ row.session.id }}</code
        ><span :title="row.session.workdir">{{ row.session.workdir }}</span>
      </div>
      <div
        class="agent-session-resume-intents"
        role="radiogroup"
        aria-label="继续方式"
      >
        <label
          ><input v-model="model.intentKind" type="radio" value="native" /><span
            ><strong>使用当前连接继续</strong
            ><small
              >继续原会话，连接和登录凭据使用 Agent 已有设置。</small
            ></span
          ></label
        >
        <label v-if="row.supportsProviderResume"
          ><input
            v-model="model.intentKind"
            type="radio"
            value="provider"
          /><span
            ><strong>选择中转站继续</strong
            ><small>临时使用所选中转站，继续同一条原生会话。</small></span
          ></label
        >
      </div>
      <section
        v-if="model.intentKind === 'provider'"
        class="agent-session-resume-providers"
      >
        <p v-if="!providers.length" class="agent-workspace-note">
          尚未添加中转站，可以使用 Agent 已有连接继续。
        </p>
        <AgentProviderOptions
          v-else
          :providers="providers"
          :selected-id="model.providerId"
          @select="model.selectProvider"
        />
        <label v-if="model.selectedProvider" class="agent-session-form-field"
          ><span>使用的 API Key</span
          ><a-select
            v-model="model.apiKeyLocalId"
            aria-label="继续会话使用的 API Key"
            ><a-option value="">使用中转站当前 Key</a-option
            ><a-option
              v-for="key in model.apiKeys"
              :key="key.localId"
              :value="key.localId"
              >{{ keyLabel(key) }}</a-option
            ></a-select
          ></label
        >
      </section>
      <AgentCliTerminalFields
        :agent-kind="row.session.cliKind"
        :label="agentLabel"
        v-model:cli-value="model.cliPath"
        :cli-options="cliChoices"
        v-model:terminal-kind="model.terminalKind"
        :terminal-options="model.terminalOptions"
        :probing="model.probing"
        :error="model.probeError"
        cli-aria-label="继续会话使用的 CLI 路径"
        @refresh="model.refreshEnvironment"
      />
      <label v-if="supportsModelSelection" class="agent-session-form-field">
        <span>本次模型</span>
        <AgentModelSelect
          :value="model.modelOverride || undefined"
          :native-choices="model.nativeModelChoices"
          :catalog="model.modelCatalog"
          :provider-models="
            model.intentKind === 'provider'
              ? (model.selectedProvider?.capabilities.availableModels ?? [])
              : undefined
          "
          :loading="model.modelLoading"
          :default-label="
            row.session.model
              ? `原会话模型：${row.session.model}`
              : '恢复原会话模型'
          "
          :default-description="
            row.session.model
              ? '未指定本次模型时，继续使用原会话的模型。'
              : '当前会话信息没有提供具体模型 ID，继续会话时由 Agent 从原记录恢复。'
          "
          :default-badge="row.session.model ? '已配置' : '原会话'"
          aria-label="选择继续会话的模型"
          @update:value="model.modelOverride = $event ?? ''"
        />
      </label>
      <p v-if="model.modelError" class="agent-workspace-note">
        {{ model.modelError }}
      </p>
      <footer class="agent-session-resume-footer">
        <p role="status">
          {{
            model.unavailableReason ||
            "确认后在后台启动，可在任务中心查看结果。"
          }}
        </p>
        <a-button
          type="primary"
          :disabled="!model.canConfirm"
          @click="model.confirm"
          >确认继续</a-button
        >
      </footer>
    </div>
  </a-modal>
</template>
