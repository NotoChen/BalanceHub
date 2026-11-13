<script setup lang="ts">
import { computed } from "vue";
import {
  IconApps,
  IconBarChart,
  IconCalendar,
  IconClockCircle,
  IconExperiment,
  IconFile,
  IconLink,
  IconNotification,
  IconRefresh,
  IconSafe,
  IconSubscribe,
  IconTag,
  IconUserGroup,
} from "@arco-design/web-vue/es/icon";
import type {
  AuthMode,
  Provider,
  ProviderOptionalCapability,
  ProviderProtocolDescriptor,
} from "../stores/providers";
import {
  providerAuthModeDescriptor,
  providerProtocolDescriptor,
} from "../utils/provider-protocol";
import { providerDisplayLabel, providerModelScopeLabel } from "../utils/provider-display";

type ProbeStepKey =
  | "checkIn"
  | "apiKeys"
  | "invitation"
  | "pricing"
  | "models"
  | "usage"
  | "requestLogs"
  | "announcements"
  | "subscription"
  | "groups";
type ProbeStepStatus =
  | "running"
  | "supported"
  | "unsupported"
  | "skipped"
  | "unknown"
  | "error"
  | "pending";

const OPTIONAL_STEPS: Array<{
  key: ProbeStepKey;
  capability: ProviderOptionalCapability;
  label: string;
  icon: typeof IconCalendar;
}> = [
  { key: "pricing", capability: "pricing", label: "站点定价", icon: IconTag },
  { key: "usage", capability: "usage", label: "用量趋势", icon: IconBarChart },
  { key: "requestLogs", capability: "requestLogs", label: "请求日志", icon: IconFile },
  { key: "announcements", capability: "announcements", label: "站点公告", icon: IconNotification },
  { key: "subscription", capability: "subscription", label: "订阅额度", icon: IconSubscribe },
  { key: "groups", capability: "groups", label: "站点分组", icon: IconUserGroup },
];

/// 模型列表既有独立的同步动作，也由能力探测记录结论，因此和上面六个接口分开
/// 列出，但读取同一份 `capabilities.optional`。
const OPTIONAL_CAPABILITY_BY_STEP: Partial<Record<ProbeStepKey, ProviderOptionalCapability>> = {
  pricing: "pricing",
  usage: "usage",
  requestLogs: "requestLogs",
  announcements: "announcements",
  subscription: "subscription",
  groups: "groups",
  models: "models",
};

const props = defineProps<{
  visible: boolean;
  providerProtocols: ProviderProtocolDescriptor[];
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

const modalTitle = computed(() =>
  props.provider ? `${providerDisplayLabel(props.provider)} · 站点能力探测` : "站点能力探测",
);

const probeFinished = computed(() => !props.running && props.finishedAt !== null);
const protocolDescriptor = computed(() =>
  props.provider
    ? providerProtocolDescriptor(props.providerProtocols, props.provider.identity.protocol)
    : undefined,
);
const capabilityError = computed(() => props.provider?.capabilities.errorMessage?.trim() || "");
const modelError = computed(() => props.provider?.capabilities.availableModelsState.error || "");
const optionalProbeErrors = computed(() =>
  (props.provider?.capabilities.optional ?? [])
    .filter((state) => !state.known && state.error)
    .map((state) => state.error?.trim() || ""),
);
const partialError = computed(
  () => capabilityError.value || modelError.value || optionalProbeErrors.value[0] || "",
);
const scopedErrors = computed(() => parseScopedErrors(capabilityError.value));
const unscopedPartialError = computed(() =>
  capabilityError.value && scopedErrors.value.size === 0 ? capabilityError.value : "",
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
    makeStep("checkIn", "签到能力", IconCalendar, checkInMethod()),
    makeStep("apiKeys", "密钥管理", IconSafe, apiKeyMethod()),
    makeStep("invitation", "邀请能力", IconLink, invitationMethod()),
    ...OPTIONAL_STEPS.map((step) =>
      makeStep(step.key, step.label, step.icon, optionalMethod(step.key)),
    ),
    makeStep("models", "模型列表", IconApps, modelMethod()),
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

function optionalState(key: ProbeStepKey) {
  const capability = OPTIONAL_CAPABILITY_BY_STEP[key];
  if (!capability) return undefined;
  return props.provider?.capabilities.optional?.find(
    (state) => state.capability === capability,
  );
}

function isOptionalKey(key: ProbeStepKey) {
  return OPTIONAL_CAPABILITY_BY_STEP[key] !== undefined;
}

function stepStatus(key: ProbeStepKey): ProbeStepStatus {
  const provider = props.provider;
  if (!provider) return "pending";
  if (isSkipped(provider, key)) return "skipped";
  if (props.running) return "running";
  if (props.error) return "error";
  if (!probeFinished.value) return "pending";

  // 可选接口有明确的探测结论时优先使用；未知状态显示为「未确认」而不是
  // 「失败」，避免把一次认证/网络错误当成站点不支持。
  const optional = optionalState(key);
  if (optional) {
    if (optional.known) return optional.supported ? "supported" : "unsupported";
    return "unknown";
  }
  if (stepError(key)) return "error";

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
  if (key === "models") {
    return capabilities.availableModelsState.updatedAt ? "supported" : "unsupported";
  }
  return "unknown";
}

function isSkipped(provider: Provider, key: ProbeStepKey) {
  if (key === "models") {
    return !provider.actions.models.canSync && !optionalState(key);
  }
  if (isOptionalKey(key)) return !optionalState(key) && !optionalOperationMethod(key);
  return provider.auth.mode === "apiKey" || protocolDescriptor.value?.capabilities.account === false;
}

function stepStatusLabel(status: ProbeStepStatus) {
  if (status === "running") return "请求中";
  if (status === "supported") return "支持";
  if (status === "unsupported") return "不支持";
  if (status === "skipped") return "已跳过";
  if (status === "unknown") return "未确认";
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
    if (key === "models") return provider.actions.models.unavailableReason || "当前认证信息不支持读取模型";
    if (isOptionalKey(key)) return "当前协议未声明该站点接口";
    if (protocolDescriptor.value?.capabilities.account === false) {
      return "当前协议不提供账号级能力";
    }
    return "API Key 认证仅具备 Key 维度能力，不探测账号功能";
  }

  const capabilities = provider.capabilities;
  if (key === "models" && status === "supported") {
    const count = capabilities.availableModels.length;
    if (capabilities.availableModelsState.fallbackReason) {
      return `Key 接口失败，已读取 ${count} 个账号可用模型：${capabilities.availableModelsState.fallbackReason}`;
    }
    return count > 0
      ? `已读取 ${count} 个${providerModelScopeLabel(capabilities.availableModelsState.scope)}`
      : "模型接口已正常响应，当前列表为空";
  }
  const optional = optionalState(key);
  if (optional) {
    if (status === "unknown") {
      return optional.error || optional.reason || "探测没有得到明确结论";
    }
    if (status === "unsupported") {
      return optional.reason || optional.error || "站点接口不可用";
    }
    return optional.reason || "接口可用";
  }
  if (key === "checkIn") {
    if (!capabilities.checkInSupported) return "站点接口未确认可用的签到方式";
    const modes = capabilities.checkInAuthModes
      .map((mode) => authModeLabel(mode))
      .join("、");
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
  return "未读取到该接口的探测结果";
}

function stepError(key: ProbeStepKey) {
  if (props.error) return props.error;
  const optional = optionalState(key);
  if (optional?.error) return optional.error;
  if (key === "models") return modelError.value;
  if (unscopedPartialError.value) return unscopedPartialError.value;
  return scopedErrors.value.get(key) || "";
}

function parseScopedErrors(message: string) {
  const errors = new Map<ProbeStepKey, string>();
  const prefixes: Array<[string, ProbeStepKey]> = [
    ["签到能力:", "checkIn"],
    ["密钥管理:", "apiKeys"],
    ["邀请链接:", "invitation"],
  ];
  for (const part of message.split("；").map((item) => item.trim()).filter(Boolean)) {
    const matched = prefixes.find(([prefix]) => part.startsWith(prefix));
    if (!matched) continue;
    errors.set(matched[1], part.slice(matched[0].length).trim() || part);
  }
  return errors;
}

function authModeLabel(mode: AuthMode) {
  if (!props.provider) return mode;
  return providerAuthModeDescriptor(props.providerProtocols, props.provider.identity.protocol, mode)?.label || mode;
}

function checkInMethod() {
  return protocolDescriptor.value?.operationMethods.checkIn || "协议未声明签到接口";
}

function apiKeyMethod() {
  return protocolDescriptor.value?.operationMethods.apiKeys || "协议未声明密钥管理接口";
}

function invitationMethod() {
  return protocolDescriptor.value?.operationMethods.invitation || "协议未声明邀请接口";
}

function optionalOperationMethod(key: ProbeStepKey) {
  const methods = protocolDescriptor.value?.operationMethods;
  if (!methods) return "";
  if (key === "pricing") return methods.pricing || "";
  if (key === "usage") return methods.usage || "";
  if (key === "requestLogs") return methods.requestLogs || "";
  if (key === "announcements") return methods.announcements || "";
  if (key === "subscription") return methods.subscription || "";
  if (key === "groups") return methods.groups || "";
  return "";
}

function optionalMethod(key: ProbeStepKey) {
  return optionalOperationMethod(key) || "协议未声明该站点接口";
}

function modelMethod() {
  return providerModelScopeLabel(props.provider?.capabilities.availableModelsState.scope ?? props.provider?.actions.models.scope);
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
          <strong>{{ protocolDescriptor?.label || provider.identity.protocol }}</strong>
        </div>
        <div>
          <span>认证</span>
          <strong>{{ authModeLabel(provider.auth.mode) }}</strong>
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
