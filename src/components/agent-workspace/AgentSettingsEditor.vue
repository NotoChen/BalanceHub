<script setup lang="ts">
import { computed, nextTick, onScopeDispose, ref, watch } from "vue";
import {
  IconCode,
  IconSettings,
  IconRefresh,
  IconInfoCircle,
} from "@arco-design/web-vue/es/icon";
import AgentNativeSettingField from "../AgentNativeSettingField.vue";
import ContentEditor from "../ContentEditor.vue";
import { settingReadFailed } from "../../utils/agent-setting-display";
import { revealInScrollContainer } from "../../utils/scroll-container";
import type {
  AgentConfigurationForm,
  AgentNativeSetting,
  AgentSettingSourceReference,
} from "../../stores/agent-settings-types";
import { agentAssetScopeLabels } from "../../composables/useAgentAssetCatalog";
import {
  useAgentSettingsEditor,
  type AgentSettingsEditorDraft,
  type AgentSettingsEditorProps,
} from "../../composables/useAgentSettingsEditor";
import "../../styles/modules/agent-settings-editor.css";

const props = defineProps<AgentSettingsEditorProps>();
const emit = defineEmits<{
  "update:modelValue": [text: string];
  draft: [value: AgentSettingsEditorDraft];
}>();
const ui = useAgentSettingsEditor(
  props,
  (text) => emit("update:modelValue", text),
  (draft) => emit("draft", draft),
);
const activeTab = ref("model");
const fieldsContainer = ref<HTMLElement | null>(null);
const rawEditor = ref<InstanceType<typeof ContentEditor> | null>(null);
const cursor = ref(0);
const forcedFieldId = ref("");
const highlightedFieldId = ref("");
const highlightedRowId = ref("");
const navigationNotice = ref("");
const navigating = ref(false);
type SourceTarget = {
  field: AgentNativeSetting;
  reference: AgentSettingSourceReference;
};
let navigationRevision = 0;
let highlightTimer: ReturnType<typeof setTimeout> | undefined;
const hasReadError = computed(() =>
  Boolean(ui.form.value?.fields.some(settingReadFailed)),
);
const tabs = computed(() =>
  [
    { id: "model", label: "模型与上下文", sections: ["model"] },
    {
      id: "permissions",
      label: "权限与工具",
      sections: ["permissions", "tools"],
    },
    { id: "context", label: "规则与目录", sections: ["context"] },
  ]
    .map((tab) => ({
      ...tab,
      fields: (ui.form.value?.fields ?? []).filter(
        (field) =>
          tab.sections.includes(field.section) &&
          (ui.visible(field) || field.id === forcedFieldId.value),
      ),
    }))
    .filter((tab) => tab.fields.length),
);
const selectedTab = computed(
  () => tabs.value.find((tab) => tab.id === activeTab.value) ?? tabs.value[0],
);

function sourceAt(
  form: AgentConfigurationForm,
  position: number,
): SourceTarget | null {
  let result: SourceTarget | null = null;
  for (const field of form.fields) {
    for (const reference of field.sourceReferences ?? []) {
      if (position < reference.from || position > reference.valueEnd) continue;
      if (
        !result ||
        reference.valueEnd - reference.from <
          result.reference.valueEnd - result.reference.from
      )
        result = { field, reference };
    }
  }
  return result;
}
const currentSource = computed(() =>
  ui.form.value?.text === props.modelValue
    ? sourceAt(ui.form.value, cursor.value)
    : null,
);
const sourcePending = computed(
  () =>
    ui.pending.value ||
    ui.loading.value ||
    ui.converting.value ||
    ui.form.value?.text !== props.modelValue,
);
const sourceLookupNeeded = computed(
  () =>
    ui.form.value?.text !== props.modelValue &&
    Boolean(props.modelValue.trim()),
);

function clearHighlight() {
  clearTimeout(highlightTimer);
  highlightedFieldId.value = "";
  highlightedRowId.value = "";
}
function resetNavigation() {
  navigationRevision++;
  navigating.value = false;
  navigationNotice.value = "";
  forcedFieldId.value = "";
  clearHighlight();
}
function switchView(mode: "form" | "raw") {
  resetNavigation();
  return mode === "form" ? ui.showForm() : ui.showRaw();
}
function navigationCurrent(
  request: number,
  mode: "form" | "raw",
  text?: string,
) {
  return (
    request === navigationRevision &&
    ui.mode.value === mode &&
    (text === undefined || text === props.modelValue)
  );
}
async function revealSource(id: string, rowId?: string) {
  // Draft edits regenerate source references. Wait for them to finish so a
  // just-removed setting cannot navigate using its previous document location.
  if (sourcePending.value || ui.mode.value !== "form") return;
  const form = ui.form.value;
  const reference = form?.fields
    .find((field) => field.id === id)
    ?.sourceReferences.find((reference) => !rowId || reference.rowId === rowId);
  if (!form || !reference) return;
  const request = ++navigationRevision;
  navigating.value = true;
  navigationNotice.value = "";
  clearHighlight();
  try {
    await ui.showRaw();
    await nextTick();
    if (!navigationCurrent(request, "raw", form.text)) return;
    await rawEditor.value?.reveal(reference.from, reference.to);
  } finally {
    if (request === navigationRevision) navigating.value = false;
  }
}
async function revealSetting() {
  const request = ++navigationRevision;
  const position = cursor.value;
  navigating.value = true;
  navigationNotice.value = "";
  try {
    const form = await ui.readForNavigation();
    if (!form || !navigationCurrent(request, "raw", form.text)) return;
    const target = sourceAt(form, position);
    if (!target) {
      navigationNotice.value =
        "光标所在位置没有对应的可视化设置。可继续编辑原文。";
      return;
    }
    forcedFieldId.value = target.field.id;
    const result = await ui.showForm();
    if (!result || !navigationCurrent(request, "form")) return;
    activeTab.value =
      tabs.value.find((tab) =>
        tab.fields.some((field) => field.id === target.field.id),
      )?.id ?? activeTab.value;
    clearHighlight();
    highlightedFieldId.value = target.field.id;
    highlightedRowId.value = target.reference.rowId ?? "";
    await nextTick();
    if (!navigationCurrent(request, "form")) return;
    const field = [
      ...(fieldsContainer.value?.querySelectorAll<HTMLElement>(
        "[data-setting-id]",
      ) ?? []),
    ].find((element) => element.dataset.settingId === target.field.id);
    const row = target.reference.rowId
      ? [
          ...(field?.querySelectorAll<HTMLElement>("[data-mapping-id]") ?? []),
        ].find(
          (element) => element.dataset.mappingId === target.reference.rowId,
        )
      : null;
    const element = row ?? field;
    revealInScrollContainer(fieldsContainer.value, element);
    element?.focus({ preventScroll: true });
    highlightTimer = setTimeout(clearHighlight, 2200);
  } finally {
    if (request === navigationRevision) navigating.value = false;
  }
}
watch(() => [props.editId, props.sourceId, props.revision], resetNavigation, {
  flush: "sync",
});
watch(
  () => props.modelValue,
  () => {
    navigationNotice.value = "";
  },
);
onScopeDispose(resetNavigation);
</script>

<template>
  <div class="agent-settings-editor">
    <header class="agent-settings-editor-toolbar">
      <div class="agent-settings-view-switch" aria-label="配置编辑方式">
        <button
          type="button"
          :aria-pressed="ui.mode.value === 'form'"
          @click="switchView('form')"
        >
          <icon-settings />可视化
        </button>
        <button
          type="button"
          :aria-pressed="ui.mode.value === 'raw'"
          @click="switchView('raw')"
        >
          <icon-code />原文
        </button>
      </div>
      <span v-if="ui.form.value" class="agent-settings-scope"
        >{{ agentAssetScopeLabels[ui.form.value.scope]
        }}<template v-if="ui.form.value.profile">
          · {{ ui.form.value.profile }}</template
        ></span
      >
      <a-popover
        v-if="ui.form.value"
        trigger="click"
        position="bottom"
        title="可视化覆盖范围"
        content-class="cli-native-explanation"
      >
        <button
          type="button"
          class="agent-settings-coverage"
          aria-label="查看可视化配置覆盖范围"
        >
          {{ ui.form.value.fields.length }} 项设置 <icon-info-circle />
        </button>
        <template #content>
          <p>
            这里提供当前
            Agent、模型和文件范围支持的常用设置，尚未覆盖全部官方配置。
          </p>
          <p>
            MCP、Skill、Hook
            与插件定义在资产管理中编辑。自定义角色、复杂环境变量等高级配置可切换到原文编辑，已有内容会保留。
          </p>
          <a-button type="text" size="small" @click="switchView('raw')"
            >编辑原文</a-button
          >
        </template>
      </a-popover>
      <span
        v-if="ui.converting.value"
        class="agent-settings-saving"
        role="status"
        >整理草稿…</span
      >
      <a-button v-if="ui.error.value" type="text" size="small" @click="ui.retry"
        ><template #icon><icon-refresh /></template>重新读取</a-button
      >
    </header>
    <p v-if="ui.error.value" class="agent-settings-error" role="alert">
      {{ ui.error.value }}
    </p>
    <p
      v-if="navigationNotice"
      class="agent-settings-note agent-settings-location-note"
      role="status"
    >
      {{ navigationNotice }}
    </p>
    <template v-if="ui.mode.value === 'form'">
      <div
        v-if="ui.loading.value"
        class="agent-settings-skeleton"
        role="status"
        aria-label="正在读取原生配置选项"
      >
        <i /><i /><i />
      </div>
      <template v-else-if="ui.form.value">
        <nav class="agent-settings-section-tabs" aria-label="配置分类">
          <button
            v-for="tab in tabs"
            :key="tab.id"
            type="button"
            :aria-pressed="selectedTab?.id === tab.id"
            @click="activeTab = tab.id"
          >
            {{ tab.label }}
          </button>
        </nav>
        <div
          ref="fieldsContainer"
          class="agent-settings-fields"
          :key="selectedTab?.id"
        >
          <AgentNativeSettingField
            v-for="field in selectedTab?.fields ?? []"
            :key="field.id"
            :field="field"
            :class="{
              'is-source-target':
                field.id === highlightedFieldId && !highlightedRowId,
            }"
            :highlighted-row-id="
              field.id === highlightedFieldId ? highlightedRowId : ''
            "
            :model-catalog="ui.form.value.modelCatalog"
            :value="ui.valueFor(field.id)"
            mode="configuration"
            :effective="ui.form.value.effective[field.id]"
            :inherited="ui.form.value.inherited[field.id]"
            :disabled="readonly"
            :source-pending="sourcePending"
            @update:value="ui.change(field.id, $event)"
            @reveal-source="revealSource(field.id, $event)"
          />
          <p v-if="!selectedTab" class="agent-settings-note">
            此配置范围没有可视化字段，可以切换到原文编辑。
          </p>
        </div>
        <details
          v-if="ui.form.value.notices.length"
          class="agent-settings-notices"
          :class="{ 'has-read-error': hasReadError }"
          :open="hasReadError"
        >
          <summary>
            {{ hasReadError ? "配置读取存在问题" : "配置来源说明" }}
          </summary>
          <p v-for="notice in ui.form.value.notices" :key="notice">
            {{ notice }}
          </p>
        </details>
      </template>
    </template>
    <template v-else>
      <p v-if="ui.rawNotice.value" class="agent-settings-note" role="status">
        {{ ui.rawNotice.value
        }}<a-button size="mini" type="text" @click="ui.useRaw"
          >以当前原文为准</a-button
        >
      </p>
      <ContentEditor
        ref="rawEditor"
        :model-value="modelValue"
        :original-text="originalText"
        :format="format"
        :readonly="readonly"
        @update:model-value="ui.setRaw"
        @cursor="cursor = $event"
      >
        <template #navigation="{ showingDiff }">
          <button
            v-if="!showingDiff && (currentSource || sourceLookupNeeded)"
            type="button"
            class="agent-settings-source-action"
            :disabled="navigating || ui.loading.value || ui.converting.value"
            :title="
              currentSource
                ? `定位到 ${currentSource.field.label}（${currentSource.reference.key}）`
                : '解析当前草稿，再查找光标所在配置对应的可视化设置'
            "
            @click="revealSetting"
          >
            <icon-settings />{{
              navigating || ui.loading.value
                ? "正在定位…"
                : currentSource
                  ? `可视化 · ${currentSource.field.label}`
                  : "解析并定位"
            }}
          </button>
        </template>
      </ContentEditor>
    </template>
  </div>
</template>
