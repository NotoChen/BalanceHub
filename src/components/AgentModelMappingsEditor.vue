<script setup lang="ts">
import { computed, ref } from "vue";
import {
  IconArrowRight,
  IconCode,
  IconInfoCircle,
  IconPlus,
  IconUndo,
} from "@arco-design/web-vue/es/icon";
import AgentModelSelect from "./AgentModelSelect.vue";
import type {
  AgentModelMappingEntry,
  AgentModelMappingValue,
  AgentModelCatalogContext,
  AgentNativeSetting,
  AgentSettingEffectiveValue,
  AgentSettingValueSummary,
} from "../stores/agent-settings-types";
import {
  settingValueBadge,
  settingValueDescription,
  settingValueLabel,
} from "../utils/agent-setting-display";
import "../styles/modules/agent-model-mappings.css";

const props = defineProps<{
  field: AgentNativeSetting;
  value?: string;
  inherited?: AgentSettingEffectiveValue;
  effective?: AgentSettingEffectiveValue;
  modelCatalog?: AgentModelCatalogContext;
  providerModels?: readonly string[];
  disabled?: boolean;
  persistent?: boolean;
  highlightedRowId?: string;
  sourcePending?: boolean;
}>();
const emit = defineEmits<{
  "update:value": [value: string | undefined];
  "reveal-source": [rowId: string];
}>();
type Mappings = Record<string, AgentModelMappingValue>;
function parse(value?: string | null): Mappings {
  const result: Mappings = Object.create(null);
  if (!value) return result;
  try {
    return Object.assign(result, JSON.parse(value));
  } catch {
    return result;
  }
}
const options = computed(() => props.field.modelMappings);
const own = computed(() => parse(props.value));
const fallback = computed(() =>
  parse(props.inherited?.value ?? props.field.currentValue),
);
const adding = ref(false);
const newAlias = ref("");
const newModel = ref<string>();
const addError = ref("");
const rows = computed(() => {
  const entries = [...(options.value?.entries ?? [])];
  for (const id of Object.keys(own.value)) {
    if (entries.some((entry) => entry.id === id)) continue;
    entries.push({
      id,
      label: id,
      nativeKey: props.field.nativeKey,
      current: {
        value: null,
        label: "未设置模型映射",
        source: "",
        state: "unset" as const,
        detail: "此别名尚未保存。",
      },
      targetDefault: null,
      resetValue: null,
      lockedReason: "",
      supportsDisplayName: options.value?.displayNames ?? false,
    });
  }
  return entries;
});
const parentOptions = computed(() =>
  rows.value.map((row) => ({ value: row.id, label: row.label })),
);
function publish(value: Mappings) {
  emit(
    "update:value",
    Object.keys(value).length ? JSON.stringify(value) : undefined,
  );
}
function change(id: string, key: keyof AgentModelMappingValue, value?: string) {
  const next = { ...own.value };
  const row = { ...own.value[id] };
  if (value?.trim()) row[key] = value;
  else delete row[key];
  if (Object.keys(row).length) publish({ ...next, [id]: row });
  else {
    delete next[id];
    publish(next);
  }
}
function reset(id: string) {
  const next = { ...own.value };
  delete next[id];
  publish(next);
}
function defaultValue(row: AgentModelMappingEntry): AgentSettingValueSummary {
  return row.targetDefault ?? row.current;
}
function resetValue(row: AgentModelMappingEntry): AgentSettingValueSummary {
  if (row.resetValue) return row.resetValue;
  if (!props.inherited) return row.current;
  const inherited = props.inherited.modelMappings?.entries.find(
    (entry) => entry.id === row.id,
  );
  if (inherited) return inherited.current;
  return {
    value: null,
    label: "未设置模型映射",
    source: "",
    state: "unset",
    detail: "移除此处的模型映射后，其他已读取的配置中也没有此别名的模型映射。",
  };
}
function effectiveModel(row: AgentModelMappingEntry) {
  const effective = props.effective?.modelMappings?.entries.find(
    (entry) => entry.id === row.id,
  )?.current;
  if (effective?.state === "readError") return "生效模型未确认";
  const selected = own.value[row.id]?.model ?? row.current.value;
  return effective?.value && selected && effective.value !== selected
    ? `${props.persistent ? "保存后生效" : "本次生效"}：${settingValueLabel(effective)}`
    : "";
}
function add() {
  const id = newAlias.value.trim();
  if (!id || !newModel.value) {
    addError.value = "填写别名并选择实际模型";
    return;
  }
  if (rows.value.some((row) => row.id === id)) {
    addError.value = "此别名已存在，可直接修改对应行";
    return;
  }
  publish({ ...own.value, [id]: { model: newModel.value } });
  newAlias.value = "";
  newModel.value = undefined;
  addError.value = "";
  adding.value = false;
}
</script>

<template>
  <div
    class="agent-model-mappings"
    :class="{ 'has-extra': options?.displayNames || options?.inheritance }"
  >
    <div v-if="rows.length" class="agent-model-mapping-head" aria-hidden="true">
      <span>{{ options?.allowCustomAliases ? "别名" : "档位" }}</span
      ><span />
      <span>实际模型</span>
      <span v-if="options?.displayNames || options?.inheritance">{{
        options.inheritance ? "继承配置（可选）" : "显示名称（可选）"
      }}</span
      ><span />
    </div>
    <TransitionGroup
      name="model-mapping"
      tag="div"
      class="agent-model-mapping-list"
    >
      <div
        v-for="row in rows"
        :key="row.id"
        class="agent-model-mapping-row"
        :data-mapping-id="row.id"
        :class="{
          'is-customized': own[row.id],
          'is-source-target': row.id === highlightedRowId,
        }"
        :aria-label="row.label"
        tabindex="-1"
      >
        <div class="agent-model-mapping-name">
          <span :title="row.id">{{ row.label }}</span>
          <a-popover
            trigger="click"
            position="bottom"
            :title="row.label"
            content-class="cli-native-explanation"
          >
            <button
              type="button"
              class="cli-native-reference"
              :class="{
                'has-read-error': row.current.state === 'readError',
              }"
              :aria-label="`查看 ${row.label} 的映射来源`"
            >
              <icon-info-circle />
            </button>
            <template #content
              ><dl>
                <dt>配置键</dt>
                <dd>
                  <code>{{ row.nativeKey }}</code>
                </dd>
                <dt>解析模型</dt>
                <dd>{{ settingValueLabel(row.current) }}</dd>
                <dt>解析状态</dt>
                <dd>{{ settingValueBadge(row.current) }}</dd>
                <template v-if="row.current.source">
                  <dt>来源</dt>
                  <dd>{{ row.current.source }}</dd>
                </template>
                <template v-if="row.current.detail">
                  <dt>说明</dt>
                  <dd>{{ row.current.detail }}</dd>
                </template>
                <template v-if="own[row.id]">
                  <dt>
                    {{ persistent ? "移除此处映射后" : "取消本次覆盖后" }}
                  </dt>
                  <dd>{{ settingValueLabel(resetValue(row)) }}</dd>
                  <template v-if="settingValueDescription(resetValue(row))">
                    <dt>依据</dt>
                    <dd>{{ settingValueDescription(resetValue(row)) }}</dd>
                  </template>
                </template>
              </dl>
              <p v-if="row.lockedReason">{{ row.lockedReason }}</p></template
            >
          </a-popover>
          <button
            v-if="
              persistent &&
              field.sourceReferences.some(
                (reference) => reference.rowId === row.id,
              )
            "
            type="button"
            class="cli-native-icon-button"
            :disabled="sourcePending"
            :title="
              sourcePending
                ? '等待草稿生成后定位'
                : `定位 ${row.label} 的配置原文`
            "
            :aria-label="`定位 ${row.label} 的配置原文`"
            @click="emit('reveal-source', row.id)"
          >
            <icon-code />
          </button>
        </div>
        <icon-arrow-right class="agent-model-mapping-arrow" />
        <div class="agent-model-mapping-target">
          <AgentModelSelect
            :value="own[row.id]?.model"
            :native-choices="field.choices"
            :catalog="modelCatalog"
            :provider-models="providerModels"
            :default-label="settingValueLabel(defaultValue(row))"
            :default-description="settingValueDescription(defaultValue(row))"
            :default-badge="settingValueBadge(defaultValue(row))"
            :disabled="disabled || Boolean(row.lockedReason)"
            :aria-label="`${row.label} 的实际模型`"
            @update:value="change(row.id, 'model', $event)"
          />
          <span
            v-if="effectiveModel(row)"
            class="agent-model-mapping-effective"
            >{{ effectiveModel(row) }}</span
          >
        </div>
        <a-input
          v-if="options?.displayNames && row.supportsDisplayName"
          class="agent-model-mapping-extra"
          :model-value="own[row.id]?.displayName ?? ''"
          :placeholder="fallback[row.id]?.displayName || '未设置显示名称'"
          :disabled="disabled || Boolean(row.lockedReason)"
          :aria-label="`${row.label} 的显示名称`"
          @update:model-value="change(row.id, 'displayName', $event)"
        />
        <span
          v-else-if="options?.displayNames"
          class="agent-model-mapping-extra agent-model-mapping-empty-cell"
          title="子 Agent 模型没有独立的菜单显示名称"
          >—</span
        >
        <a-select
          v-else-if="options?.inheritance"
          class="agent-model-mapping-extra"
          :model-value="own[row.id]?.extends"
          :placeholder="fallback[row.id]?.extends || '不继承'"
          :options="parentOptions.filter((option) => option.value !== row.id)"
          allow-clear
          allow-create
          allow-search
          :disabled="disabled || Boolean(row.lockedReason)"
          :aria-label="`${row.label} 的继承配置`"
          @change="
            change(
              row.id,
              'extends',
              $event == null ? undefined : String($event),
            )
          "
        />
        <button
          v-if="own[row.id]"
          type="button"
          class="cli-native-icon-button cli-native-reset agent-model-mapping-reset"
          :disabled="disabled"
          :title="
            persistent
              ? '移除此处映射，其他来源与模型参数继续生效'
              : '取消这项映射的本次覆盖'
          "
          :aria-label="`${row.label}：${persistent ? '移除映射' : '取消覆盖'}`"
          @click="reset(row.id)"
        >
          <icon-undo />
        </button>
        <span v-else class="agent-model-mapping-reset" />
      </div>
    </TransitionGroup>
    <p v-if="!rows.length" class="agent-model-mapping-empty">
      尚未设置模型别名
    </p>
    <template v-if="options?.allowCustomAliases && !disabled">
      <div v-if="adding" class="agent-model-mapping-add">
        <a-input
          v-model="newAlias"
          placeholder="别名，例如 coding"
          aria-label="新模型别名"
          @press-enter="add"
        />
        <AgentModelSelect
          v-model:value="newModel"
          :native-choices="field.choices"
          :catalog="modelCatalog"
          :provider-models="providerModels"
          default-label="选择实际模型"
          aria-label="新别名的实际模型"
        />
        <a-button type="primary" size="small" @click="add">添加</a-button
        ><a-button
          type="text"
          size="small"
          @click="
            adding = false;
            addError = '';
          "
          >取消</a-button
        >
        <p v-if="addError" class="cli-native-error" role="alert">
          {{ addError }}
        </p>
      </div>
      <a-button
        v-else
        type="text"
        size="small"
        class="agent-model-mapping-add-button"
        @click="adding = true"
        ><template #icon><icon-plus /></template>添加别名</a-button
      >
    </template>
    <p v-if="field.lockedReason" class="cli-native-hint">
      {{ field.lockedReason }}
    </p>
  </div>
</template>
