<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import SettingsCodexCommandPreview from "./SettingsCodexCommandPreview.vue";
import SettingsCodexPromptSection from "./SettingsCodexPromptSection.vue";
import SettingsCliManager from "./SettingsCliManager.vue";
import SettingsSection from "./SettingsSection.vue";
import { MIN_LIVENESS_INTERVAL_SECONDS } from "../../utils/liveness-defaults";
import {
  codexIntervalModeOptions,
  livenessCliKindOptions,
  temporaryCliTerminalOptionsForPlatform,
} from "../../utils/liveness-options";
import { useLivenessConsent } from "../../composables/useLivenessConsent";
import { useProviderStore } from "../../stores/providers";
import { hostPlatform } from "../../api/app";
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

const { ensureConsent, consented } = useLivenessConsent();
const store = useProviderStore();
const platform = ref("macos");

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

// 授权弹窗承诺「可在设置中重置」，这里就是那个入口。重置后调度器立即停跑，
// 同时把草稿开关拨回关，保存后状态自洽；再次开启时会重新弹授权。
function onRevokeConsent() {
  Modal.confirm({
    title: "重置自动测活授权",
    content:
      "重置后后台自动测活立即停止（手动测活不受影响）；再次开启自动测活开关时会重新弹出授权确认。",
    okText: "重置授权",
    cancelText: "取消",
    onOk: async () => {
      try {
        await store.revokeLivenessCost();
        props.settings.livenessEnabled = false;
        Message.success("已重置授权，自动测活开关已关闭，保存设置后生效");
      } catch (error) {
        Message.error(error instanceof Error ? error.message : String(error));
      }
    },
  });
}

// 修复「开关已开但授权缺失」的卡死态：此状态下自动测活静默不跑，给出重新授权入口。
async function onReconsent() {
  await ensureConsent();
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

const temporaryCliTerminalSelectOptions = computed(() =>
  temporaryCliTerminalOptionsForPlatform(platform.value),
);
const minimumRandomMaxInterval = computed(() =>
  Math.max(MIN_LIVENESS_INTERVAL_SECONDS, Number(props.settings.livenessRandomMinInterval) || 0),
);

onMounted(async () => {
  try {
    platform.value = await hostPlatform();
  } catch {
    platform.value = "macos";
  }
});
</script>

<template>
  <SettingsSection
    title="测活"
    description="通过真实 CLI 调用检测中转站和模型是否可用。"
    :expanded="expanded"
    @toggle="emit('toggle')"
  >
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
    <a-form-item label="临时 CLI 终端">
      <a-select
        v-model="settings.temporaryCliTerminalKind"
        :options="temporaryCliTerminalSelectOptions"
      />
      <template #extra>
        仅影响右键菜单里的临时启动 CLI；自动测活仍在后台静默执行。
      </template>
    </a-form-item>
    <a-form-item
      v-if="settings.temporaryCliTerminalKind === 'custom'"
      label="自定义终端命令"
    >
      <a-input
        v-model="settings.temporaryCliTerminalCommand"
        placeholder="例如：open -a Warp {script}"
        allow-clear
      />
      <template #extra>
        支持 {script} 和 {workdir} 占位符；未写 {script} 时会自动追加脚本路径。
      </template>
    </a-form-item>
    <a-form-item label="自动测活">
      <a-space>
        <a-switch :model-value="settings.livenessEnabled" @change="onToggleAutoLiveness" />
        <a-button v-if="consented" size="mini" @click="onRevokeConsent">重置授权</a-button>
      </a-space>
      <template #extra>
        <span v-if="settings.livenessEnabled && !consented">
          尚未授权消耗真实额度，自动测活不会运行。
          <a-link @click="onReconsent">重新授权</a-link>
        </span>
        <span v-else-if="consented">已授权自动测活消耗真实额度；可随时重置授权。</span>
      </template>
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
        :min="MIN_LIVENESS_INTERVAL_SECONDS"
        :step="1"
      />
    </a-form-item>
    <template v-else>
      <a-form-item label="最短随机周期（秒）">
        <a-input-number
          v-model="settings.livenessRandomMinInterval"
          :min="MIN_LIVENESS_INTERVAL_SECONDS"
          :step="1"
        />
      </a-form-item>
      <a-form-item label="最长随机周期（秒）">
        <a-input-number
          v-model="settings.livenessRandomMaxInterval"
          :min="minimumRandomMaxInterval"
          :step="1"
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
