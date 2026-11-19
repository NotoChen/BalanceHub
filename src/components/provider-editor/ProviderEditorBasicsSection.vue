<script setup lang="ts">
import { computed, ref } from "vue";
import { IconCloud, IconLink, IconRefresh } from "@arco-design/web-vue/es/icon";
import type {
  ProviderInput,
  ProviderProtocol,
  ProviderProtocolDescriptor,
  ProviderProtocolDetectionResult,
  ProviderSiteProbeResult,
} from "../../stores/providers";
import type { ProtocolSelectionSource } from "../../composables/provider-editor-shared";
import { providerProtocolLabel, providerScopeNoun } from "../../utils/provider-protocol";
import {
  applyProviderGatewayPreset,
  gatewayPresetForBaseUrl,
  PROVIDER_GATEWAY_PRESETS,
  type ProviderGatewayPreset,
} from "../../utils/provider-gateway-presets";
import RadioChoiceGroup from "../RadioChoiceGroup.vue";

const props = defineProps<{
  draft: ProviderInput;
  providerProtocols: ProviderProtocolDescriptor[];
  siteProbeResult: ProviderSiteProbeResult | null;
  protocolDetectionResult: ProviderProtocolDetectionResult | null;
  protocolSelectionSource: ProtocolSelectionSource;
  probingSite: boolean;
  siteNameSourceBaseUrl: string;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  "probe-site": [options?: { force?: boolean }];
  "select-protocol": [protocol: ProviderProtocol];
}>();

const normalizedBaseUrl = computed(() => normalizeBaseUrl(props.draft.identity.baseUrl));
const baseUrlTouched = ref(false);
/// 地址在提交前就能判断格式：空值、缺协议、非 HTTP(S) 各有不同提示。只
/// 在用户离开字段后显示，避免新表单一打开就满屏红字。
const baseUrlError = computed(() => {
  if (!baseUrlTouched.value) return "";
  const raw = props.draft.identity.baseUrl.trim();
  if (!raw) return "请填写中转站地址";
  try {
    const url = new URL(raw);
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return "仅支持 http:// 或 https:// 地址";
    }
    if (!url.hostname) return "地址缺少主机名";
  } catch {
    return "地址格式无效，例如 https://relay.example.com";
  }
  return "";
});
const detectionIsCurrent = computed(() =>
  Boolean(props.draft.identity.name.trim() && normalizedBaseUrl.value === props.siteNameSourceBaseUrl),
);
const hostFallback = computed(() => {
  try {
    return new URL(props.draft.identity.baseUrl.trim()).host || "等待识别";
  } catch {
    return props.draft.identity.baseUrl.trim().replace(/^https?:\/\//i, "").split("/")[0] || "等待识别";
  }
});
const detectedName = computed(() => {
  const name = props.draft.identity.name.trim();
  return name && normalizedBaseUrl.value === props.siteNameSourceBaseUrl ? name : hostFallback.value;
});
const detectionLabel = computed(() => {
  if (props.probingSite) return "识别中";
  if (props.protocolDetectionResult?.ambiguous) return "识别冲突 · 手动选择";
  if (props.protocolDetectionResult && !props.protocolDetectionResult.detectedProtocol) {
    return "无法识别 · 手动选择";
  }
  if (
    props.protocolSelectionSource === "manual"
    && props.protocolDetectionResult?.detectedProtocol
    && props.protocolDetectionResult.detectedProtocol !== props.draft.identity.protocol
  ) {
    return `识别为 ${protocolLabel(props.protocolDetectionResult.detectedProtocol)} · 保留手动选择`;
  }
  if (props.protocolDetectionResult?.detectedProtocol) {
    return `已识别 · ${protocolLabel(props.protocolDetectionResult.detectedProtocol)}`;
  }
  if (props.protocolSelectionSource === "saved") {
    return `已保存 · ${protocolLabel(props.draft.identity.protocol)}`;
  }
  if (props.protocolSelectionSource === "manual") {
    return `手动选择 · ${protocolLabel(props.draft.identity.protocol)}`;
  }
  if (detectionIsCurrent.value) return "已识别";
  return "待识别";
});

const protocolOptions = computed(() => props.providerProtocols.filter((descriptor) => descriptor.formCreation).map((descriptor) => ({
  value: descriptor.kind,
  label: descriptor.label,
  // 说明也来自协议目录：选型面板上一句话讲清每种接入是什么，不必先去试。
  description: descriptor.description,
})));

/// 称呼跟着选中的协议走，地址字段和占位示例一起改口。
const scopeNoun = computed(() => providerScopeNoun(props.draft.identity.protocol));
const baseUrlPlaceholder = computed(() =>
  props.draft.identity.protocol === "cliProxyApi"
    ? "http://127.0.0.1:8317"
    : "https://relay.example.com",
);
/// 通用 API Key 只认 OpenAI 兼容地址；给出常见网关入口，省去用户自己拼版本段。
const gatewayPresets = computed(() =>
  props.draft.identity.protocol === "api" ? PROVIDER_GATEWAY_PRESETS : [],
);
const activeGatewayPreset = computed(() =>
  gatewayPresetForBaseUrl(props.draft.identity.baseUrl),
);

function applyGatewayPreset(preset: ProviderGatewayPreset) {
  applyProviderGatewayPreset(props.draft.identity, preset);
  emit("probe-site");
}

function normalizeBaseUrl(value: string) {
  return value.trim().replace(/\/+$/, "");
}

function protocolLabel(protocol: ProviderProtocol) {
  return providerProtocolLabel(props.providerProtocols, protocol);
}
</script>

<template>
  <div class="provider-form-page provider-basics-page">
    <section class="provider-form-block provider-form-block-primary">
      <header class="provider-form-block-header">
        <span class="provider-form-block-icon"><IconCloud /></span>
        <div><strong>连接信息</strong></div>
      </header>
      <div class="provider-form-block-body">
        <a-form-item class="provider-field" label="接入类型">
          <RadioChoiceGroup
            :model-value="draft.identity.protocol"
            :options="protocolOptions"
            :disabled="disabled"
            label="接入类型"
            class="provider-protocol-picker"
            option-class="provider-protocol-option"
            @update:model-value="emit('select-protocol', $event)"
          >
            <template #default="{ option }">
              <strong>{{ option.label }}</strong>
              <span v-if="option.description" class="provider-protocol-option-description">{{ option.description }}</span>
            </template>
          </RadioChoiceGroup>
        </a-form-item>
        <a-form-item class="provider-field" field="identity.baseUrl" :label="`${scopeNoun}地址`" required>
          <a-input
            v-model="draft.identity.baseUrl"
            :placeholder="baseUrlPlaceholder"
            allow-clear
            @blur="baseUrlTouched = true; emit('probe-site')"
          >
            <template #prefix><IconLink /></template>
            <template #suffix>
              <a-tooltip content="重新识别站点">
                <button
                  type="button"
                  class="provider-inline-icon-button"
                  :class="{ spinning: probingSite }"
                  :disabled="disabled || probingSite"
                  aria-label="重新识别站点"
                  @click.stop="emit('probe-site', { force: true })"
                >
                  <IconRefresh />
                </button>
              </a-tooltip>
            </template>
          </a-input>
          <template v-if="baseUrlError || draft.identity.baseUrl.trim()" #extra>
            <span v-if="baseUrlError" class="provider-field-error" role="alert">{{ baseUrlError }}</span>
            <div
              v-if="!baseUrlError"
              class="provider-site-detection"
              :class="{ loading: probingSite, ready: siteProbeResult?.ok, warning: protocolDetectionResult && !protocolDetectionResult.detectedProtocol }"
              role="status"
            >
              <span class="provider-site-detection-copy"><strong>{{ detectedName }}</strong><span>{{ detectionLabel }}</span></span>
              <span v-if="detectionIsCurrent && siteProbeResult?.message" class="provider-site-detection-message">{{ siteProbeResult.message }}</span>
            </div>
            <ul
              v-if="detectionIsCurrent && protocolDetectionResult?.candidates?.length"
              class="provider-site-detection-candidates"
              aria-label="协议识别证据"
            >
              <li
                v-for="candidate in protocolDetectionResult.candidates"
                :key="candidate.protocol"
                :class="{ matched: candidate.ok }"
                :title="candidate.message"
              >
                <strong>{{ candidate.label }}</strong>
                <small>{{ candidate.ok ? (candidate.role === "explicit" ? "已确认" : "匹配") : candidate.message }}</small>
              </li>
            </ul>
          </template>
        </a-form-item>

        <div v-if="gatewayPresets.length" class="provider-gateway-presets" role="group" aria-label="常用网关">
          <span class="provider-gateway-presets-label">常用网关</span>
          <button
            v-for="preset in gatewayPresets"
            :key="preset.label"
            type="button"
            class="provider-gateway-preset"
            :class="{ active: activeGatewayPreset?.label === preset.label }"
            :disabled="disabled"
            @click="applyGatewayPreset(preset)"
          >{{ preset.label }}</button>
        </div>

        <a-form-item class="provider-field" field="identity.remark" label="备注">
          <a-input
            v-model="draft.identity.remark"
            placeholder="可选，例如：Claude 主用、备用站"
            allow-clear
          />
        </a-form-item>
      </div>
    </section>
  </div>
</template>
