<script setup lang="ts">
import { computed, ref, useId } from "vue";
import {
  IconCode,
  IconInfoCircle,
  IconUndo,
} from "@arco-design/web-vue/es/icon";
import type {
  AgentNativeSetting,
  AgentSettingEffectiveValue,
  AgentModelCatalogContext,
} from "../stores/agent-settings-types";
import AgentSettingSlider from "./AgentSettingSlider.vue";
import AgentModelSelect from "./AgentModelSelect.vue";
import AgentModelMappingsEditor from "./AgentModelMappingsEditor.vue";
import {
  currentSettingValue,
  knownSettingValue,
  settingValueBadge,
  settingValueDescription,
  settingValueLabel,
} from "../utils/agent-setting-display";
import "../styles/modules/agent-setting-fields.css";

const props = defineProps<{
  field: AgentNativeSetting;
  value?: string;
  disabled?: boolean;
  conflict?: string;
  mode?: "launch" | "configuration";
  effective?: AgentSettingEffectiveValue;
  inherited?: AgentSettingEffectiveValue;
  providerModels?: readonly string[];
  modelCatalog?: AgentModelCatalogContext;
  highlightedRowId?: string;
  sourcePending?: boolean;
}>();
const emit = defineEmits<{
  "update:value": [value: string | undefined];
  "reveal-source": [rowId?: string];
}>();
const customTool = ref("");
const inputId = useId();
const persistent = computed(() => props.mode === "configuration");
const resetLabel = computed(() => (persistent.value ? "移除设置" : "取消覆盖"));
const inheritedValue = computed(
  () => props.inherited ?? props.field.resetValue ?? undefined,
);
const resetValue = computed(
  () => inheritedValue.value ?? currentSettingValue(props.field),
);
const resetValueLabel = computed(() =>
  settingValueLabel(resetValue.value, props.field),
);
const resetHint = computed(() =>
  [
    persistent.value
      ? "移除此项设置，确认保存后写入文件"
      : "取消这一项的本次覆盖",
    `操作后：${resetValueLabel.value}`,
    settingValueDescription(resetValue.value),
  ].join("\n"),
);
const overridden = computed(() => props.value !== undefined);
const displayedField = computed(() =>
  !overridden.value && inheritedValue.value
    ? {
        ...props.field,
        currentValue: inheritedValue.value.value,
        currentLabel: inheritedValue.value.label,
        currentState: inheritedValue.value.state,
        currentDetail: inheritedValue.value.detail,
        source: inheritedValue.value.source,
      }
    : props.field,
);
const displayedValue = computed(() =>
  currentSettingValue(displayedField.value),
);
const valueReadError = computed(
  () => displayedValue.value.state === "readError",
);
const locked = computed(
  () => props.disabled || Boolean(props.field.lockedReason),
);
const selected = computed(() =>
  (props.value ?? "").split("\n").filter(Boolean),
);
const extraTools = computed(() =>
  selected.value.filter(
    (value) => !props.field.choices.some((choice) => choice.value === value),
  ),
);
const numberModel = computed({
  get: () => {
    const value = props.value ?? knownSettingValue(displayedValue.value);
    return value === null || value === "" || !Number.isFinite(Number(value))
      ? undefined
      : Number(value);
  },
  set: (value: number | undefined) => {
    if (value === numberModel.value) return;
    emit("update:value", value === undefined ? undefined : String(value));
  },
});
const slider = computed(
  () =>
    props.field.section === "model" &&
    (props.field.control === "number" ||
      (props.field.control === "select" &&
        ["effort", "verbosity"].includes(props.field.id) &&
        props.field.choices.length > 1)),
);
const effectiveDifference = computed(() =>
  props.effective &&
  props.effective.value !== (props.value ?? displayedField.value.currentValue)
    ? settingValueLabel(props.effective, props.field)
    : "",
);
const inheritedItems = computed(() =>
  !overridden.value && ["multiSelect", "rules"].includes(props.field.control)
    ? (displayedField.value.currentValue ?? "").split("\n").filter(Boolean)
    : [],
);
const selectedDescription = computed(
  () =>
    props.field.choices.find((choice) => choice.value === props.value)
      ?.description,
);
const invalidSelection = computed(() => {
  if (!overridden.value) return "";
  if (props.field.lockedReason || props.field.control === "readonly")
    return persistent.value ? "" : "当前条件下不能覆盖此项，请取消覆盖。";
  if (
    props.field.control === "select" &&
    !props.field.allowCustom &&
    !props.field.choices.some((choice) => choice.value === props.value)
  )
    return `当前 Agent 或模型未提供这个值，请重新选择或${resetLabel.value}。`;
  return props.conflict || "";
});

function setChoice(value: unknown) {
  if (
    value !== undefined &&
    !["string", "number", "boolean"].includes(typeof value)
  )
    return;
  emit(
    "update:value",
    value === "__inherit__" || value === undefined ? undefined : String(value),
  );
}
function enableToolSelection() {
  emit("update:value", displayedField.value.currentValue ?? "");
}
function toggleTool(value: string, checked: boolean) {
  const values = new Set(selected.value);
  if (checked) values.add(value);
  else values.delete(value);
  emit("update:value", [...values].join("\n"));
}
function addTool() {
  const values = customTool.value.split(/[,，\s]+/).filter(Boolean);
  if (!values.length) return;
  emit("update:value", [...new Set([...selected.value, ...values])].join("\n"));
  customTool.value = "";
}
</script>

<template>
  <section
    class="cli-native-field"
    :data-setting-id="field.id"
    :aria-label="field.label"
    tabindex="-1"
    :class="{
      'is-overridden': overridden,
      'has-slider': slider && !field.lockedReason,
      'has-model-mappings': field.control === 'modelMappings',
    }"
  >
    <header class="cli-native-field-heading">
      <label :for="inputId">{{ field.label }}</label>
      <a-popover
        trigger="click"
        position="bottom"
        :title="field.label"
        content-class="cli-native-explanation"
      >
        <button
          type="button"
          class="cli-native-reference"
          :class="{ 'has-read-error': valueReadError }"
          :title="`查看${field.label}的配置来源`"
          :aria-label="`查看${field.label}的原生配置与说明`"
        >
          <icon-info-circle />
        </button>
        <template #content>
          <dl>
            <dt>配置键</dt>
            <dd>
              <code>{{ field.nativeKey }}</code>
            </dd>
            <dt>
              {{ field.currentState === "readError" ? "参考值" : "当前值" }}
            </dt>
            <dd>
              {{
                field.currentState === "readError" &&
                field.control !== "modelMappings"
                  ? (field.currentValue ?? "无可用参考值")
                  : field.currentLabel
              }}
            </dd>
            <dt>配置状态</dt>
            <dd>{{ settingValueBadge(currentSettingValue(field)) }}</dd>
            <template
              v-if="
                field.currentNativeValue !== null &&
                field.currentNativeValue !== field.currentValue
              "
            >
              <dt>原文值</dt>
              <dd>
                <pre>{{ field.currentNativeValue }}</pre>
              </dd>
            </template>
            <template v-if="field.source">
              <dt>读取来源</dt>
              <dd>{{ field.source }}</dd>
            </template>
            <template
              v-if="
                effective &&
                (effective.label !== field.currentLabel ||
                  effective.source !== field.source)
              "
            >
              <dt>{{ persistent ? "保存后生效值" : "本次生效值" }}</dt>
              <dd>{{ settingValueLabel(effective, field) }}</dd>
              <template v-if="effective.source">
                <dt>生效来源</dt>
                <dd>{{ effective.source }}</dd>
              </template>
              <template v-if="effective.detail">
                <dt>说明</dt>
                <dd>{{ effective.detail }}</dd>
              </template>
            </template>
            <template v-if="overridden && field.control !== 'modelMappings'">
              <dt>{{ persistent ? "此处设置" : "本次选择" }}</dt>
              <dd>
                <pre
                  >{{ value || "空集合"
                  }}{{ field.unit ? " " + field.unit : "" }}</pre>
              </dd>
              <template v-if="inheritedValue">
                <dt>移除后</dt>
                <dd>{{ resetValueLabel }}</dd>
                <template v-if="settingValueDescription(resetValue)">
                  <dt>依据</dt>
                  <dd>{{ settingValueDescription(resetValue) }}</dd>
                </template>
              </template>
            </template>
          </dl>
          <p v-if="field.currentDetail || field.description">
            {{
              field.currentDetail &&
              field.currentDetail !== "已读取的配置中没有设置此项。"
                ? field.currentDetail
                : field.description
            }}
          </p>
          <p v-if="selectedDescription">{{ selectedDescription }}</p>
          <p v-if="field.lockedReason">{{ field.lockedReason }}</p>
          <p v-if="slider && field.control === 'number'">
            滑块用于快速调整，数值框可精确输入。拖动范围不代表模型服务端的容量上限。
          </p>
        </template>
      </a-popover>
      <button
        v-if="persistent && field.sourceReferences?.length"
        type="button"
        class="cli-native-source-link"
        :disabled="sourcePending"
        :title="
          sourcePending
            ? '等待草稿生成后定位'
            : `定位 ${field.label} 的配置原文`
        "
        :aria-label="`定位 ${field.label} 的配置原文`"
        @click="emit('reveal-source')"
      >
        <icon-code />原文
      </button>
      <button
        v-if="overridden && (!slider || field.lockedReason)"
        type="button"
        class="cli-native-icon-button cli-native-reset"
        :disabled="disabled"
        :title="resetHint"
        :aria-label="`${field.label}：${resetLabel}`"
        @click="emit('update:value', undefined)"
      >
        <icon-undo />
      </button>
    </header>
    <div class="cli-native-control">
      <AgentModelMappingsEditor
        v-if="field.control === 'modelMappings'"
        :field="field"
        :value="value"
        :inherited="inheritedValue"
        :effective="effective"
        :model-catalog="modelCatalog"
        :provider-models="providerModels"
        :disabled="locked"
        :persistent="persistent"
        :highlighted-row-id="highlightedRowId"
        :source-pending="sourcePending"
        @update:value="emit('update:value', $event)"
        @reveal-source="emit('reveal-source', $event)"
      />
      <div
        v-else-if="field.control === 'readonly' || field.lockedReason"
        class="cli-native-current-value"
      >
        <strong>{{ settingValueLabel(displayedValue, field) }}</strong>
        <span v-if="field.lockedReason">{{ field.lockedReason }}</span>
      </div>
      <AgentSettingSlider
        v-else-if="slider"
        :field="displayedField"
        :value="value"
        :disabled="disabled"
        :input-id="inputId"
        :reset-label="resetLabel"
        :reset-hint="resetHint"
        @update:value="emit('update:value', $event)"
      />
      <a-select
        v-else-if="field.control === 'select'"
        :id="inputId"
        :model-value="value ?? '__inherit__'"
        :disabled="disabled"
        :allow-create="field.allowCustom"
        :allow-search="field.allowCustom"
        :aria-label="field.label"
        @change="setChoice"
      >
        <a-option
          value="__inherit__"
          :label="resetValueLabel"
          :title="resetHint"
        >
          <span class="cli-native-choice"
            ><span>{{ resetValueLabel }}</span
            ><small>{{ settingValueBadge(resetValue) }}</small></span
          >
        </a-option>
        <a-option
          v-for="choice in field.choices"
          :key="choice.value"
          :value="choice.value"
          :label="choice.label"
          :title="choice.description"
        >
          <span class="cli-native-choice"
            ><span>{{ choice.label }}</span
            ><code v-if="choice.label !== choice.value">{{
              choice.value
            }}</code></span
          >
        </a-option>
      </a-select>
      <AgentModelSelect
        v-else-if="field.control === 'model'"
        :id="inputId"
        :value="value"
        :native-choices="field.choices"
        :catalog="modelCatalog"
        :provider-models="providerModels"
        :default-label="resetValueLabel"
        :default-description="resetHint"
        :default-badge="settingValueBadge(resetValue)"
        :disabled="disabled"
        :aria-label="field.label"
        @update:value="emit('update:value', $event)"
      />
      <div v-else-if="field.control === 'number'" class="cli-native-number">
        <a-input-number
          v-model="numberModel"
          :id="inputId"
          :min="field.minimum ?? undefined"
          :max="field.maximum ?? undefined"
          :precision="0"
          :placeholder="
            displayedValue.state === 'readError' ? '读取失败' : '未设置'
          "
          :disabled="disabled"
          hide-button
          :aria-label="field.label"
        />
        <span>{{ field.unit }}</span>
      </div>
      <template v-else-if="field.control === 'multiSelect'">
        <div v-if="!overridden" class="cli-native-tool-inherit">
          <span>{{ settingValueLabel(displayedValue, field) }}</span>
          <a-button
            type="outline"
            size="small"
            :disabled="locked"
            @click="enableToolSelection"
            >自选{{ field.choices.length ? "" : "工具" }}</a-button
          >
        </div>
        <div v-else class="cli-native-tools">
          <div v-if="field.choices.length" class="cli-native-tool-grid">
            <a-checkbox
              v-for="choice in field.choices"
              :key="choice.value"
              :model-value="selected.includes(choice.value)"
              :disabled="disabled"
              @change="toggleTool(choice.value, Boolean($event))"
            >
              <span
                >{{ choice.label }}<code>{{ choice.value }}</code></span
              >
            </a-checkbox>
          </div>
          <div v-if="extraTools.length" class="cli-native-extra-tools">
            <a-tag
              v-for="tool in extraTools"
              :key="tool"
              :closable="!disabled"
              @close="toggleTool(tool, false)"
              >{{ tool }}</a-tag
            >
          </div>
          <div v-if="field.allowCustom" class="cli-native-custom-tool">
            <a-input
              v-model="customTool"
              size="small"
              placeholder="补充原生工具名"
              :disabled="disabled"
              @press-enter="addTool"
            />
            <a-button
              size="small"
              :disabled="disabled || !customTool.trim()"
              @click="addTool"
              >添加</a-button
            >
          </div>
          <p
            class="cli-native-tool-count"
            :class="{
              'cli-native-error': selected.length < field.minimumItems,
            }"
          >
            {{
              selected.length
                ? "已选 " + selected.length + " 项"
                : field.minimumItems
                  ? `至少选择一项，或${resetLabel}`
                  : "不提供这些工具／类别"
            }}
          </p>
        </div>
      </template>
      <a-textarea
        v-else-if="field.control === 'rules'"
        :id="inputId"
        :model-value="value ?? ''"
        :disabled="disabled"
        :auto-size="{ minRows: 2, maxRows: 6 }"
        :placeholder="
          persistent
            ? field.id === 'additionalDirectories'
              ? '每行一个目录；清空移除此项设置'
              : '每行一条规则；清空移除此项设置'
            : '追加本次规则，每行一条；留空使用已有规则'
        "
        :aria-label="field.label"
        @update:model-value="
          emit('update:value', $event.trim() ? $event : undefined)
        "
      />

      <div
        v-if="inheritedItems.length"
        class="cli-native-inherited-items"
        aria-label="从现有配置读取的内容"
      >
        <span v-for="item in inheritedItems" :key="item">{{
          field.choices.find((choice) => choice.value === item)?.label || item
        }}</span>
      </div>
      <p
        v-if="effectiveDifference && field.control !== 'modelMappings'"
        class="cli-native-effective"
      >
        {{ persistent ? "保存后生效" : "本次生效" }}：{{ effectiveDifference }}
      </p>
      <p v-if="field.hint" class="cli-native-hint">{{ field.hint }}</p>

      <p v-if="invalidSelection" class="cli-native-error" role="alert">
        {{ invalidSelection }}
      </p>
    </div>
  </section>
</template>
