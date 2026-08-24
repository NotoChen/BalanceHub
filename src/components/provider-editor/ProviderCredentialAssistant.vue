<script setup lang="ts">
import { computed } from "vue";
import type { ProviderInput, ProviderProtocolDescriptor } from "../../stores/providers";
import {
  providerAuthModeDescriptor,
  providerProtocolDescriptor,
} from "../../utils/provider-protocol";
import type {
  CredentialCompletionState,
  CredentialCompletionStep,
} from "../../composables/useProviderCredentialCompletion";

const props = defineProps<{
  draft: ProviderInput;
  providerProtocols: ProviderProtocolDescriptor[];
  state: CredentialCompletionState;
  steps: CredentialCompletionStep[];
  message: string;
  busy: boolean;
  canRun: boolean;
  saved: boolean;
}>();

const emit = defineEmits<{
  run: [];
}>();

const currentProtocol = computed(() =>
  providerProtocolDescriptor(props.providerProtocols, props.draft.identity.protocol),
);

const currentAuthMode = computed(() =>
  providerAuthModeDescriptor(
    props.providerProtocols,
    props.draft.identity.protocol,
    props.draft.auth.mode,
  ),
);

const visible = computed(() =>
  props.draft.auth.mode !== "apiKey" && currentProtocol.value?.credentialAssistant.enabled === true,
);

const titleText = computed(() => {
  if (props.saved) return "配置已完成";
  if (props.state === "needApiKeySelection") return "选择当前调用 API Key";
  if (props.state === "failed") return "配置未完成";
  if (props.busy) return "正在自动完成配置";
  return "配置助手";
});

const descriptionText = computed(() => {
  if (props.saved) {
    return "已保存本次补全结果，可继续调整运行策略。";
  }
  if (props.state === "needApiKeySelection") {
    return "已同步多个 API Key，请在凭据列表中选择本卡片用于默认请求的 Key。";
  }
  if (props.state === "failed") {
    return props.message || "处理失败，请按失败步骤调整后重试。";
  }
  const schema = currentAuthMode.value;
  if (!schema) return "认证 Schema 尚未加载，请重新打开编辑窗口。";
  const missing = [];
  if (!props.draft.identity.baseUrl.trim()) {
    missing.push("中转站地址");
  }
  for (const field of schema.requiredFields) {
    if (!authFieldValue(field).trim()) {
      missing.push(schema.fields.find((candidate) => candidate.field === field)?.label || field);
    }
  }
  if (missing.length > 0) {
    return `填写${missing.join("、")}后，可以自动补全配置。`;
  }
  const targets = [];
  if (currentProtocol.value?.capabilities.accessToken) targets.push("访问令牌");
  if (currentProtocol.value?.capabilities.apiKeyManagement) targets.push("API Key");
  const targetText = targets.length > 0 ? `，并同步${targets.join("和")}` : "";
  return `所需信息已填写，将${schema.description}${targetText}。`;
});

function authFieldValue(field: string) {
  const value = props.draft.auth[field as keyof ProviderInput["auth"]];
  return typeof value === "string" ? value : "";
}

const actionText = computed(() => {
  if (props.state === "failed") return "重新尝试";
  if (props.saved) return "重新自动完成";
  return "自动完成配置";
});

function stepStatusLabel(status: CredentialCompletionStep["status"]) {
  const labels: Record<CredentialCompletionStep["status"], string> = {
    pending: "等待",
    running: "进行中",
    done: "完成",
    error: "失败",
    skipped: "跳过",
  };
  return labels[status];
}
</script>

<template>
  <section v-if="visible" class="credential-completion-panel">
    <div class="credential-completion-header">
      <div>
        <h3>{{ titleText }}</h3>
        <p>{{ descriptionText }}</p>
      </div>
      <a-button
        type="primary"
        size="small"
        :loading="busy"
        :disabled="!canRun"
        @click="emit('run')"
      >
        {{ actionText }}
      </a-button>
    </div>

    <ul v-if="steps.length > 0" class="credential-completion-steps">
      <li
        v-for="step in steps"
        :key="step.key"
        :class="`credential-step-${step.status}`"
      >
        <b>{{ stepStatusLabel(step.status) }}</b>
        <div>
          <strong>{{ step.name }}</strong>
          <span>{{ step.message }}</span>
        </div>
      </li>
    </ul>
  </section>
</template>
