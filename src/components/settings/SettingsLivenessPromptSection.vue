<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { Message } from "@arco-design/web-vue";
import { previewLivenessPrompts } from "../../api/app";
import { confirmAction } from "../../composables/provider-credential-dialogs";
import { useLatestRequest } from "../../composables/useLatestRequest";
import { defaultSettings, type AppSettings } from "../../stores/providers";
import { livenessPromptModeOptions } from "../../utils/liveness-options";

const props = defineProps<{
  settings: AppSettings;
}>();

const preview = useLatestRequest({ timeoutMessage: "生成提示词预览超时，请重试", timeoutMs: 15_000 });
const promptPreviews = ref<string[]>([]);
const promptAdvancedVisible = ref(false);
const confirmingReset = ref(false);
let disposed = false;
onScopeDispose(() => { disposed = true; });

const profileSignature = computed(() => JSON.stringify([
  props.settings.livenessPromptMode,
  props.settings.livenessFixedPrompt,
  props.settings.livenessPromptLibrary,
  props.settings.livenessPlaceholderPools,
  props.settings.livenessNumberMin,
  props.settings.livenessNumberMax,
]));
watch(profileSignature, () => {
  preview.invalidate();
  promptPreviews.value = [];
}, { flush: "sync" });

const promptProfileStats = computed(() => {
  const templateCount = props.settings.livenessPromptLibrary.filter((item) =>
    item.trim(),
  ).length;
  const poolCount = props.settings.livenessPlaceholderPools.filter(
    (pool) => pool.key.trim() && pool.values.some((value) => value.trim()),
  ).length;
  const valueCount = props.settings.livenessPlaceholderPools.reduce(
    (total, pool) => total + pool.values.filter((value) => value.trim()).length,
    0,
  );
  return `${templateCount} 个模板 · ${poolCount} 个变量 · ${valueCount} 个候选值`;
});

async function refreshPromptPreviews() {
  if (preview.loading.value) return;
  const snapshot = JSON.parse(JSON.stringify(props.settings)) as AppSettings;
  await preview.run(() => previewLivenessPrompts(snapshot, 10), (result) => {
    promptPreviews.value = result;
  });
}

async function resetPromptProfile() {
  if (disposed || confirmingReset.value) return;
  const signature = profileSignature.value;
  confirmingReset.value = true;
  let confirmed = false;
  try {
    confirmed = await confirmAction(
      "恢复推荐提示词",
      "将替换当前的提示词策略、模板和变量素材，其他测活设置保持不变。",
      "恢复推荐",
      "warning",
    );
  } finally {
    confirmingReset.value = false;
  }
  if (!confirmed || disposed || signature !== profileSignature.value) return;
  const defaults = defaultSettings();
  props.settings.livenessPromptMode = defaults.livenessPromptMode;
  props.settings.livenessFixedPrompt = defaults.livenessFixedPrompt;
  props.settings.livenessPromptLibrary = [...defaults.livenessPromptLibrary];
  props.settings.livenessPlaceholderPools =
    defaults.livenessPlaceholderPools.map((pool) => ({
      key: pool.key,
      values: [...pool.values],
    }));
  props.settings.livenessNumberMin = defaults.livenessNumberMin;
  props.settings.livenessNumberMax = defaults.livenessNumberMax;
  promptPreviews.value = [];
  Message.success("已恢复推荐提示词");
}
</script>

<template>
  <a-form-item label="提示词策略">
    <a-select
      v-model="settings.livenessPromptMode"
      :options="livenessPromptModeOptions"
    />
  </a-form-item>
  <a-form-item v-if="settings.livenessPromptMode === 'fixed'" label="固定提示词">
    <a-textarea
      v-model="settings.livenessFixedPrompt"
      :auto-size="{ minRows: 2, maxRows: 5 }"
      placeholder="填写用于测活的简短问题"
    />
    <template #extra>支持变量，例如 <code>{a}</code>、<code>{cmd}</code>。留空时使用提示词模板。</template>
  </a-form-item>
  <a-form-item v-else label="提示词模板">
    <a-textarea
      :model-value="settings.livenessPromptLibrary.join('\n')"
      :auto-size="{ minRows: 4, maxRows: 8 }"
      placeholder="每行一条提示词"
      @update:model-value="settings.livenessPromptLibrary = String($event).split('\n')"
    />
    <template #extra>每行一条，按所选策略抽取；留空时使用推荐模板。</template>
  </a-form-item>
  <div class="liveness-prompt-toolbar">
    <div>
      <strong>测活提示词</strong>
      <span>{{ promptProfileStats }}</span>
    </div>
    <div class="liveness-prompt-actions">
      <a-button size="small" :loading="preview.loading.value" @click="refreshPromptPreviews">
        预览 10 条
      </a-button>
      <a-button size="small" :disabled="confirmingReset" @click="resetPromptProfile">恢复推荐</a-button>
      <a-button size="small" :aria-expanded="promptAdvancedVisible" @click="promptAdvancedVisible = !promptAdvancedVisible">
        {{ promptAdvancedVisible ? "收起变量" : "变量设置" }}
      </a-button>
    </div>
  </div>
  <a-alert v-if="preview.error.value" type="error" show-icon>{{ preview.error.value }}</a-alert>
  <div v-if="promptPreviews.length > 0" class="liveness-prompt-preview" aria-live="polite">
    <span>内容示例，不会发送测活请求</span>
    <ol>
      <li v-for="(prompt, index) in promptPreviews" :key="`${index}-${prompt}`">
        {{ prompt }}
      </li>
    </ol>
  </div>
  <div v-if="promptAdvancedVisible" class="liveness-prompt-variables">
      <a-form-item label="数字占位范围">
        <div class="duration-control">
          <a-input-number v-model="settings.livenessNumberMin" :min="0" :step="1" aria-label="数字范围起点" />
          <span>至</span>
          <a-input-number v-model="settings.livenessNumberMax" :min="0" :step="1" aria-label="数字范围终点" />
        </div>
        <template #extra>
          用于 <code>{a}</code>、<code>{b}</code>、<code>{number}</code>，按较小值到较大值生成。
        </template>
      </a-form-item>
      <div class="liveness-prompt-toolbar">
        <div><strong>变量素材</strong><span>在模板中使用 {变量名}，每次随机抽取一个候选值</span></div>
        <a-button size="small" @click="settings.livenessPlaceholderPools.push({ key: '', values: [] })">添加变量</a-button>
      </div>
      <div v-for="(pool, index) in settings.livenessPlaceholderPools" :key="index" class="liveness-prompt-pool">
        <a-input v-model="pool.key" placeholder="变量名，如 cmd" :aria-label="`第 ${index + 1} 个变量名称`" />
        <a-textarea
          :model-value="pool.values.join('\n')"
          :auto-size="{ minRows: 2, maxRows: 5 }"
          placeholder="每行一个候选值"
          :aria-label="`${pool.key || `第 ${index + 1} 个变量`}的候选值`"
          @update:model-value="pool.values = String($event).split('\n')"
        />
        <a-button type="text" size="small" status="danger" :aria-label="`移除变量 ${pool.key || index + 1}`" @click="settings.livenessPlaceholderPools.splice(index, 1)">移除</a-button>
      </div>
      <p class="settings-card-note">名称无需填写花括号；空行不参与抽取。内置变量 <code>{time}</code>、<code>{nonce}</code> 可直接使用。</p>
  </div>
</template>
