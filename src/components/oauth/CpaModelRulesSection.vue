<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { ChevronDown } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import type { Provider } from "../../stores/provider-types";
import type {
  CpaModelDefinition,
  CpaModelRuleKind,
  CpaModelRules,
} from "../../stores/oauth-types";
import {
  getCpaModelDefinitions,
  getCpaModelRules,
  updateCpaModelRules,
} from "../../api/oauth-accounts";
import {
  getModelExclusionState,
  normalizeExcludedRules,
  replaceCustomExcludedRules,
  sameExcludedRules,
  splitExcludedRules,
  summarizeExclusion,
  toggleExcludedRule,
} from "../../utils/cpa-excluded-models";
import ContentEditor from "../ContentEditor.vue";

const props = defineProps<{ provider: Provider; revision: number }>();

/// Channels the gateway ships definitions for. The picker also lists whatever
/// the saved rules already name, so a plugin-defined channel stays selectable.
const DEFAULT_CHANNELS = [
  "claude",
  "codex",
  "gemini",
  "aistudio",
  "vertex",
  "xai",
  "kimi",
  "antigravity",
  "devin",
  "meta",
];

const rules = ref<CpaModelRules | null>(null);
const open = ref(false);
const loading = ref(false);
const error = ref("");
const editing = ref<CpaModelRuleKind | null>(null);
const draft = ref("");
const original = ref("");
const saving = ref(false);
const editorError = ref("");
/// The excluded-model editor works one channel at a time. `drafts` keeps each
/// channel's edits apart so switching channels — or opening another one — never
/// throws away work already done in this dialog.
const channel = ref("");
const drafts = ref<Record<string, string[]>>({});
const catalog = ref<CpaModelDefinition[]>([]);
const catalogLoading = ref(false);
const catalogError = ref("");
/// Custom rules live in their own ref rather than being re-derived from the
/// draft on every keystroke, so the textarea does not reorder or trim the line
/// the user is typing.
const customText = ref("");
let rulesRequest = 0;
let catalogRequest = 0;
let disposed = false;
onScopeDispose(() => {
  disposed = true;
  rulesRequest++;
  catalogRequest++;
});
watch(
  () => [props.provider.identity.id, props.revision],
  () => {
    rulesRequest++;
    catalogRequest++;
    rules.value = null;
    open.value = false;
    loading.value = false;
    error.value = "";
    /// A half-written rule survives the refresh; only the transient channel
    /// catalog, which belongs to the gateway that just went away, is dropped.
    catalog.value = [];
    catalogLoading.value = false;
    catalogError.value = "";
    drafts.value = {};
    customText.value = "";
    editorError.value = "";
  },
);

interface RuleRow {
  channel: string;
  count: number;
}
/// Both maps are channel → list, so one reader covers them; anything that is
/// not an object (including a draft that cannot be parsed) shows no rows.
function rows(text: string): RuleRow[] {
  try {
    const value: unknown = JSON.parse(text);
    if (!value || typeof value !== "object" || Array.isArray(value)) return [];
    return Object.entries(value as Record<string, unknown>).map(
      ([channel, items]) => ({
        channel,
        count: Array.isArray(items) ? items.length : 1,
      }),
    );
  } catch {
    return [];
  }
}
/// The saved exclusion map, narrowed to the string lists the gateway stores.
/// A channel whose value is not a list cannot be edited structurally; dropping
/// it is safe because the gateway would reject such a document on save anyway.
function exclusionMap(text: string): Record<string, string[]> {
  let value: unknown;
  try {
    value = JSON.parse(text || "{}");
  } catch {
    return {};
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const map: Record<string, string[]> = {};
  for (const [channelName, items] of Object.entries(
    value as Record<string, unknown>,
  )) {
    if (!Array.isArray(items)) continue;
    map[channelName] = normalizeExcludedRules(
      items.filter((item): item is string => typeof item === "string"),
    );
  }
  return map;
}
const savedExcluded = computed(() =>
  exclusionMap(rules.value?.excludedModels ?? "{}"),
);
const excludedRows = computed(() => rows(rules.value?.excludedModels ?? "{}"));
const aliasRows = computed(() => rows(rules.value?.modelAliases ?? "{}"));
const summary = computed(() => {
  if (loading.value) return "读取模型排除与别名…";
  if (!rules.value) return "查看通道模型排除与别名";
  const excluded = excludedRows.value.reduce((sum, row) => sum + row.count, 0);
  const aliases = aliasRows.value.reduce((sum, row) => sum + row.count, 0);
  if (!excluded && !aliases) return "未配置模型排除或别名";
  return `排除 ${excluded} 个模型 · ${aliases} 个别名`;
});
const aliasHint =
  "键是通道（claude、codex、gemini、xai），值是别名数组：name 为上游模型，alias 为下游看到的名称；fork 为 true 时同时保留原名，另可选 display-name 与 force-mapping。保存会整体替换这张表。";
const aliasDirty = computed(
  () => !!rules.value && draft.value !== original.value,
);

const channelOptions = computed(() => {
  const names = new Set(DEFAULT_CHANNELS);
  for (const name of Object.keys(savedExcluded.value)) names.add(name);
  for (const name of Object.keys(drafts.value)) names.add(name);
  if (channel.value) names.add(channel.value);
  return [...names].sort();
});

/// Rules shown for one channel: the draft when this dialog already edited it,
/// otherwise what the gateway last returned.
function rulesOf(name: string): string[] {
  return drafts.value[name] ?? savedExcluded.value[name] ?? [];
}
const currentRules = computed(() =>
  channel.value ? rulesOf(channel.value) : [],
);
const catalogIds = computed(() => catalog.value.map((model) => model.id));
const split = computed(() =>
  splitExcludedRules(currentRules.value, catalogIds.value),
);
const stats = computed(() =>
  summarizeExclusion(currentRules.value, catalogIds.value),
);
/// Every channel the two sides name, compared without regard to order so a
/// gateway-normalized list does not read as a pending edit.
const excludedDirty = computed(() => {
  const saved = savedExcluded.value;
  const names = new Set([...Object.keys(saved), ...Object.keys(drafts.value)]);
  return [...names].some(
    (name) =>
      !sameExcludedRules(rulesOf(name), saved[name] ?? []),
  );
});

function isChecked(modelId: string): boolean {
  return split.value.exactRules.some(
    (rule) => rule.toLowerCase() === modelId.toLowerCase(),
  );
}
/// The wildcard rule that also hits this model, if any. Saying so matters:
/// unchecking the box would leave the model excluded, which otherwise looks
/// like the click did nothing.
function wildcardFor(modelId: string): string {
  const state = getModelExclusionState(currentRules.value, modelId);
  return state.state === "excluded" && state.by !== "exact" ? state.rule : "";
}
function modelMeta(model: CpaModelDefinition): string {
  const parts: string[] = [];
  if (model.contextLength) parts.push(`${Math.round(model.contextLength / 1000)}K`);
  if (model.ownedBy && model.ownedBy !== model.id) parts.push(model.ownedBy);
  return parts.join(" · ");
}

function toggle() {
  open.value = !open.value;
  if (open.value && !rules.value && !loading.value) void load();
}
async function load() {
  const sequence = ++rulesRequest;
  const id = props.provider.identity.id;
  loading.value = true;
  error.value = "";
  try {
    const result = await getCpaModelRules(id);
    if (!disposed && sequence === rulesRequest) rules.value = result;
  } catch (cause) {
    if (!disposed && sequence === rulesRequest) error.value = String(cause);
  } finally {
    if (sequence === rulesRequest) loading.value = false;
  }
}
function openEditor(kind: CpaModelRuleKind) {
  if (kind === "modelAliases") openAliases();
  else openExcluded();
}
/// One dirty check and one save for both editors, so the modal footer does not
/// need to know which map is open.
function dirty(): boolean {
  return editing.value === "modelAliases" ? aliasDirty.value : excludedDirty.value;
}
async function save() {
  if (editing.value === "modelAliases") await saveAliases();
  else await saveExcluded();
}
function openExcluded() {
  editing.value = "excludedModels";
  editorError.value = "";
  drafts.value = {};
  catalog.value = [];
  catalogError.value = "";
  customText.value = "";
  const saved = Object.keys(savedExcluded.value);
  chooseChannel(saved[0] ?? DEFAULT_CHANNELS[0]);
}
function openAliases() {
  editing.value = "modelAliases";
  editorError.value = "";
  draft.value = rules.value?.modelAliases ?? "{}";
  original.value = draft.value;
}
function closeEditor() {
  editing.value = null;
  draft.value = "";
  original.value = "";
  editorError.value = "";
  catalogRequest++;
  catalog.value = [];
  catalogLoading.value = false;
  catalogError.value = "";
  drafts.value = {};
  customText.value = "";
}
function chooseChannel(raw: unknown) {
  const name = typeof raw === "string" ? raw.trim() : "";
  if (!name) return;
  channel.value = name;
  customText.value = splitExcludedRules(
    rulesOf(name),
    catalogIds.value,
  ).customRules.join("\n");
  void loadCatalog(name);
}
async function loadCatalog(name: string) {
  const sequence = ++catalogRequest;
  catalog.value = [];
  catalogError.value = "";
  catalogLoading.value = true;
  try {
    const models = await getCpaModelDefinitions(
      props.provider.identity.id,
      name,
    );
    if (disposed || sequence !== catalogRequest || channel.value !== name) {
      return;
    }
    catalog.value = models;
    /// Rules that were text-only may now be real catalog IDs; moving them into
    /// checkboxes is what the user expects once the list arrives.
    customText.value = splitExcludedRules(
      rulesOf(name),
      models.map((model) => model.id),
    ).customRules.join("\n");
  } catch (cause) {
    if (!disposed && sequence === catalogRequest && channel.value === name) {
      catalogError.value = String(cause);
    }
  } finally {
    if (sequence === catalogRequest) catalogLoading.value = false;
  }
}
function setDraft(list: string[]) {
  if (!channel.value) return;
  drafts.value = { ...drafts.value, [channel.value]: list };
}
function setCustom(text: string) {
  customText.value = text;
  if (!channel.value) return;
  setDraft(
    replaceCustomExcludedRules(
      rulesOf(channel.value),
      catalogIds.value,
      text,
    ),
  );
}
function toggleModel(modelId: string, excluded: boolean) {
  if (!channel.value) return;
  setDraft(toggleExcludedRule(rulesOf(channel.value), modelId, excluded));
}
function resetExcluded() {
  drafts.value = {};
  if (channel.value) {
    customText.value = splitExcludedRules(
      savedExcluded.value[channel.value] ?? [],
      catalogIds.value,
    ).customRules.join("\n");
  }
}
/// Sends the whole map back, not one channel: CPA replaces the document, so a
/// per-channel write would drop every other channel's rules.
async function saveExcluded() {
  if (saving.value || !excludedDirty.value) return;
  const merged: Record<string, string[]> = { ...savedExcluded.value };
  for (const [name, list] of Object.entries(drafts.value)) {
    const normalized = normalizeExcludedRules(list);
    if (normalized.length) merged[name] = normalized;
    else delete merged[name];
  }
  const body = JSON.stringify(merged, null, 2);
  const name = channel.value;
  saving.value = true;
  editorError.value = "";
  try {
    const result = await updateCpaModelRules(
      props.provider.identity.id,
      "excludedModels",
      body,
      props.revision,
    );
    if (disposed) return;
    /// The gateway normalizes what it stores, so the confirmed rules replace
    /// both the saved copy and the drafts the next save starts from — even
    /// when the dialog was closed while the write was out.
    rules.value = result;
    drafts.value = {};
    if (editing.value === "excludedModels" && name) {
      channel.value = name;
      customText.value = splitExcludedRules(
        savedExcluded.value[name] ?? [],
        catalogIds.value,
      ).customRules.join("\n");
    }
    Message.success("模型排除已保存");
  } catch (cause) {
    if (!disposed) editorError.value = String(cause);
  } finally {
    saving.value = false;
  }
}
async function saveAliases() {
  if (saving.value || !aliasDirty.value) return;
  saving.value = true;
  editorError.value = "";
  try {
    const result = await updateCpaModelRules(
      props.provider.identity.id,
      "modelAliases",
      draft.value,
      props.revision,
    );
    if (disposed) return;
    rules.value = result;
    if (editing.value === "modelAliases") {
      draft.value = result.modelAliases;
      original.value = draft.value;
    }
    Message.success("模型别名已保存");
  } catch (cause) {
    if (!disposed) editorError.value = String(cause);
  } finally {
    saving.value = false;
  }
}
</script>
<template>
  <section class="cpa-model-rules" aria-label="模型排除与别名">
    <button
      type="button"
      class="cpa-model-rules-head"
      :aria-expanded="open"
      @click="toggle"
    >
      <span
        ><strong>模型排除与别名</strong><small>{{ summary }}</small></span
      ><ChevronDown
        :size="13"
        class="cpa-model-rules-caret"
        :class="{ open }"
      />
    </button>
    <div v-if="open" class="cpa-model-rules-body">
      <a-alert v-if="error" type="warning">{{ error }}</a-alert>
      <a-spin v-if="loading" tip="读取模型规则" />
      <template v-else-if="rules">
        <div class="cpa-model-rule">
          <div class="cpa-model-rule-head">
            <strong>模型排除</strong
            ><span>{{
              excludedRows.length ? `${excludedRows.length} 个通道` : "未配置"
            }}</span
            ><a-button size="mini" @click="openEditor('excludedModels')"
              >编辑</a-button
            >
          </div>
          <div v-if="excludedRows.length" class="cpa-model-rule-chips">
            <span v-for="row in excludedRows" :key="row.channel"
              >{{ row.channel }} · {{ row.count }}</span
            >
          </div>
        </div>
        <div class="cpa-model-rule">
          <div class="cpa-model-rule-head">
            <strong>模型别名</strong
            ><span>{{
              aliasRows.length ? `${aliasRows.length} 个通道` : "未配置"
            }}</span
            ><a-button size="mini" @click="openEditor('modelAliases')"
              >编辑</a-button
            >
          </div>
          <div v-if="aliasRows.length" class="cpa-model-rule-chips">
            <span v-for="row in aliasRows" :key="row.channel"
              >{{ row.channel }} · {{ row.count }}</span
            >
          </div>
        </div>
      </template>
    </div>
    <a-modal
      :visible="editing === 'excludedModels'"
      title="网关模型排除"
      width="860px"
      :footer="false"
      unmount-on-close
      @cancel="closeEditor"
    >
      <a-alert v-if="editorError" type="warning">{{ editorError }}</a-alert>
      <div class="cpa-model-excluded">
        <div class="cpa-model-excluded-toolbar">
          <a-select
            :model-value="channel"
            size="small"
            allow-create
            allow-search
            style="width: 180px"
            @change="chooseChannel"
          >
            <a-option v-for="name in channelOptions" :key="name" :value="name">{{
              name
            }}</a-option>
          </a-select>
          <span v-if="catalog.length" class="cpa-model-excluded-stats"
            >{{ stats.total }} 个模型 · 已排除 {{ stats.excluded }}</span
          >
        </div>
        <a-spin v-if="catalogLoading" tip="读取通道模型" />
        <template v-else>
          <a-alert v-if="catalogError" type="warning"
            >{{ catalogError }}。可以继续在下面直接填写模型 ID。</a-alert
          >
          <div v-if="catalog.length" class="cpa-model-catalog">
            <label
              v-for="model in catalog"
              :key="model.id"
              class="cpa-model-catalog-row"
            >
              <a-checkbox
                :model-value="isChecked(model.id)"
                @change="toggleModel(model.id, $event as boolean)"
                ><span class="cpa-model-catalog-id">{{ model.id }}</span></a-checkbox
              >
              <span v-if="wildcardFor(model.id)" class="cpa-model-catalog-note"
                >通配符 {{ wildcardFor(model.id) }} 也会排除</span
              >
              <span class="cpa-model-catalog-meta">{{ modelMeta(model) }}</span>
            </label>
          </div>
        </template>
        <label class="cpa-model-custom">
          <span>通配符与不在目录里的模型（每行一条）</span>
          <a-textarea
            :model-value="customText"
            :auto-size="{ minRows: 4, maxRows: 10 }"
            placeholder="例如 gpt-5-*，或已下线但仍在规则里的模型 ID"
            @update:model-value="setCustom"
          />
        </label>
      </div>
      <p class="cpa-model-rule-hint">
        规则大小写不敏感，只有 <code>*</code> 是通配符，其余字符按字面量匹配。保存会整体替换这张表；某个通道的规则被清空时，该通道会从表中移除。
      </p>
      <div class="cpa-model-rule-savebar">
        <a-button
          size="mini"
          :disabled="!dirty() || saving"
          @click="resetExcluded"
          >放弃修改</a-button
        ><a-button
          size="mini"
          type="primary"
          :disabled="!dirty()"
          :loading="saving"
          @click="save"
          >保存模型排除</a-button
        >
      </div>
    </a-modal>
    <a-modal
      :visible="editing === 'modelAliases'"
      title="网关模型别名"
      width="680px"
      :footer="false"
      unmount-on-close
      @cancel="closeEditor"
    >
      <a-alert v-if="editorError" type="warning">{{ editorError }}</a-alert>
      <div class="cpa-model-rule-editor">
        <ContentEditor
          :model-value="draft"
          :original-text="original"
          format="json"
          label="模型别名"
          @update:model-value="draft = $event"
        />
      </div>
      <p class="cpa-model-rule-hint">{{ aliasHint }}</p>
      <div class="cpa-model-rule-savebar">
        <a-button
          size="mini"
          :disabled="!dirty() || saving"
          @click="draft = original"
          >放弃修改</a-button
        ><a-button
          size="mini"
          type="primary"
          :disabled="!dirty()"
          :loading="saving"
          @click="save"
          >保存模型别名</a-button
        >
      </div>
    </a-modal>
  </section>
</template>
<style scoped>
.cpa-model-rules {
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
  background: var(--color-fill-1);
}
.cpa-model-rules-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  width: 100%;
  padding: 10px 13px;
  border: 0;
  background: none;
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.cpa-model-rules-head > span {
  display: flex;
  align-items: baseline;
  gap: 9px;
  min-width: 0;
}
.cpa-model-rules-head strong {
  color: var(--color-text-1);
  font-size: 12px;
  font-weight: 550;
  white-space: nowrap;
}
.cpa-model-rules-head small {
  overflow: hidden;
  color: var(--color-text-3);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-model-rules-caret {
  flex: 0 0 auto;
  color: var(--color-text-3);
  transition: transform 0.15s ease;
}
.cpa-model-rules-caret.open {
  transform: rotate(180deg);
}
.cpa-model-rules-body {
  display: grid;
  gap: 8px;
  padding: 0 13px 13px;
}
.cpa-model-rule {
  display: grid;
  gap: 6px;
  padding: 9px 11px;
  border: 1px solid var(--color-border-2);
  border-radius: 9px;
}
.cpa-model-rule-head {
  display: flex;
  align-items: center;
  gap: 9px;
}
.cpa-model-rule-head strong {
  color: var(--color-text-1);
  font-size: 12px;
  font-weight: 500;
}
.cpa-model-rule-head > span {
  margin-right: auto;
  color: var(--color-text-3);
  font-size: 11px;
}
.cpa-model-rule-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}
.cpa-model-rule-chips span {
  padding: 0 7px;
  border: 1px solid var(--color-border-2);
  border-radius: 999px;
  background: var(--color-fill-2);
  color: var(--color-text-2);
  font-size: 10px;
  line-height: 17px;
}
.cpa-model-excluded {
  display: grid;
  gap: 9px;
}
.cpa-model-excluded-toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
}
.cpa-model-excluded-stats {
  color: var(--color-text-3);
  font-size: 11px;
}
.cpa-model-catalog {
  display: grid;
  gap: 2px;
  max-height: 280px;
  overflow-y: auto;
  padding: 6px 8px;
  border: 1px solid var(--color-border-2);
  border-radius: 8px;
  background: var(--color-fill-1);
}
.cpa-model-catalog-row {
  display: flex;
  align-items: center;
  gap: 9px;
  min-height: 24px;
  font-size: 12px;
}
.cpa-model-catalog-id {
  color: var(--color-text-1);
  font-family: var(--font-mono, ui-monospace, monospace);
  font-size: 11px;
}
.cpa-model-catalog-note {
  color: #b26a00;
  font-size: 10px;
}
.cpa-model-catalog-meta {
  margin-left: auto;
  color: var(--color-text-3);
  font-size: 10px;
}
.cpa-model-custom {
  display: grid;
  gap: 4px;
  color: var(--color-text-3);
  font-size: 11px;
}
.cpa-model-rule-editor {
  display: flex;
  flex-direction: column;
  height: calc(100vh - 320px);
  min-height: 260px;
}
.cpa-model-rule-editor :deep(.content-editor) {
  flex: 1;
  min-height: 0;
}
.cpa-model-rule-hint {
  margin: 10px 0 0;
  color: var(--color-text-3);
  font-size: 11px;
  line-height: 1.6;
}
.cpa-model-rule-savebar {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 10px;
}
</style>
