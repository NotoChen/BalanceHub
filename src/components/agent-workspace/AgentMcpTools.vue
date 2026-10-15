<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import {
  getAgentMcpToolRules,
  planAgentMcpTools,
} from "../../api/agent-maintenance";
import { cancelAgentCatalogRead } from "../../api/agent-catalog";
import { useAgentMaintenanceStore } from "../../stores/agent-maintenance";
import { agentEnvironmentErrorMessage } from "../../stores/agent-environment";
import type { AgentMcpToolRules } from "../../stores/agent-maintenance-types";
import { withTimeout } from "../../utils/promise-timeout";

const props = defineProps<{
  assetId: string;
  workspace?: string;
  bindings: { id: string; label: string }[];
  tools: { name: string; description: string }[];
}>();
const maintenance = useAgentMaintenanceStore();
const bindingId = ref("");
const model = ref<AgentMcpToolRules | null>(null);
const disabled = ref<string[]>([]);
const loading = ref(false);
const error = ref("");
const query = ref("");
const page = ref(1);
let revision = 0;
let readId = "";
function invalidate() {
  revision += 1;
  if (readId) void cancelAgentCatalogRead(readId).catch(() => {});
  readId = "";
  loading.value = false;
}
function request() {
  return {
    assetId: props.assetId,
    bindingId: bindingId.value,
    workspace: props.workspace ?? null,
    tools: props.tools.map((tool) => tool.name),
  };
}
async function load() {
  invalidate();
  model.value = null;
  disabled.value = [];
  error.value = "";
  if (!bindingId.value || !props.tools.length) return;
  const current = revision;
  readId = crypto.randomUUID();
  loading.value = true;
  try {
    const result = await withTimeout(
      getAgentMcpToolRules(request(), readId),
      15_000,
      "读取工具规则超时",
    );
    if (current !== revision) return;
    model.value = result;
    disabled.value = result.rules
      .filter((rule) => rule.disabledHere)
      .map((rule) => rule.name);
  } catch (failure) {
    if (current === revision)
      error.value = agentEnvironmentErrorMessage(failure);
  } finally {
    if (current === revision) {
      loading.value = false;
      if (readId) void cancelAgentCatalogRead(readId).catch(() => {});
      readId = "";
    }
  }
}
watch(
  () => props.bindings.map((binding) => binding.id).join("\0"),
  () => {
    if (!props.bindings.some((binding) => binding.id === bindingId.value))
      bindingId.value = props.bindings[0]?.id ?? "";
  },
  { immediate: true },
);
watch(
  [
    () => props.assetId,
    () => props.workspace,
    bindingId,
    () => props.tools.map((tool) => tool.name).join("\0"),
    () => maintenance.historyRevision,
  ],
  () => {
    page.value = 1;
    void load();
  },
  { immediate: true },
);
onScopeDispose(invalidate);
watch(query, () => {
  page.value = 1;
});
const states = computed(
  () => new Map(model.value?.rules.map((rule) => [rule.name, rule]) ?? []),
);
const filtered = computed(() =>
  props.tools.filter((tool) =>
    `${tool.name} ${tool.description}`
      .toLocaleLowerCase()
      .includes(query.value.trim().toLocaleLowerCase()),
  ),
);
const visibleTools = computed(() =>
  filtered.value.slice((page.value - 1) * 50, page.value * 50),
);
const dirty = computed(() =>
  Boolean(
    model.value &&
    model.value.rules.some(
      (rule) => rule.disabledHere !== disabled.value.includes(rule.name),
    ),
  ),
);
function toggle(name: string, enabled: boolean) {
  disabled.value = enabled
    ? disabled.value.filter((value) => value !== name)
    : [...new Set([...disabled.value, name])];
}
async function preview() {
  if (!dirty.value || loading.value) return;
  invalidate();
  const current = revision;
  readId = crypto.randomUUID();
  loading.value = true;
  error.value = "";
  try {
    await maintenance.presentWhenReady(
      planAgentMcpTools(request(), disabled.value, readId),
      () => current === revision,
      15_000,
      "预览工具规则超时",
    );
  } catch (failure) {
    if (current === revision)
      error.value = agentEnvironmentErrorMessage(failure);
  } finally {
    if (current === revision) {
      loading.value = false;
      if (readId) void cancelAgentCatalogRead(readId).catch(() => {});
      readId = "";
    }
  }
}
</script>
<template>
  <section class="mcp-tool-manager" aria-label="工具管理">
    <header>
      <a-select
        v-if="bindings.length > 1"
        v-model="bindingId"
        :options="
          bindings.map((binding) => ({
            value: binding.id,
            label: binding.label,
          }))
        "
        aria-label="要配置的 Agent 来源"
      /><strong v-else>{{ bindings[0]?.label }}</strong
      ><a-button
        v-if="dirty"
        size="small"
        type="primary"
        :loading="loading"
        @click="preview"
        >预览更改</a-button
      >
    </header>
    <p v-if="error" class="agent-workspace-error" role="alert">
      {{ error }}
      <a-button size="mini" type="text" @click="load">重新读取</a-button>
    </p>
    <p v-else-if="loading" class="agent-workspace-note" role="status">
      正在读取工具规则…
    </p>
    <a-input
      v-model="query"
      size="small"
      allow-clear
      placeholder="筛选工具"
      aria-label="筛选工具"
    />
    <ul>
      <li v-for="tool in visibleTools" :key="tool.name">
        <div>
          <strong>{{ tool.name }}</strong>
          <p>{{ tool.description || "服务未提供说明" }}</p>
          <small v-if="states.get(tool.name)?.restrictedBy.length"
            >原配置限制：{{
              states.get(tool.name)?.restrictedBy.join("、")
            }}</small
          ><small v-else-if="disabled.includes(tool.name)">{{
            states.get(tool.name)?.disabledHere
              ? "由 BalanceHub 停用"
              : "待停用，尚未保存"
          }}</small>
        </div>
        <a-switch
          v-if="model"
          :model-value="
            !disabled.includes(tool.name) &&
            !states.get(tool.name)?.restrictedBy.length
          "
          :disabled="
            loading ||
            (Boolean(states.get(tool.name)?.restrictedBy.length) &&
              !disabled.includes(tool.name))
          "
          :aria-label="`${tool.name} 在此 Agent 的可用状态`"
          :title="
            states.get(tool.name)?.restrictedBy.length
              ? '原配置限制仍然生效；这里只撤回 BalanceHub 添加的停用项'
              : undefined
          "
          @change="(value) => toggle(tool.name, Boolean(value))"
        /><a-button
          v-if="
            states.get(tool.name)?.restrictedBy.length &&
            disabled.includes(tool.name)
          "
          size="mini"
          type="text"
          :disabled="loading"
          @click="toggle(tool.name, true)"
          >撤回 App 停用项</a-button
        >
      </li>
    </ul>
    <p v-if="!filtered.length" class="agent-workspace-note">没有匹配的工具</p>
    <footer>
      <span>{{ filtered.length }} 个工具</span
      ><a-pagination
        v-if="filtered.length > 50"
        v-model:current="page"
        :total="filtered.length"
        :page-size="50"
        simple
        size="small"
      />
    </footer>
    <p v-for="note in model?.notes" :key="note" class="agent-workspace-note">
      {{ note }}
    </p>
  </section>
</template>
<style scoped>
.mcp-tool-manager {
  display: grid;
  gap: 12px;
  color: var(--color-text-1);
}
header,
footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
header :deep(.arco-select) {
  max-width: 440px;
}
ul {
  list-style: none;
  padding: 0;
  margin: 0;
  max-height: 380px;
  overflow: auto;
}
li {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 12px 2px;
  border-bottom: 1px solid var(--color-border-2);
}
li > div {
  min-width: 0;
  flex: 1;
  overflow-wrap: anywhere;
}
li strong {
  font-size: 13px;
}
li p {
  margin: 5px 0 0;
  color: var(--color-text-2);
  font-size: 12px;
  line-height: 1.6;
}
li small,
footer {
  color: var(--color-text-3);
  font-size: 12px;
}
li small {
  display: block;
  margin-top: 5px;
}
</style>
