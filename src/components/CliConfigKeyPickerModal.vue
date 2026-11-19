<script setup lang="ts">
import { computed } from "vue";
import { IconCheck, IconLock } from "@arco-design/web-vue/es/icon";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import type {
  AgentCliKind,
  CliConfigSnapshot,
  Provider,
  ProviderApiKeyOption,
} from "../stores/providers";
import { agentCliLabel } from "../utils/cli-environment";
import {
  formatQuotaValue,
  maskApiKey,
  providerApiKeyDisplayName,
  providerApiKeySecondaryName,
  providerUsesApiKeyOption,
} from "../utils/provider-display";
import AgentCliIcon from "./AgentCliIcon.vue";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  cliKind: AgentCliKind | null;
  keys: ProviderApiKeyOption[];
  currentConfig: CliConfigSnapshot | null;
  /** Recommendation targets a concrete model; that is what this write will bind. */
  model?: string;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  select: [option: ProviderApiKeyOption];
}>();

const store = useCliRuntimeStore();
const cliLabel = computed(() =>
  props.cliKind ? agentCliLabel(store.cliEnvironmentProbe, props.cliKind) : "Agent CLI",
);
function keyDisplay(option: ProviderApiKeyOption) {
  return option.maskedKey?.trim() || maskApiKey(option.key) || "完整 Key 不可读取";
}

function isDefault(option: ProviderApiKeyOption) {
  return props.provider ? providerUsesApiKeyOption(props.provider, option) : false;
}

/// 选 Key 时真正影响这次写入的事实：Key 的分组、模型限制和剩余额度。没有
/// 远端额度的本地 Key 不写“剩余 0”，那会被读成额度已经用完。
function keyFacts(option: ProviderApiKeyOption) {
  const facts: string[] = [];
  if (option.group?.trim()) facts.push(`分组 ${option.group.trim()}`);
  if (option.modelLimitsEnabled) {
    facts.push(option.modelLimits.length
      ? `限 ${option.modelLimits.length} 个模型`
      : "已启用模型限制");
  }
  if (option.tokenId?.trim()) {
    facts.push(option.unlimitedQuota
      ? "无限额度"
      : formatQuotaValue(option.remainQuota || 0, {
          quotaDisplayType: option.quotaDisplayType || "currency",
          currencySymbol: option.currencySymbol || "$",
        }));
  }
  return facts.join(" · ");
}

function isCurrentAgentKey(option: ProviderApiKeyOption) {
  const current = props.currentConfig;
  if (!current || current.providerId !== props.provider?.identity.id) return false;
  const localId = current.apiKeyLocalId?.trim() || "";
  if (localId) return localId === option.localId.trim();
  return isDefault(option);
}
</script>

<template>
  <a-modal
    :visible="visible"
    :footer="false"
    :width="620"
    modal-class="surface-modal cli-config-key-picker-modal"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title cli-config-key-picker-title">
        <span class="surface-modal-title-icon">
          <AgentCliIcon v-if="cliKind" :kind="cliKind" :size="18" />
          <IconLock v-else />
        </span>
        <span class="surface-modal-title-copy"><strong>选择 {{ cliLabel }} 使用的 API Key</strong></span>
        <span class="surface-modal-title-meta">{{ keys.length }} 个可用</span>
      </div>
    </template>

    <div class="cli-config-key-picker">
      <header>
        <span>目标中转站</span>
        <strong>{{ provider?.displayLabel || provider?.identity.name }}</strong>
        <span v-if="model?.trim()" class="cli-config-key-target-model">
          写入模型 <code>{{ model.trim() }}</code>
        </span>
      </header>
      <div class="cli-config-key-options" role="listbox" :aria-label="`${cliLabel} API Key`">
        <button
          v-for="option in keys"
          :key="option.localId || option.tokenId || option.key"
          type="button"
          class="cli-config-key-option"
          :class="{ current: isCurrentAgentKey(option) }"
          role="option"
          :aria-selected="isCurrentAgentKey(option)"
          @click="emit('select', option)"
        >
          <span class="cli-config-key-option-marker">
            <IconCheck v-if="isCurrentAgentKey(option)" />
            <IconLock v-else />
          </span>
          <span class="cli-config-key-option-copy">
            <span class="cli-config-key-option-title">
              <strong>{{ providerApiKeyDisplayName(option) }}</strong>
              <small v-if="isDefault(option)" class="cli-config-key-default-badge">卡片当前调用</small>
              <small v-if="isCurrentAgentKey(option)" class="cli-config-key-current-badge">当前使用</small>
            </span>
            <small v-if="providerApiKeySecondaryName(option)">
              站点名称：{{ providerApiKeySecondaryName(option) }}
            </small>
            <small v-if="keyFacts(option)" class="cli-config-key-option-facts">
              {{ keyFacts(option) }}
            </small>
            <code>{{ keyDisplay(option) }}</code>
          </span>
          <span class="cli-config-key-option-action">
            {{ isCurrentAgentKey(option) ? "当前使用 · 查看并编辑配置" : "选择并预览 Diff" }}
          </span>
        </button>
      </div>
    </div>
  </a-modal>
</template>
