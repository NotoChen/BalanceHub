<script setup lang="ts">
import { computed } from "vue";
import {
  IconApps,
  IconCalendar,
  IconClockCircle,
  IconExperiment,
  IconLink,
  IconRefresh,
  IconSafe,
} from "@arco-design/web-vue/es/icon";
import type { AuthMode, Provider } from "../stores/providers";
import { providerAgentBaseUrl } from "../utils/cli-environment";
import { providerAuthModeLabel, providerProtocolLabel } from "../utils/provider-display";

type ProbeStepKey = "checkIn" | "apiKeys" | "invitation" | "models";
type ProbeStepStatus = "running" | "supported" | "unsupported" | "skipped" | "error" | "pending";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  running: boolean;
  error: string;
  resultMessage: string;
  startedAt: number | null;
  finishedAt: number | null;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  retry: [];
}>();

const authModeLabels: Record<AuthMode, string> = {
  session: "Cookie",
  accessToken: "访问令牌",
  apiKey: "API Key",
  password: "账号密码",
};

const modalTitle = computed(() =>
  props.provider ? `${props.provider.identity.name} · 站点能力探测` : "站点能力探测",
);

const probeFinished = computed(() => !props.running && props.finishedAt !== null);
const partialError = computed(() => props.provider?.capabilities.errorMessage?.trim() || "");
const scopedErrors = computed(() => parseScopedErrors(partialError.value));
const unscopedPartialError = computed(() =>
  partialError.value && scopedErrors.value.size === 0 ? partialError.value : "",
);
const overallTone = computed(() => {
  if (props.running) return "running";
  if (props.error) return "error";
  if (partialError.value) return "partial";
  if (probeFinished.value) return "success";
  return "pending";
});

const overallLabel = computed(() => {
  if (props.running) return "正在探测";
  if (props.error) return "探测失败";
  if (partialError.value) return "部分完成";
  if (probeFinished.value) return "探测完成";
  return "等待探测";
});

const steps = computed(() => {
  const provider = props.provider;
  if (!provider) return [];
  return [
    makeStep("checkIn", "签到能力", IconCalendar, checkInMethod(provider)),
    makeStep("apiKeys", "密钥管理", IconSafe, apiKeyMethod(provider)),
    makeStep("invitation", "邀请能力", IconLink, invitationMethod(provider)),
    makeStep("models", "模型列表", IconApps, modelMethod(provider)),
  ];
});

function makeStep(key: ProbeStepKey, label: string, icon: typeof IconCalendar, method: string) {
  const status = stepStatus(key);
  return {
    key,
    label,
    icon,
    method,
    status,
    statusLabel: stepStatusLabel(status),
    detail: stepDetail(key, status),
  };
}

function stepStatus(key: ProbeStepKey): ProbeStepStatus {
  const provider = props.provider;
  if (!provider) return "pending";
  if (isSkipped(provider, key)) return "skipped";
  if (props.running) return "running";
  if (props.error) return "error";
  if (stepError(key)) return "error";
  if (!probeFinished.value) return "pending";

  const capabilities = provider.capabilities;
  if (key === "checkIn") {
    return capabilities.checkInKnown && capabilities.checkInSupported ? "supported" : "unsupported";
  }
  if (key === "apiKeys") {
    return capabilities.apiKeyManagementKnown && capabilities.apiKeyManagementSupported
      ? "supported"
      : "unsupported";
  }
  if (key === "invitation") {
    return capabilities.invitationKnown && capabilities.invitationSupported
      ? "supported"
      : "unsupported";
  }
  return capabilities.availableModels.length > 0 ? "supported" : "unsupported";
}

function isSkipped(provider: Provider, key: ProbeStepKey) {
  if (key === "models") return !provider.auth.apiKey.trim();
  if (provider.identity.protocol === "api") return true;
  return provider.auth.mode === "apiKey";
}

function stepStatusLabel(status: ProbeStepStatus) {
  if (status === "running") return "请求中";
  if (status === "supported") return "支持";
  if (status === "unsupported") return "不支持";
  if (status === "skipped") return "已跳过";
  if (status === "error") return "失败";
  return "等待";
}

function stepDetail(key: ProbeStepKey, status: ProbeStepStatus) {
  const provider = props.provider;
  if (!provider) return "等待选择中转站";
  if (status === "running") return "正在由本地后端使用当前认证信息请求站点";
  if (status === "error") return stepError(key) || props.error || "探测请求未完成";
  if (status === "pending") return "尚未开始";
  if (status === "skipped") {
    if (key === "models") return "当前没有 API Key，未请求 OpenAI 兼容模型列表";
    if (provider.identity.protocol === "api") return "通用 API 协议不提供账号级能力";
    return "API Key 认证仅具备 Key 维度能力，不探测账号功能";
  }

  const capabilities = provider.capabilities;
  if (key === "checkIn") {
    if (!capabilities.checkInSupported) return "站点接口未确认可用的签到方式";
    const modes = capabilities.checkInAuthModes.map((mode) => authModeLabels[mode]).join("、");
    return modes ? `可用认证方式：${modes}` : "站点支持签到";
  }
  if (key === "apiKeys") {
    return capabilities.apiKeyManagementSupported
      ? "当前账号可以读取、创建和删除 API Key"
      : "当前账号或站点接口不支持密钥管理";
  }
  if (key === "invitation") {
    return capabilities.invitationSupported
      ? "已确认邀请信息接口可用"
      : "未读取到可用的邀请信息";
  }
  const count = capabilities.availableModels.length;
  return count > 0 ? `已读取 ${count} 个可用模型` : "模型接口未返回可用模型";
}

function stepError(key: ProbeStepKey) {
  if (props.error) return props.error;
  if (unscopedPartialError.value) return unscopedPartialError.value;
  return scopedErrors.value.get(key) || "";
}

function parseScopedErrors(message: string) {
  const errors = new Map<ProbeStepKey, string>();
  const prefixes: Array<[string, ProbeStepKey]> = [
    ["签到能力:", "checkIn"],
    ["密钥管理:", "apiKeys"],
    ["邀请链接:", "invitation"],
    ["模型列表:", "models"],
  ];
  for (const part of message.split("；").map((item) => item.trim()).filter(Boolean)) {
    const matched = prefixes.find(([prefix]) => part.startsWith(prefix));
    if (!matched) continue;
    errors.set(matched[1], part.slice(matched[0].length).trim() || part);
  }
  return errors;
}

function checkInMethod(provider: Provider) {
  if (provider.identity.protocol === "newApi") return "GET /api/user/checkin?month=YYYY-MM";
  if (provider.identity.protocol === "sub2Api") return "按 Sub2API 协议声明判定";
  return "通用 API 协议声明";
}

function apiKeyMethod(provider: Provider) {
  if (provider.identity.protocol === "newApi") return "GET /api/token/";
  if (provider.identity.protocol === "sub2Api") return "Sub2API 密钥列表接口";
  return "通用 API 协议声明";
}

function invitationMethod(provider: Provider) {
  if (provider.identity.protocol === "newApi") return "GET /api/user/aff";
  if (provider.identity.protocol === "sub2Api") return "Sub2API 邀请信息接口";
  return "通用 API 协议声明";
}

function modelMethod(provider: Provider) {
  const baseUrl = providerAgentBaseUrl(provider, "codex");
  return baseUrl ? "GET OpenAI 兼容 /models" : "OpenAI 兼容模型接口";
}

function formatTime(value: number | null) {
  if (!value) return "-";
  return new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(value));
}

function durationLabel() {
  if (!props.startedAt || !props.finishedAt) return "";
  const duration = Math.max(0, props.finishedAt - props.startedAt);
  return duration < 1000 ? `${duration} ms` : `${(duration / 1000).toFixed(1)} s`;
}
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal capability-probe-modal"
    :footer="false"
    :width="760"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title capability-probe-title">
        <span class="surface-modal-title-icon"><icon-experiment /></span>
        <span class="surface-modal-title-copy">
          <strong>{{ modalTitle }}</strong>
        </span>
        <span class="surface-modal-title-meta" :class="`is-${overallTone}`">{{ overallLabel }}</span>
      </div>
    </template>

    <div v-if="provider" class="capability-probe-panel">
      <header class="capability-probe-summary" :class="`is-${overallTone}`">
        <div class="capability-probe-summary-main">
          <span class="capability-probe-status-dot" aria-hidden="true" />
          <div>
            <strong>{{ overallLabel }}</strong>
            <span v-if="running">后端正在检查站点能力和模型接口</span>
            <span v-else-if="error">{{ error }}</span>
            <span v-else>{{ resultMessage || partialError || "探测结果已写入当前中转站" }}</span>
          </div>
        </div>
        <a-tooltip content="重新探测">
          <a-button
            shape="circle"
            :loading="running"
            :disabled="running"
            aria-label="重新探测站点能力"
            @click="emit('retry')"
          >
            <template #icon><icon-refresh /></template>
          </a-button>
        </a-tooltip>
      </header>

      <div class="capability-probe-progress" :class="{ running, complete: probeFinished && !error }" aria-hidden="true">
        <span />
      </div>

      <section class="capability-probe-context" aria-label="探测上下文">
        <div>
          <span>协议</span>
          <strong>{{ providerProtocolLabel(provider.identity.protocol) }}</strong>
        </div>
        <div>
          <span>认证</span>
          <strong>{{ providerAuthModeLabel(provider) }}</strong>
        </div>
        <div>
          <span>开始</span>
          <strong>{{ formatTime(startedAt) }}</strong>
        </div>
        <div>
          <span>耗时</span>
          <strong>{{ durationLabel() || (running ? "计算中" : "-") }}</strong>
        </div>
      </section>

      <section class="capability-probe-steps" aria-label="探测项目">
        <article v-for="step in steps" :key="step.key" class="capability-probe-step" :class="`is-${step.status}`">
          <span class="capability-probe-step-icon"><component :is="step.icon" /></span>
          <div class="capability-probe-step-copy">
            <div>
              <strong>{{ step.label }}</strong>
              <span class="capability-probe-step-state">{{ step.statusLabel }}</span>
            </div>
            <p>{{ step.detail }}</p>
            <code>{{ step.method }}</code>
          </div>
        </article>
      </section>

      <footer class="capability-probe-footnote">
        <icon-clock-circle />
        <span>所有请求均由本地 Tauri 后端发起，弹窗不会展示 Cookie、访问令牌或 API Key。</span>
      </footer>
    </div>

    <div v-else class="capability-probe-panel">
      <header class="capability-probe-summary is-error">
        <div class="capability-probe-summary-main">
          <span class="capability-probe-status-dot" aria-hidden="true" />
          <div>
            <strong>探测已停止</strong>
            <span>{{ error || "中转站已不存在，本次探测结果未写入" }}</span>
          </div>
        </div>
      </header>
    </div>
  </a-modal>
</template>
