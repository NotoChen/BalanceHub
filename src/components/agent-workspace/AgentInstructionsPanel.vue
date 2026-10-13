<script setup lang="ts">
import { computed, onScopeDispose, reactive, ref, watch } from "vue";
import { Plus, RefreshCw } from "@lucide/vue";
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
import AgentCliIcon from "../AgentCliIcon.vue";
import ContentEditor from "../ContentEditor.vue";
import FileEditorModal from "../FileEditorModal.vue";

const props = defineProps<{
  agents: AgentCliDescriptor[];
  workspace?: string;
  query: string;
}>();
const wsl = useAgentWslStore();
const targetEnvironment = ref("");
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
const section = ref<"common" | AgentCliKind>("common");
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
const visibleSets = computed(() =>
  sets.value.filter((set) =>
    `${set.name}\n${set.common}`
      .toLocaleLowerCase()
      .includes(props.query.trim().toLocaleLowerCase()),
  ),
);
const original = computed(() =>
  sets.value.find((set) => set.id === draft.value?.id),
);
const originalText = computed(() =>
  section.value === "common"
    ? (original.value?.common ?? "")
    : (original.value?.additions[section.value] ?? ""),
);
const text = computed({
  get: () =>
    !draft.value
      ? ""
      : section.value === "common"
        ? draft.value.common
        : (draft.value.additions[section.value] ?? ""),
  set: (value: string) => {
    if (!draft.value) return;
    if (section.value === "common") draft.value.common = value;
    else draft.value.additions[section.value] = value;
  },
});
const source = computed(() =>
  targets.value.find((source) => source.sourceId === selectedSource.value),
);
const writable = computed(() =>
  source.value?.actions.some(
    (action) =>
      (action.action === "edit" || action.action === "create") &&
      action.available,
  ),
);
function failure(value: unknown) {
  return agentConfigurationErrorMessage(value, "操作失败，请重试");
}
async function refresh() {
  const current = ++revision;
  loading.value = true;
  error.value = "";
  try {
    const result = await withTimeout(
      listAgentInstructionSets(),
      16_000,
      "读取共享指令超时",
    );
    if (!disposed && current === revision) sets.value = result;
  } catch (e) {
    if (!disposed && current === revision) error.value = failure(e);
  } finally {
    if (current === revision) loading.value = false;
  }
}
function edit(set?: AgentInstructionSet) {
  if (draft.value) {
    draftOpen.value = true;
    return;
  }
  draft.value = set
    ? { ...set, additions: { ...set.additions } }
    : { id: "", revision: "", name: "", common: "", additions: {} };
  draftRevision += 1;
  section.value = "common";
  draftOpen.value = true;
}
async function save() {
  if (!draft.value || saving.value) return;
  const input = { ...draft.value, additions: { ...draft.value.additions } };
  const current = draftRevision;
  saving.value = true;
  error.value = "";
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
    if (current === draftRevision) {
      draft.value = null;
      draftOpen.value = false;
    }
    Message.success("指令方案已保存到共享库");
  } catch (e) {
    if (!disposed) error.value = failure(e);
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
    }
  } catch (e) {
    if (!disposed) error.value = failure(e);
  } finally {
    deleting.value = "";
  }
}
function closeTargets() {
  targetRevision += 1;
  targetSet.value = null;
  targetKind.value = null;
  targets.value = [];
  selectedSource.value = "";
  targetLoading.value = false;
}
async function loadTargets() {
  const kind = targetKind.value;
  const current = ++targetRevision;
  targets.value = [];
  selectedSource.value = "";
  targetError.value = "";
  if (!kind) {
    targetLoading.value = false;
    return;
  }
  targetLoading.value = true;
  try {
    const result = await withTimeout(
      listAgentInstructionTargets(
        kind,
        targetWorkspace.value,
        targetScope.value,
      ),
      16_000,
      "读取指令文件超时",
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
    if (!disposed && current === targetRevision) targetError.value = failure(e);
  } finally {
    if (current === targetRevision) targetLoading.value = false;
  }
}
watch([targetKind, targetEnvironment], () => {
  void loadTargets();
});
watch(() => props.workspace, closeTargets);
function configure(set: AgentInstructionSet) {
  void wsl.ensure();
  targetSet.value = set;
  targetKind.value = props.agents[0]?.kind ?? null;
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
      if (!isCurrent()) throw new Error("指令读取已取消");
      const fresh = snapshot.sources.find(
        (item) => item.sourceId === selected.sourceId,
      );
      if (!fresh || fresh.access.kind !== "ready")
        throw new Error("指令文件已变化，请重新选择");
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
      label: remove ? "移除共享指令" : `配置指令 · ${set.name}`,
    },
    `instructions:${set.id}:${selected.sourceId}:${remove}`,
  );
}
onScopeDispose(() => {
  disposed = true;
  revision += 1;
  targetRevision += 1;
});
void refresh();
defineExpose({ refresh });
</script>

<template>
  <section class="agent-instructions-panel">
    <header class="agent-section-toolbar">
      <div>
        <strong>共享指令</strong>
        <p>统一公共规则，为各 Agent 保留补充内容。</p>
      </div>
      <div class="instruction-actions">
        <a-button size="small" :loading="loading" @click="refresh"
          ><template #icon><RefreshCw :size="14" /></template>刷新</a-button
        ><a-button type="primary" size="small" @click="edit()"
          ><template #icon><Plus :size="14" /></template>新建方案</a-button
        >
      </div>
    </header>
    <p v-if="error" class="agent-workspace-error" role="alert">{{ error }}</p>
    <div v-if="!visibleSets.length" class="agent-workspace-empty">
      {{
        loading
          ? "正在读取共享指令…"
          : query
            ? "没有匹配的指令方案"
            : "还没有指令方案。新建后可分别配置到各 Agent。"
      }}
    </div>
    <article
      v-for="set in visibleSets"
      :key="set.id"
      class="instruction-scheme"
    >
      <div>
        <h3>{{ set.name }}</h3>
        <p>
          {{ set.common.trim().split("\n")[0] || "使用各 Agent 的补充正文" }}
        </p>
        <small
          >{{
            Object.values(set.additions).filter((value) => value?.trim()).length
          }}
          个 Agent 有补充内容</small
        >
      </div>
      <div class="instruction-actions">
        <a-button size="small" @click="edit(set)">编辑</a-button
        ><a-button size="small" type="primary" @click="configure(set)"
          >配置到 Agent</a-button
        ><a-popconfirm
          content="删除共享方案？已写入 Agent 的指令会保留。"
          @ok="removeSet(set)"
          ><a-button
            size="small"
            type="text"
            status="danger"
            :loading="deleting === set.id"
            >删除</a-button
          ></a-popconfirm
        >
      </div>
    </article>
    <FileEditorModal :visible="draftOpen" fill @close="draftOpen = false">
      <template #title>编辑共享指令</template>
      <div v-if="draft" class="instruction-editor">
        <a-input
          v-model="draft.name"
          aria-label="方案名称"
          placeholder="方案名称"
          :max-length="100"
          :disabled="saving"
        />
        <div class="instruction-sections" role="group" aria-label="指令正文">
          <a-button
            size="small"
            :type="section === 'common' ? 'primary' : 'text'"
            @click="section = 'common'"
            >公共正文</a-button
          ><a-button
            v-for="agent in agents"
            :key="agent.kind"
            size="small"
            :type="section === agent.kind ? 'primary' : 'text'"
            @click="section = agent.kind"
            ><template #icon
              ><AgentCliIcon :kind="agent.kind" :size="15" /></template
            >{{ agent.label }}</a-button
          >
        </div>
        <p>
          {{
            section === "common"
              ? "公共正文会用于每个 Agent。"
              : "这部分会追加在公共正文后。留空则只使用公共正文。"
          }}
        </p>
        <ContentEditor
          :key="section"
          v-model="text"
          :original-text="originalText"
          format="markdown"
          :readonly="saving"
          label="指令正文"
        />
        <p v-if="error" class="agent-workspace-error" role="alert">
          {{ error }}
        </p>
        <footer>
          <a-button
            :disabled="saving"
            @click="
              draft = null;
              draftOpen = false;
            "
            >放弃草稿</a-button
          ><a-button @click="draftOpen = false">保留草稿并关闭</a-button
          ><a-button
            type="primary"
            :loading="saving"
            :disabled="!draft.name.trim()"
            @click="save"
            >保存到共享库</a-button
          >
        </footer>
      </div>
    </FileEditorModal>
    <a-modal
      :visible="Boolean(targetSet)"
      title="配置共享指令"
      :footer="false"
      closable
      mask-closable
      esc-to-close
      @cancel="closeTargets"
    >
      <div class="instruction-target">
        <a-select
          v-if="wsl.inventory?.distributions.length"
          v-model="targetEnvironment"
          aria-label="目标运行环境"
          ><a-option value="">本机</a-option
          ><a-option
            v-for="distro in wsl.inventory.distributions"
            :key="distro.id"
            :value="distro.id"
            :disabled="!distro.running || Boolean(distro.error)"
            >WSL · {{ distro.name
            }}{{ distro.running ? "" : "（已停止）" }}</a-option
          ></a-select
        ><a-select
          :model-value="targetKind ?? undefined"
          @change="(value) => (targetKind = value as AgentCliKind)"
          aria-label="目标 Agent"
          ><a-option
            v-for="agent in agents"
            :key="agent.kind"
            :value="agent.kind"
            >{{ agent.label }}</a-option
          ></a-select
        ><a-select
          v-model="selectedSource"
          :loading="targetLoading"
          aria-label="原生指令文件"
          placeholder="选择原生指令文件"
          ><a-option
            v-for="item in targets"
            :key="item.sourceId"
            :value="item.sourceId"
            >{{ item.label }} · {{ item.path }}</a-option
          ></a-select
        >
        <p v-if="source" class="instruction-path">{{ source.path }}</p>
        <p v-if="source">{{ source.effect.message }}</p>
        <p v-if="targetError" class="agent-workspace-error" role="alert">
          {{ targetError }}
        </p>
        <p v-else-if="!targetLoading && !targets.length">
          当前 Agent 没有可识别的指令文件。
        </p>
        <p v-else-if="source && !writable">
          {{
            source.actions.find(
              (action) =>
                action.action === "edit" || action.action === "create",
            )?.reason || "此文件当前不能写入。"
          }}
        </p>
        <footer>
          <a-button
            :disabled="!writable || targetLoading"
            @click="preview(true)"
            >移除共享区块…</a-button
          ><a-button
            type="primary"
            :disabled="!writable || targetLoading"
            @click="preview()"
            >预览配置…</a-button
          >
        </footer>
      </div>
    </a-modal>
    <AgentConfigurationEditorModal :model="editor" />
  </section>
</template>

<style scoped>
.agent-instructions-panel {
  padding: 16px;
  min-width: 0;
}
.agent-section-toolbar p,
.instruction-editor > p,
.instruction-target > p {
  color: var(--color-text-3);
  margin: 6px 0;
  font-size: 12px;
}
.instruction-actions,
.instruction-sections,
footer {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.instruction-scheme {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 16px;
  padding: 18px 0;
  border-bottom: 1px solid var(--color-border-2);
}
.instruction-scheme > div:first-child {
  min-width: 0;
  flex: 1 1 220px;
}
.instruction-scheme h3 {
  margin: 0 0 6px;
  font-size: 14px;
  color: var(--color-text-1);
}
.instruction-scheme p {
  color: var(--color-text-2);
  margin: 0 0 6px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.instruction-scheme small {
  color: var(--color-text-3);
}
.instruction-editor {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: 100%;
  min-height: 0;
  padding: 16px;
}
.instruction-editor :deep(.content-editor) {
  flex: 1;
  min-height: 240px;
}
.instruction-editor footer,
.instruction-target footer {
  justify-content: flex-end;
  margin-top: 8px;
}
.instruction-target {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 4px 0;
}
.instruction-path {
  overflow-wrap: anywhere;
}
</style>
