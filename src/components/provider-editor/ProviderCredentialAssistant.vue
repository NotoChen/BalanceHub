<script setup lang="ts">
import { computed } from "vue";
import type { ProviderInput } from "../../stores/providers";
import type {
  CredentialCompletionState,
  CredentialCompletionStep,
} from "../../composables/useProviderCredentialCompletion";

const props = defineProps<{
  draft: ProviderInput;
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

const visible = computed(() => props.draft.auth.mode !== "apiKey");

const titleText = computed(() => {
  if (props.saved) return "配置已完成";
  if (props.state === "failed") return "配置未完成";
  if (props.busy) return "正在自动完成配置";
  return "配置助手";
});

const descriptionText = computed(() => {
  if (props.saved) {
    return "已保存本次补全结果，抽屉不会自动关闭。";
  }
  if (props.state === "failed") {
    return props.message || "处理失败，请按失败步骤调整后重试。";
  }
  if (props.draft.auth.mode === "session") {
    if (!props.draft.identity.baseUrl.trim() || !props.draft.auth.sessionCookie.trim()) {
      return "填写中转站地址和会话 Cookie 后，可以解析用户并补全配置。";
    }
    return "已填写会话 Cookie，可以解析用户并补全访问令牌、API 密钥。";
  }
  if (!props.draft.identity.baseUrl.trim() || !props.draft.auth.accessToken.trim() || !props.draft.auth.apiUser.trim()) {
    return "填写中转站地址、访问令牌和 API User ID 后，可以补全 API 密钥。";
  }
  return "已填写访问令牌，可以补全 API 密钥并保存配置。";
});

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
