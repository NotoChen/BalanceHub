<script setup lang="ts">
import { computed } from "vue";
import { IconCloud, IconLink, IconRefresh } from "@arco-design/web-vue/es/icon";
import type {
  ProviderInput,
  ProviderProtocol,
  ProviderProtocolDescriptor,
  ProviderProtocolDetectionResult,
  ProviderSiteProbeResult,
} from "../../stores/providers";
import type { ProtocolSelectionSource } from "../../composables/provider-editor-shared";
import { providerProtocolLabel } from "../../utils/provider-protocol";
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

const protocolOptions = computed(() => props.providerProtocols.map((descriptor) => ({
  value: descriptor.kind,
  label: descriptor.label,
  description: descriptor.description,
})));
const selectedProtocolDescription = computed(() =>
  protocolOptions.value.find((option) => option.value === props.draft.identity.protocol)?.description,
);

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
        <a-form-item class="provider-field" label="中转站类型">
          <RadioChoiceGroup
            :model-value="draft.identity.protocol"
            :options="protocolOptions"
            :disabled="disabled"
            label="中转站类型"
            class="provider-protocol-picker"
            option-class="provider-protocol-option"
            @update:model-value="emit('select-protocol', $event)"
          >
            <template #default="{ option }">
              <strong>{{ option.label }}</strong>
            </template>
          </RadioChoiceGroup>
          <p v-if="selectedProtocolDescription" class="provider-protocol-hint">
            {{ selectedProtocolDescription }}
          </p>
        </a-form-item>
        <a-form-item class="provider-field" field="identity.baseUrl" label="中转站地址" required>
          <a-input
            v-model="draft.identity.baseUrl"
            placeholder="https://relay.example.com"
            allow-clear
            @blur="emit('probe-site')"
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
          <template v-if="draft.identity.baseUrl.trim()" #extra>
            <div
              class="provider-site-detection"
              :class="{ loading: probingSite, ready: siteProbeResult?.ok, warning: protocolDetectionResult && !protocolDetectionResult.detectedProtocol }"
              role="status"
            >
              <span class="provider-site-detection-copy"><strong>{{ detectedName }}</strong><span>{{ detectionLabel }}</span></span>
              <span v-if="detectionIsCurrent && siteProbeResult?.message" class="provider-site-detection-message">{{ siteProbeResult.message }}</span>
            </div>
          </template>
        </a-form-item>

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
