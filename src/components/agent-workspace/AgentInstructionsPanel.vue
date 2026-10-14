<script setup lang="ts">
import { computed, onScopeDispose, reactive, ref, watch } from "vue";
import { BookOpen, Info, Plus, RefreshCw } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import {
  beginAgentInstructionEdit,
  deleteAgentInstructionSet,
  listAgentInstructionSets,
  listAgentInstructionTargets,
  saveAgentInstructionSet,
  type AgentInstructionSet,
} from "../../api/agent-instructions";
import { useAgentWslStore } from "../../stores/agent-wsl";
import { useAgentConfigurationSourcesStore } from "../../stores/agent-configuration-sources";
import type {
  AgentCliDescriptor,
  AgentCliKind,
  AgentRuntimeScope,
} from "../../stores/provider-types";
import type { AgentConfigurationSource } from "../../stores/agent-configuration-types";
import { useAgentConfigurationEditor } from "../../composables/useAgentConfigurationEditor";
import { agentConfigurationErrorMessage } from "../../utils/agent-configuration-display";
import { withTimeout } from "../../utils/promise-timeout";
import AgentConfigurationEditorModal from "./AgentConfigurationEditorModal.vue";
import AgentInstructionEditorModal from "./AgentInstructionEditorModal.vue";
import AgentInstructionTargetModal from "./AgentInstructionTargetModal.vue";
import AgentInstructionSources from "./AgentInstructionSources.vue";
import AgentCliIcon from "../AgentCliIcon.vue";

const props = defineProps<{
  agents: AgentCliDescriptor[];
  active: boolean;
  workspace?: string;
  query: string;
}>();
const existingRules = ref<InstanceType<typeof AgentInstructionSources> | null>(null);
const wsl = useAgentWslStore();
const configurations = useAgentConfigurationSourcesStore();
const targetEnvironment = ref("");
watch(() => wsl.supported, (supported) => {
  if (supported === false) targetEnvironment.value = "";
});
const targetScope = computed<AgentRuntimeScope>(() =>
  targetEnvironment.value
    ? { kind: "wsl", distro_id: targetEnvironment.value }
    : { kind: "native" },
);
const targetWorkspace = computed(() =>
  targetEnvironment.value ? undefined : props.workspace,
);
const sets = ref<AgentInstructionSet[]>([]);
const loading = ref(false);
const error = ref("");
const saving = ref(false);
const deleting = ref("");
const draft = ref<AgentInstructionSet | null>(null);
const draftOpen = ref(false);
const draftError = ref("");
const savedTemplate = ref<AgentInstructionSet | null>(null);
const targetSet = ref<AgentInstructionSet | null>(null);
const targetKind = ref<AgentCliKind | null>(null);
const targets = ref<AgentConfigurationSource[]>([]);
const selectedSource = ref("");
const targetLoading = ref(false);
const targetError = ref("");
const editor = reactive(useAgentConfigurationEditor());
let revision = 0;
let targetRevision = 0;
let draftRevision = 0;
let disposed = false;
let targetController: AbortController | null = null;
const visibleSets = computed(() =>
  sets.value.filter((set) =>
    [set.name, set.common, ...Object.values(set.additions)].join("\n")
      .toLocaleLowerCase()
      .includes(props.query.trim().toLocaleLowerCase()),
  ),
);
const original = computed(() =>
  sets.value.find((set) => set.id === draft.value?.id),
);
const source = computed(() =>
  targets.value.find((source) => source.sourceId === selectedSource.value),
);
const writable = computed(() =>
  source.value?.access.kind === "ready" &&
  source.value?.actions.some(
    (action) =>
      (action.action === "edit" || action.action === "create") &&
      action.available,
  ),
);
function failure(value: unknown, fallback = "操作失败，请重试") {
  return agentConfigurationErrorMessage(value, fallback);
}
function summary(set: AgentInstructionSet) {
  return set.common.trim().split(/\r?\n/).filter(Boolean).slice(0, 2)
    .map((line) => line.replace(/^#{1,6}\s+/, "")).join(" ")
    || (Object.values(set.additions).some((value) => value?.trim())
      ? "已填写 Agent 专用补充，通用规则留空。"
      : "还没有规则内容，编辑后可应用到 Agent。");
}
function additions(set: AgentInstructionSet) {
  return props.agents.filter((agent) => set.additions[agent.kind]?.trim());
}
async function refreshTemplates() {
  const current = ++revision;
  loading.value = true;
  error.value = "";
  savedTemplate.value = null;
  try {
    const result = await withTimeout(
      listAgentInstructionSets(),
      16_000,
      "读取规则模板超时",
    );
    if (!disposed && current === revision) sets.value = result;
  } catch (e) {
    if (!disposed && current === revision)
      error.value = failure(e, "规则模板读取失败，请刷新重试");
  } finally {
    if (current === revision) loading.value = false;
  }
}
function refresh() {
  return Promise.allSettled([refreshTemplates(), existingRules.value?.refresh()]);
}
function edit(set?: AgentInstructionSet) {
  if (draft.value) {
    if (set && draft.value.id !== set.id)
      Message.info("请先保存或放弃当前规则草稿，再编辑其他规则");
    draftOpen.value = true;
    return;
  }
  draft.value = set
    ? { ...set, additions: { ...set.additions } }
    : { id: "", revision: "", name: "", common: "", additions: {} };
  draftRevision += 1;
  draftError.value = "";
  savedTemplate.value = null;
  draftOpen.value = true;
}
function discardDraft() {
  if (saving.value) return;
  draftRevision += 1;
  draft.value = null;
  draftOpen.value = false;
  draftError.value = "";
}
async function save() {
  if (!draft.value || saving.value) return;
  const input = { ...draft.value, additions: { ...draft.value.additions } };
  const current = draftRevision;
  saving.value = true;
  draftError.value = "";
  try {
    const result = await withTimeout(
      saveAgentInstructionSet({
        id: input.id || null,
        expectedRevision: input.revision || null,
        name: input.name,
        common: input.common,
        additions: input.additions,
      }),
      16_000,
      "保存尚未确认，请刷新共享库核对结果",
    );
    if (disposed) return;
    revision += 1;
    loading.value = false;
    sets.value = [...sets.value.filter((set) => set.id !== result.id), result];
    savedTemplate.value = result;
    if (current === draftRevision) {
      draft.value = null;
      draftOpen.value = false;
    }
    Message.success("规则模板已保存，应用到 Agent 后才会写入规则文件");
  } catch (e) {
    if (!disposed)
      draftError.value = failure(e, "规则模板保存未确认，草稿已保留，请刷新后核对");
  } finally {
    saving.value = false;
  }
}
async function removeSet(set: AgentInstructionSet) {
  if (deleting.value) return;
  deleting.value = set.id;
  try {
    await withTimeout(
      deleteAgentInstructionSet(set.id, set.revision),
      16_000,
      "删除尚未确认，请刷新共享库核对结果",
    );
    if (!disposed) {
      revision += 1;
      loading.value = false;
      sets.value = sets.value.filter((item) => item.id !== set.id);
      if (savedTemplate.value?.id === set.id) savedTemplate.value = null;
      Message.success("规则模板已删除，Agent 文件中的内容保留");
    }
  } catch (e) {
    if (!disposed) error.value = failure(e);
  } finally {
    deleting.value = "";
  }
}
function closeTargets() {
  targetRevision += 1;
  targetController?.abort();
  targetController = null;
  targetSet.value = null;
  targetKind.value = null;
  targets.value = [];
  selectedSource.value = "";
  targetLoading.value = false;
}
async function loadTargets() {
  const kind = targetKind.value;
  const scope = targetScope.value;
  const workspace = targetWorkspace.value;
  const current = ++targetRevision;
  targetController?.abort();
  targetController = null;
  targets.value = [];
  selectedSource.value = "";
  targetError.value = "";
  if (!kind) {
    targetLoading.value = false;
    return;
  }
  const controller = new AbortController();
  targetController = controller;
  targetLoading.value = true;
  try {
    const result = await configurations.forRuntime(scope).transaction(
      kind,
      () => withTimeout(listAgentInstructionTargets(kind, workspace, scope), 16_000, "读取规则文件超时"),
      20_000,
      controller.signal,
    );
    if (disposed || current !== targetRevision) return;
    targets.value = result;
    selectedSource.value =
      result.find(
        (item) =>
          item.actions.some(
            (action) =>
              action.available &&
              (action.action === "edit" || action.action === "create"),
          ) &&
          (!targetWorkspace.value || item.scope === "workspace"),
      )?.sourceId ??
      result[0]?.sourceId ??
      "";
  } catch (e) {
    if (!disposed && current === targetRevision)
      targetError.value = failure(e, "规则文件读取失败，请重新读取");
  } finally {
    if (targetController === controller) targetController = null;
    if (current === targetRevision) targetLoading.value = false;
  }
}
watch([targetKind, targetEnvironment], () => {
  void loadTargets();
});
watch(() => props.workspace, closeTargets);
function configure(set: AgentInstructionSet) {
  void wsl.ensure();
  savedTemplate.value = null;
  targetSet.value = set;
  targetKind.value = props.agents[0]?.kind ?? null;
}
function editTarget() {
  const set = targetSet.value;
  closeTargets();
  if (set) edit(set);
}
function preview(remove = false) {
  const set = targetSet.value;
  const selected = source.value;
  if (!set || !selected || selected.access.kind !== "ready" || !writable.value)
    return;
  closeTargets();
  void editor.open(
    async (isCurrent, publish) => {
      const snapshot = await publish(selected.workspace);
      if (!isCurrent()) throw new Error("规则读取已取消");
      const fresh = snapshot.sources.find(
        (item) => item.sourceId === selected.sourceId,
      );
      if (!fresh || fresh.access.kind !== "ready")
        throw new Error("规则文件已变化，请重新选择");
      return beginAgentInstructionEdit(
        set.id,
        set.revision,
        {
          sourceId: fresh.sourceId,
          accessId: fresh.access.accessId,
          environmentId: fresh.environmentId,
          workspace: fresh.workspace,
          expectedRevision: fresh.revision.identity,
        },
        remove,
      );
    },
    {
      agentKind: selected.agentKind,
      workspace: selected.workspace,
      runtimeScope: selected.runtimeScope,
      label: remove ? "移除文件中的共享规则" : `应用规则 · ${set.name}`,
    },
    `instructions:${set.id}:${selected.sourceId}:${remove}`,
  );
}
onScopeDispose(() => {
  disposed = true;
  targetController?.abort();
  targetController = null;
  revision += 1;
  targetRevision += 1;
});
void refreshTemplates();
defineExpose({ refresh });
</script>

<template>
  <section class="agent-instructions-panel" aria-label="工作规则">
    <AgentInstructionSources ref="existingRules" :active="active" :agents="agents" :workspace="workspace" :query="query" />
    <section class="instruction-library" aria-labelledby="instruction-library-title" :aria-busy="loading || undefined">
      <header class="instruction-library-header">
        <div class="instruction-library-heading">
          <h2 id="instruction-library-title">规则模板</h2>
          <span v-if="sets.length || !loading" class="instruction-library-count">{{ visibleSets.length }} 份</span>
          <a-popover trigger="click" position="bottom" title="保存与应用">
            <a-button type="text" size="mini" shape="circle" class="instruction-help-button" aria-label="了解规则模板的保存与应用"><Info :size="14" /></a-button>
            <template #content>
              <div class="instruction-help-copy">
                <p>模板保存在 BalanceHub 共享库，可随 WebDAV 同步。应用到 Agent 时，选择规则文件，预览差异并确认后才会写入。</p>
                <p>修改模板后需重新应用；删除模板会保留已写入 Agent 的规则。</p>
              </div>
            </template>
          </a-popover>
        </div>
        <div class="instruction-actions">
          <a-button type="text" size="small" :loading="loading" @click="refreshTemplates"><template #icon><RefreshCw :size="14" /></template>刷新</a-button>
          <a-button type="primary" size="small" @click="edit()"><template #icon><Plus :size="14" /></template>{{ draft ? '继续编辑草稿' : '新建模板' }}</a-button>
        </div>
      </header>
      <div v-if="savedTemplate" class="instruction-notice" role="status">
        <div><strong>「{{ savedTemplate.name }}」模板已保存</strong><p>选择 Agent，预览并应用这份规则。</p></div>
        <a-button size="small" type="primary" @click="configure(savedTemplate)">应用到 Agent</a-button>
      </div>
      <div v-if="draft && !draftOpen" class="instruction-notice" role="status">
        <div>
          <strong>{{ saving ? '正在保存模板' : '已保留草稿' }} · {{ draft.name || '新规则' }}</strong>
          <p v-if="draftError" class="agent-workspace-error" role="alert">{{ draftError }}</p>
          <p v-else>{{ saving ? '保存会在后台完成，结果显示在此处。' : '退出 App 前请保存草稿。' }}</p>
        </div>
        <a-button size="small" @click="draftOpen = true">继续编辑</a-button>
      </div>
      <p v-if="error" class="instruction-library-error agent-workspace-error" role="alert">{{ error }}</p>
      <div v-if="loading && !sets.length" class="instruction-empty" role="status">正在读取规则模板…</div>
      <div v-else-if="!error && !visibleSets.length" class="instruction-empty" role="status">
        <BookOpen :size="22" aria-hidden="true" />
        <div>
          <strong>{{ query.trim() ? '没有匹配的规则模板' : '还没有规则模板' }}</strong>
          <p>{{ query.trim() ? '试试规则名称或内容中的关键词。' : '把常用约定保存下来，按需应用到多个 Agent。' }}</p>
        </div>
      </div>
      <article v-for="set in visibleSets" :key="set.id" class="instruction-scheme">
        <div class="instruction-scheme-content">
          <h3>{{ set.name }}</h3>
          <p class="instruction-scheme-preview">{{ summary(set) }}</p>
          <div v-if="additions(set).length" class="instruction-addition-labels">
            <span>专用补充</span>
            <span v-for="agent in additions(set)" :key="agent.kind"><AgentCliIcon :kind="agent.kind" :size="14" />{{ agent.label }}</span>
          </div>
        </div>
        <div class="instruction-actions">
          <a-button size="small" :aria-label="`编辑模板 ${set.name}`" @click="edit(set)">编辑</a-button>
          <a-button size="small" type="outline" @click="configure(set)">应用到 Agent</a-button>
          <a-popconfirm content="删除这份规则模板？已写入 Agent 文件的规则会保留。" @ok="removeSet(set)">
            <a-button size="small" type="text" status="danger" :aria-label="`删除模板 ${set.name}`" :loading="deleting === set.id">删除</a-button>
          </a-popconfirm>
        </div>
      </article>
    </section>
    <AgentInstructionEditorModal
      v-model:draft="draft"
      :visible="draftOpen"
      :original="original"
      :agents="agents"
      :saving="saving"
      :error="draftError"
      @close="draftOpen = false"
      @discard="discardDraft"
      @save="save"
    />
    <AgentInstructionTargetModal
      :set="targetSet"
      :agents="agents"
      :distributions="wsl.supported === true ? wsl.inventory?.distributions : undefined"
      v-model:environment="targetEnvironment"
      v-model:agent-kind="targetKind"
      :targets="targets"
      :source="source"
      :loading="targetLoading"
      :error="targetError"
      :writable="Boolean(writable)"
      @update:source-id="selectedSource = $event"
      @preview="preview"
      @edit="editTarget"
      @retry="loadTargets"
      @close="closeTargets"
    />
    <AgentConfigurationEditorModal :model="editor" />
  </section>
</template>

<style scoped>
.agent-instructions-panel { display: flex; width: 100%; max-width: 1120px; min-width: 0; flex: 0 0 auto; flex-direction: column; gap: 18px; margin: 0 auto; padding: 6px 0 16px; color: var(--surface-ink); container: agent-instructions / inline-size; }
.instruction-library { min-width: 0; border: 1px solid var(--surface-line); border-radius: 12px; background: var(--surface-panel); }
.instruction-library-header { display: flex; min-width: 0; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px; padding: 14px 18px; }
.instruction-library-heading { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
.instruction-library-heading h2 { margin: 0; color: var(--surface-ink); font-size: 15px; font-weight: 650; }
.instruction-library-count { color: var(--surface-ink-soft); font-size: 12px; font-variant-numeric: tabular-nums; }
.instruction-help-button { color: var(--surface-ink-soft); }
.instruction-help-copy { max-width: min(320px, calc(100vw - 72px)); color: var(--surface-ink-soft); font-size: 12px; line-height: 1.75; overflow-wrap: anywhere; }
.instruction-help-copy p { margin: 0; }
.instruction-help-copy p + p { margin-top: 8px; }
.instruction-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
.instruction-scheme { display: grid; min-width: 0; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 12px 24px; border-top: 1px solid var(--surface-line); padding: 16px 18px; }
.instruction-scheme-content { min-width: 0; }
.instruction-scheme h3 { margin: 0 0 4px; color: var(--surface-ink); font-size: 14px; font-weight: 600; overflow-wrap: anywhere; }
.instruction-scheme-preview { display: -webkit-box; overflow: hidden; margin: 0; -webkit-box-orient: vertical; -webkit-line-clamp: 2; color: var(--surface-ink-soft); font-size: 13px; line-height: 1.7; overflow-wrap: anywhere; }
.instruction-addition-labels { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 14px; margin-top: 8px; color: var(--surface-ink-soft); font-size: 12px; }
.instruction-addition-labels > span { display: inline-flex; align-items: center; gap: 5px; }
.instruction-notice { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: 12px; margin: 0 18px 14px; border-left: 3px solid var(--surface-accent); border-radius: 5px; padding: 10px 12px; background: var(--surface-accent-soft); }
.instruction-notice > div { flex: 1 1 220px; min-width: 0; overflow-wrap: anywhere; }
.instruction-notice strong { font-size: 13px; font-weight: 600; }
.instruction-notice p { margin: 5px 0 0; color: var(--surface-ink-soft); font-size: 12px; }
.instruction-library-error { margin: 0 18px 14px; }
.instruction-empty { display: flex; align-items: center; gap: 12px; border-top: 1px solid var(--surface-line); padding: 20px 18px; color: var(--surface-ink-soft); font-size: 12px; }
.instruction-empty > svg { flex: 0 0 auto; }
.instruction-empty strong { color: var(--surface-ink); font-size: 13px; font-weight: 500; }
.instruction-empty p { margin: 4px 0 0; line-height: 1.7; }
@container agent-instructions (max-width: 640px) {
  .instruction-library-header, .instruction-scheme, .instruction-empty { padding-right: 14px; padding-left: 14px; }
  .instruction-scheme { grid-template-columns: minmax(0, 1fr); }
  .instruction-notice, .instruction-library-error { margin-right: 14px; margin-left: 14px; }
}
</style>
