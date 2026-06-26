<script setup lang="ts">
import { computed } from "vue";
import SettingsCodexCommandPreview from "./SettingsCodexCommandPreview.vue";
import SettingsCodexPromptSection from "./SettingsCodexPromptSection.vue";
import SettingsCliManager from "./SettingsCliManager.vue";
import SettingsSection from "./SettingsSection.vue";
import { MIN_CODEX_LIVENESS_INTERVAL } from "../../utils/liveness-defaults";
import {
  codexIntervalModeOptions,
  livenessCliKindOptions,
  livenessHttpProtocolOptions,
  livenessMethodOptions,
} from "../../utils/liveness-options";
import { useLivenessConsent } from "../../composables/useLivenessConsent";
import type { AppSettings } from "../../stores/providers";

interface ModelProviderIndexItem {
  model: string;
  providers: { id: string; name: string }[];
}

const props = defineProps<{
  settings: AppSettings;
  expanded: boolean;
  livenessModelOptions: string[];
  modelProviderIndex: ModelProviderIndexItem[];
}>();

const emit = defineEmits<{
  toggle: [];
  "probe-codex-cli": [];
}>();

const { ensureConsent } = useLivenessConsent();

// 开启自动测活前先取得「会消耗真实额度」的一次性授权；取消则开关回弹。
async function onToggleAutoLiveness(value: boolean | string | number) {
  if (!value) {
    props.settings.livenessEnabled = false;
    return;
  }
  if (await ensureConsent()) {
    props.settings.livenessEnabled = true;
  }
}

const codexModelSelectOptions = computed(() =>
  Array.from(
    new Set(
      [
        props.settings.livenessModel.trim(),
        ...props.livenessModelOptions.map((model) => model.trim()),
      ].filter(Boolean),
    ),
  ).map((model) => ({ label: model, value: model })),
);

const selectedLivenessModelProviders = computed(() => {
  const model = props.settings.livenessModel.trim();
  if (!model) return [];
  return props.modelProviderIndex.find((item) => item.model === model)?.providers ?? [];
});
</script>

<template>
  <SettingsSection
    title="测活"
    description="通过真实 CLI 调用检测中转站和模型是否可用。"
    :expanded="expanded"
    @toggle="emit('toggle')"
  >
    <a-form-item label="测活方式">
      <a-select v-model="settings.livenessMethod" :options="livenessMethodOptions" />
      <template #extra>
        CLI 走本地 codex/claude 命令；HTTP 直接调用中转站接口（更轻量、跨机一致）。
      </template>
    </a-form-item>
    <template v-if="settings.livenessMethod === 'cli'">
      <a-form-item label="测活 CLI">
        <a-select v-model="settings.livenessCliKind" :options="livenessCliKindOptions" />
        <template #extra>
          测活通过真实 CLI 作为调用媒介；Claude Code 的 Anthropic Base URL 不会自动追加 /v1。
        </template>
      </a-form-item>
      <a-form-item label="CLI 管理">
        <SettingsCliManager :settings="settings" />
        <template #extra>
          codex 与 claude 各自的路径与状态；留空自动查找，可「浏览…」选择文件或「重新扫描」。
        </template>
      </a-form-item>
    </template>
    <a-form-item v-else label="HTTP 协议">
      <a-select v-model="settings.livenessHttpProtocol" :options="livenessHttpProtocolOptions" />
      <template #extra>
        OpenAI 用 Authorization: Bearer 调 /v1/chat/completions 或 /v1/responses；Anthropic 用 x-api-key + anthropic-version 调 /v1/messages。
      </template>
    </a-form-item>
    <a-form-item label="自动测活">
      <a-switch :model-value="settings.livenessEnabled" @change="onToggleAutoLiveness" />
    </a-form-item>
    <a-form-item label="默认模型">
      <a-select
        v-model="settings.livenessModel"
        :options="codexModelSelectOptions"
        allow-create
        allow-search
        placeholder="选择或输入模型"
      />
      <template #extra>
        <div v-if="selectedLivenessModelProviders.length > 0" class="model-support-tags">
          <span>支持该模型的中转站</span>
          <a-tag
            v-for="provider in selectedLivenessModelProviders"
            :key="`${settings.livenessModel}-${provider.id}`"
            color="blue"
          >
            {{ provider.name }}
          </a-tag>
        </div>
        <span v-else>尚未从中转站同步到该模型，可先在中转站编辑中获取模型列表。</span>
      </template>
    </a-form-item>
    <a-form-item label="周期策略">
      <a-select
        v-model="settings.livenessIntervalMode"
        :options="codexIntervalModeOptions"
      />
    </a-form-item>
    <a-form-item
      v-if="settings.livenessIntervalMode === 'fixed'"
      label="固定周期（秒）"
    >
      <a-input-number
        v-model="settings.livenessInterval"
        :min="MIN_CODEX_LIVENESS_INTERVAL"
        :step="60"
      />
      <template #extra>自动测活周期最低 45 分钟。</template>
    </a-form-item>
    <template v-else>
      <a-form-item label="最短随机周期（秒）">
        <a-input-number
          v-model="settings.livenessRandomMinInterval"
          :min="MIN_CODEX_LIVENESS_INTERVAL"
          :step="60"
        />
        <template #extra>自动测活周期最低 45 分钟。</template>
      </a-form-item>
      <a-form-item label="最长随机周期（秒）">
        <a-input-number
          v-model="settings.livenessRandomMaxInterval"
          :min="MIN_CODEX_LIVENESS_INTERVAL"
          :step="60"
        />
      </a-form-item>
    </template>
    <a-form-item label="超时（秒）">
      <a-input-number v-model="settings.livenessTimeout" :min="10" :max="300" :step="5" />
    </a-form-item>
    <SettingsCodexPromptSection :settings="settings" />
    <SettingsCodexCommandPreview :settings="settings" />
  </SettingsSection>
</template>
