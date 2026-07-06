<script setup lang="ts">
import { computed, ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { previewLivenessPrompts } from "../../api/app";
import { defaultSettings, type AppSettings } from "../../stores/providers";
import { codexPromptModeOptions } from "../../utils/liveness-options";

const props = defineProps<{
  settings: AppSettings;
}>();

const previewingPrompts = ref(false);
const promptPreviews = ref<string[]>([]);
const promptAdvancedVisible = ref(false);

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
  return `${templateCount} 个模板 / ${poolCount} 个数据池 / ${valueCount} 个素材`;
});

const placeholderPoolsText = computed({
  get: () =>
    props.settings.livenessPlaceholderPools
      .map((pool) => {
        const key = pool.key.trim();
        const values = pool.values.map((value) => value.trim()).filter(Boolean);
        return key && values.length > 0 ? `${key}=${values.join(" | ")}` : "";
      })
      .filter(Boolean)
      .join("\n"),
  set: (value: string) => {
    props.settings.livenessPlaceholderPools = value
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean)
      .map((line) => {
        const [key, ...rest] = line.split("=");
        return {
          key: key.trim().replace(/[{}]/g, ""),
          values: rest
            .join("=")
            .split("|")
            .map((item) => item.trim())
            .filter(Boolean),
        };
      })
      .filter((pool) => pool.key && pool.values.length > 0);
  },
});

async function refreshPromptPreviews() {
  previewingPrompts.value = true;
  try {
    promptPreviews.value = await previewLivenessPrompts(props.settings, 10);
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  } finally {
    previewingPrompts.value = false;
  }
}

function resetPromptProfile() {
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
  Message.success("已恢复推荐话术配置");
}
</script>

<template>
  <a-form-item label="话术策略">
    <a-select
      v-model="settings.livenessPromptMode"
      :options="codexPromptModeOptions"
    />
  </a-form-item>
  <div class="codex-prompt-toolbar">
    <div>
      <strong>话术配置</strong>
      <span>{{ promptProfileStats }}</span>
    </div>
    <div class="codex-prompt-actions">
      <a-button size="small" :loading="previewingPrompts" @click="refreshPromptPreviews">
        生成 10 条
      </a-button>
      <a-button size="small" @click="resetPromptProfile">推荐默认</a-button>
      <a-button size="small" @click="promptAdvancedVisible = !promptAdvancedVisible">
        {{ promptAdvancedVisible ? "收起高级" : "高级设置" }}
      </a-button>
    </div>
  </div>
  <div v-if="promptPreviews.length > 0" class="codex-prompt-preview">
    <ol>
      <li v-for="(prompt, index) in promptPreviews" :key="`${index}-${prompt}`">
        {{ prompt }}
      </li>
    </ol>
  </div>
  <div
    v-if="promptAdvancedVisible || settings.livenessPromptMode === 'fixed'"
    class="codex-prompt-advanced"
  >
    <a-form-item label="固定话术">
      <a-textarea
        v-model="settings.livenessFixedPrompt"
        :auto-size="{ minRows: 2, maxRows: 3 }"
        placeholder="Explain: ls -la"
      />
      <template #extra>
        支持 <code>{time}</code>、<code>{nonce}</code>、<code>{a}</code>、<code>{b}</code>、
        <code>{cmd}</code>、<code>{snake}</code>、<code>{var}</code>、<code>{path}</code>、
        <code>{json}</code>、<code>{phrase}</code> 等动态占位。
      </template>
    </a-form-item>
    <template v-if="promptAdvancedVisible">
      <a-form-item label="话术库">
        <a-textarea
          :model-value="settings.livenessPromptLibrary.join('\n')"
          :auto-size="{ minRows: 4, maxRows: 8 }"
          placeholder="每行一个测活话术"
          @update:model-value="settings.livenessPromptLibrary = String($event).split('\n')"
        />
      </a-form-item>
      <a-form-item label="数字占位范围">
        <div class="duration-control">
          <a-input-number v-model="settings.livenessNumberMin" :min="0" :step="1" />
          <a-input-number v-model="settings.livenessNumberMax" :min="0" :step="1" />
        </div>
        <template #extra>
          用于 <code>{a}</code>、<code>{b}</code>、<code>{number}</code>，保存时会按较小值到较大值生成。
        </template>
      </a-form-item>
      <a-form-item label="占位符数据池">
        <a-textarea
          v-model="placeholderPoolsText"
          :auto-size="{ minRows: 7, maxRows: 12 }"
          placeholder="cmd=ls -la | git status&#10;snake=file_name | total_count"
        />
        <template #extra>
          每行一个池：<code>key=value1 | value2</code>。模板中的
          <code>{key}</code> 会从对应池随机取值。
        </template>
      </a-form-item>
    </template>
  </div>
</template>
