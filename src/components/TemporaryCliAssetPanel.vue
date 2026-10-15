<script setup lang="ts">
import { computed, watch } from "vue";
import {
  IconInfoCircle,
  IconRefresh,
  IconSearch,
} from "@arco-design/web-vue/es/icon";
import AgentSettingReference from "./AgentSettingReference.vue";
import type {
  TemporaryCliAssetOption,
  TemporaryCliRunSettings,
  TemporaryCliSettingsCapabilities,
} from "../stores/temporary-cli-settings-types";

const props = defineProps<{
  caps: TemporaryCliSettingsCapabilities;
  assets: TemporaryCliAssetOption[];
  loaded: boolean;
  loading: boolean;
  error: string;
  notice: string;
  disabled?: boolean;
}>();
const model = defineModel<TemporaryCliRunSettings>({ required: true });
const emit = defineEmits<{ refresh: [] }>();
const query = defineModel<string>("query", { required: true });
const category = defineModel<string>("category", { required: true });
const labels: Record<string, string> = {
  skill: "Skill",
  mcp: "MCP",
  plugin: "插件",
  extension: "扩展",
  hook: "Hook",
};
const categories = computed(() => [
  ...new Set(props.assets.map((asset) => asset.category)),
]);
const queryText = computed(() => query.value.trim().toLocaleLowerCase());
const disabledIds = computed(() => new Set(model.value.disabledAssets));
const currentIds = computed(
  () => new Set(props.assets.map((asset) => asset.id)),
);
const filtered = computed(() =>
  props.assets.filter(
    (asset) =>
      (!category.value || asset.category === category.value) &&
      asset.label.toLocaleLowerCase().includes(queryText.value),
  ),
);
const missing = computed(() =>
  props.loaded
    ? model.value.disabledAssets.filter((id) => !currentIds.value.has(id))
    : [],
);
watch(
  () => [props.loaded, props.loading, categories.value] as const,
  ([loaded, loading, available]) => {
    if (
      loaded &&
      !loading &&
      category.value &&
      !available.some((item) => item === category.value)
    ) {
      category.value = "";
    }
  },
  { immediate: true },
);

function resetFilters() {
  query.value = "";
  category.value = "";
}

function removeMissing() {
  patch(
    "disabledAssets",
    model.value.disabledAssets.filter((id) => currentIds.value.has(id)),
  );
}

function patch<K extends keyof TemporaryCliRunSettings>(
  key: K,
  value: TemporaryCliRunSettings[K],
) {
  model.value = { ...model.value, [key]: value };
}
function toggle(id: string, retain: boolean) {
  patch(
    "disabledAssets",
    retain
      ? model.value.disabledAssets.filter((value) => value !== id)
      : [...new Set([...model.value.disabledAssets, id])],
  );
}
function groupDisabled(category: string) {
  return category === "skill"
    ? model.value.disableSkills
    : category === "mcp"
      ? model.value.disableMcp
      : category === "hook"
        ? model.value.disableHooks
        : model.value.disablePlugins;
}
</script>

<template>
  <div class="cli-assets-panel">
    <div v-if="caps.assetControls.length" class="cli-assets-groups">
      <section
        v-for="control in caps.assetControls"
        :key="control.key"
        class="cli-assets-group"
      >
        <header>
          <strong>{{ control.label }}</strong>
          <AgentSettingReference :reference="control" />
        </header>
        <a-radio-group
          :model-value="model[control.key] ? 'disabled' : 'inherit'"
          type="button"
          size="small"
          :disabled="disabled"
          @change="patch(control.key, $event === 'disabled')"
        >
          <a-radio value="inherit">保持当前配置</a-radio
          ><a-radio value="disabled">本次停用</a-radio>
        </a-radio-group>
      </section>
    </div>
    <p v-else class="cli-run-note">
      此 Agent
      暂不支持在单次启动时调整资产；可在「权限与工具」中设置本次工具范围。
    </p>

    <header class="cli-assets-list-heading">
      <div>
        <strong>逐项选择</strong>
        <a-tooltip
          content="勾选时按 Agent 已有的启停设置加载资产；取消勾选仅在本次启动中停用。"
        >
          <button
            type="button"
            class="cli-native-reference"
            aria-label="查看资产选择说明"
          >
            <icon-info-circle />
          </button>
        </a-tooltip>
      </div>
      <a-button
        type="text"
        size="small"
        :loading="loading"
        @click="emit('refresh')"
        ><template #icon><icon-refresh /></template>刷新</a-button
      >
    </header>
    <div class="cli-assets-search">
      <a-input
        v-model="query"
        allow-clear
        placeholder="搜索当前 Agent 的资产"
        size="small"
        ><template #prefix><icon-search /></template
      ></a-input>
    </div>
    <nav
      v-if="categories.length > 1"
      class="cli-asset-filters"
      aria-label="资产类型"
    >
      <button
        type="button"
        :class="{ active: !category }"
        :aria-pressed="!category"
        @click="category = ''"
      >
        全部</button
      ><button
        v-for="item in categories"
        :key="item"
        type="button"
        :class="{ active: category === item }"
        :aria-pressed="category === item"
        @click="category = item"
      >
        {{ labels[item] || item }}
      </button>
    </nav>
    <p v-if="error" class="cli-run-error" role="alert">{{ error }}</p>
    <p v-if="notice" class="cli-run-note">{{ notice }}</p>
    <p v-if="missing.length" class="cli-run-error">
      {{ missing.length }} 项选择已不在当前目录环境中。<a-button
        type="text"
        size="mini"
        :disabled="disabled"
        @click="removeMissing"
        >移除失效选择</a-button
      >
    </p>
    <div
      v-if="loading && !loaded"
      class="cli-run-skeleton"
      role="status"
      aria-label="正在读取所选 Agent 的资产"
    >
      <i /><i /><i />
    </div>
    <div v-else class="cli-assets-list">
      <div v-for="asset in filtered" :key="asset.id" class="cli-assets-item">
        <a-checkbox
          :model-value="
            !groupDisabled(asset.category) && !disabledIds.has(asset.id)
          "
          :disabled="
            disabled || !asset.configurable || groupDisabled(asset.category)
          "
          :aria-label="`按已有启停设置加载 ${asset.label}`"
          @change="toggle(asset.id, Boolean($event))"
        >
          <span class="cli-assets-item-copy"
            ><strong>{{ asset.label }}</strong
            ><small v-if="asset.note">{{ asset.note }}</small></span
          >
        </a-checkbox>
        <span class="cli-assets-item-state">{{
          groupDisabled(asset.category) || disabledIds.has(asset.id)
            ? "本次停用"
            : asset.currentLabel
        }}</span>
        <span class="cli-assets-item-category">{{
          labels[asset.category] || asset.category
        }}</span>
      </div>
      <p v-if="!filtered.length && !error" class="cli-run-empty">
        {{
          queryText || category
            ? "没有匹配的资产"
            : "当前没有可列出的已启用资产"
        }}
        <a-button
          v-if="queryText || category"
          type="text"
          size="small"
          @click="resetFilters"
          >清除筛选</a-button
        >
      </p>
    </div>
  </div>
</template>
