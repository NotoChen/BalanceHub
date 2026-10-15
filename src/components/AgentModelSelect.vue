<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { SelectOptionData } from "@arco-design/web-vue";
import { IconRefresh } from "@arco-design/web-vue/es/icon";
import type {
  AgentSettingChoice,
  AgentModelCatalogContext,
} from "../stores/agent-settings-types";
import { useAgentModelCatalog } from "../composables/useAgentModelCatalog";
import "../styles/modules/agent-model-select.css";

const props = defineProps<{
  value?: string;
  nativeChoices: readonly AgentSettingChoice[];
  catalog?: AgentModelCatalogContext;
  providerModels?: readonly string[];
  defaultLabel: string;
  defaultDescription?: string;
  defaultBadge?: string;
  disabled?: boolean;
  loading?: boolean;
}>();
const emit = defineEmits<{ "update:value": [value: string | undefined] }>();
const source = ref<"native" | "provider">("native");
const models = useAgentModelCatalog(() => props.catalog);
const nativeChoices = computed(() => {
  const seen = new Set<string>();
  return [
    ...props.nativeChoices,
    ...(models.catalog.value?.choices ?? []),
  ].filter((item) => {
    if (seen.has(item.value)) return false;
    seen.add(item.value);
    return true;
  });
});
const refreshTitle = computed(() => {
  const current = models.catalog.value;
  return current?.refreshedAt
    ? `刷新 CLI 模型 · ${current.source} · ${new Date(current.refreshedAt).toLocaleString("zh-CN")}`
    : "刷新 CLI 模型目录";
});
const hasProvider = computed(() => props.providerModels !== undefined);
watch(
  hasProvider,
  (enabled) => {
    source.value = enabled ? "provider" : "native";
  },
  { immediate: true },
);
const providerChoices = computed(() =>
  [
    ...new Set(
      (props.providerModels ?? []).map((id) => id.trim()).filter(Boolean),
    ),
  ]
    .sort((left, right) => left.localeCompare(right))
    .map((id) => ({
      value: id,
      label: id,
      description: "中转站返回的模型 ID",
    })),
);
const activeChoices = computed(() =>
  source.value === "provider" ? providerChoices.value : nativeChoices.value,
);
const options = computed<SelectOptionData[]>(() => {
  const result: SelectOptionData[] = [
    {
      value: "__inherit__",
      label: props.defaultLabel,
      description: props.defaultDescription,
      badge: props.defaultBadge,
    },
    ...activeChoices.value,
  ];
  if (
    props.value &&
    !activeChoices.value.some((choice) => choice.value === props.value)
  )
    result.push({
      value: props.value,
      label: props.value,
      badge: "当前选择",
      description: "当前模型 ID 不在这份列表中",
    });
  return result;
});
function filterOption(query: string, option: SelectOptionData) {
  const search = query.trim().toLocaleLowerCase();
  return `${option.label ?? ""} ${option.value ?? ""}`
    .toLocaleLowerCase()
    .includes(search);
}
function select(value: unknown) {
  emit(
    "update:value",
    typeof value === "string" && value !== "__inherit__" && value.trim()
      ? value.trim()
      : undefined,
  );
}
</script>

<template>
  <a-select
    class="agent-model-select"
    :model-value="value || '__inherit__'"
    :options="options"
    :filter-option="filterOption"
    :disabled="disabled"
    :loading="loading || (source === 'native' && models.loading.value)"
    :virtual-list-props="{ height: 260 }"
    allow-search
    allow-create
    allow-clear
    show-header-on-empty
    show-footer-on-empty
    @update:model-value="select"
    @popup-visible-change="models.popupVisible"
  >
    <template #header>
      <div
        class="agent-model-sources"
        role="group"
        aria-label="模型列表来源"
        @mousedown.prevent
      >
        <template v-if="hasProvider">
          <button
            type="button"
            :aria-pressed="source === 'provider'"
            @click.stop="source = 'provider'"
          >
            中转站模型 <span>{{ providerChoices.length }}</span>
          </button>
          <button
            type="button"
            :aria-pressed="source === 'native'"
            @click.stop="source = 'native'"
          >
            Agent CLI 模型 <span>{{ nativeChoices.length }}</span>
          </button>
        </template>
        <span v-else class="agent-model-source-label"
          >Agent CLI 模型 <small>{{ nativeChoices.length }}</small></span
        >
        <button
          v-if="source === 'native' && catalog"
          type="button"
          class="agent-model-refresh"
          :class="{ 'is-loading': models.loading.value }"
          :disabled="models.loading.value"
          :title="refreshTitle"
          aria-label="刷新 CLI 模型目录"
          @click.stop="models.load(true)"
        >
          <icon-refresh />
        </button>
      </div>
    </template>
    <template #option="{ data }">
      <span class="agent-model-option" :title="data.description">
        <span>{{ data.label }}</span>
        <small v-if="data.badge">{{ data.badge }}</small>
        <code
          v-else-if="data.value !== '__inherit__' && data.label !== data.value"
          >{{ data.value }}</code
        >
      </span>
    </template>
    <template #empty
      ><span class="agent-model-empty"
        >没有匹配项，可直接输入模型 ID</span
      ></template
    >
    <template #footer>
      <p class="agent-model-source-note">
        <template v-if="source === 'provider'">
          {{
            providerChoices.length
              ? "来自中转站模型查询；不同 API Key 的分组与权限可能不同。"
              : "暂未读取到中转站模型，可切换 Agent CLI 列表或输入模型 ID。"
          }}
        </template>
        <template v-else>
          {{
            models.notice.value ||
            (models.loading.value
              ? "正在刷新模型目录，已有选择仍可使用。"
              : nativeChoices.length
                ? "模型目录自动跟随所选 CLI；是否可调用由账号或中转站决定。"
                : "未读取到 CLI 模型目录，可直接输入模型 ID。")
          }}
        </template>
      </p>
    </template>
  </a-select>
</template>
