<script setup lang="ts">
import { onScopeDispose, reactive, ref, watch } from "vue";
import {
  getAgentSkillOrigin,
  saveAgentSkillOrigin,
  checkAgentSkillUpdate,
  planAgentSkillUpdate,
} from "../../api/agent-maintenance";
import { cancelAgentCatalogRead } from "../../api/agent-catalog";
import { useAgentMaintenanceStore } from "../../stores/agent-maintenance";
import { agentEnvironmentErrorMessage } from "../../stores/agent-environment";
import { withTimeout } from "../../utils/promise-timeout";
import type {
  AgentSkillOrigin,
  AgentSkillUpdateCheck,
} from "../../stores/agent-maintenance-types";
const props = defineProps<{
  assetId: string;
  version: number;
  workspace?: string;
}>();
const maintenance = useAgentMaintenanceStore();
const origin = ref<AgentSkillOrigin | null>(null);
const checked = ref<AgentSkillUpdateCheck | null>(null);
const draft = reactive({ repository: "", reference: "", directory: "" });
const opened = ref(false);
const editing = ref(false);
const busy = ref(false);
const error = ref("");
let revision = 0;
let loaded = false;
let readId = "";
function invalidate() {
  revision += 1;
  if (readId) void cancelAgentCatalogRead(readId).catch(() => {});
  readId = "";
  busy.value = false;
}
onScopeDispose(invalidate);
watch(
  () => [props.assetId, props.version, props.workspace],
  () => {
    invalidate();
    loaded = false;
    opened.value = false;
    origin.value = null;
    checked.value = null;
    editing.value = false;
    error.value = "";
  },
);
async function act(task: () => Promise<void>) {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  const request = revision;
  try {
    await task();
  } catch (failure) {
    if (request === revision)
      error.value = agentEnvironmentErrorMessage(failure);
  } finally {
    if (request === revision) {
      busy.value = false;
      if (readId) void cancelAgentCatalogRead(readId).catch(() => {});
      readId = "";
    }
  }
}
function edit() {
  Object.assign(
    draft,
    origin.value ?? { repository: "", reference: "", directory: "" },
  );
  editing.value = true;
}
function toggle(event: Event) {
  opened.value = (event.currentTarget as HTMLDetailsElement).open;
  if (!opened.value) {
    invalidate();
    return;
  }
  if (loaded) return;
  const request = revision;
  void act(async () => {
    const value = await withTimeout(
      getAgentSkillOrigin(props.assetId, props.workspace),
      15_000,
      "读取来源超时",
    );
    if (request !== revision) return;
    origin.value = value;
    loaded = true;
    if (!value) edit();
  });
}
function save() {
  const request = revision;
  void act(async () => {
    const value = await withTimeout(
      saveAgentSkillOrigin(props.assetId, props.version, {
        ...draft,
        revision: null,
      }),
      20_000,
      "保存来源超时",
    );
    if (request !== revision) return;
    origin.value = value;
    editing.value = false;
    checked.value = null;
  });
}
function check() {
  const request = revision;
  void act(async () => {
    // An inferred origin is read-only until the user asks to check this source.
    if (origin.value)
      await withTimeout(
        saveAgentSkillOrigin(props.assetId, props.version, origin.value),
        20_000,
        "保存来源超时",
      );
    if (request !== revision) return;
    readId = crypto.randomUUID();
    const value = await withTimeout(
      checkAgentSkillUpdate(props.assetId, readId),
      65_000,
      "检查更新超时",
    );
    if (request === revision) checked.value = value;
  });
}
function update() {
  const request = revision;
  void act(async () => {
    readId = crypto.randomUUID();
    await maintenance.presentWhenReady(
      planAgentSkillUpdate(props.assetId, readId),
      () => request === revision && opened.value,
      150_000,
      "读取上游版本超时",
    );
  });
}
</script>
<template>
  <details class="agent-catalog-advanced" :open="opened" @toggle="toggle">
    <summary>来源与更新</summary>
    <p v-if="error" class="agent-workspace-error" role="alert">{{ error }}</p>
    <a-form
      v-if="editing"
      :model="draft"
      layout="vertical"
      class="skill-source-form"
      ><a-form-item label="GitHub 仓库"
        ><a-input
          v-model="draft.repository"
          placeholder="https://github.com/作者/仓库" /></a-form-item
      ><a-form-item label="Skill 所在目录"
        ><a-input
          v-model="draft.directory"
          placeholder="例如 skills/skill-creator，仓库根目录则留空" /></a-form-item
      ><a-form-item label="分支或版本"
        ><a-input v-model="draft.reference" placeholder="留空使用默认分支"
      /></a-form-item>
      <div class="agent-inline-actions">
        <a-button
          type="primary"
          :loading="busy"
          :disabled="!draft.repository.trim()"
          @click="save"
          >保存来源</a-button
        ><a-button v-if="origin" @click="editing = false">取消编辑</a-button>
      </div></a-form
    >
    <template v-else-if="origin"
      ><div class="skill-source-location">
        <strong>{{ origin.repository }}</strong
        ><code
          >{{ origin.directory || "/" }} ·
          {{ origin.reference || "默认分支" }}</code
        >
      </div>
      <div class="agent-inline-actions">
        <a-button size="small" :loading="busy" @click="check">检查更新</a-button
        ><a-button v-if="checked" size="small" :disabled="busy" @click="update"
          >比较上游版本</a-button
        ><a-button type="text" size="small" @click="edit">编辑来源</a-button>
      </div>
      <p v-if="checked" role="status">{{ checked.message }}</p></template
    >
    <p v-else-if="busy" role="status">正在读取来源…</p>
  </details>
</template>
<style scoped>
.skill-source-form {
  max-width: 680px;
  padding: 16px 0 4px;
}
.skill-source-location {
  display: grid;
  gap: 6px;
  margin: 16px 0;
  color: var(--color-text-1);
  overflow-wrap: anywhere;
}
.skill-source-location code {
  color: var(--color-text-2);
  font-size: 12px;
}
</style>
