<script setup lang="ts">
import { computed, onScopeDispose, ref, shallowRef, watch } from "vue";
import { RefreshCw, History } from "@lucide/vue";
import {
  listAgentHistory,
  planAgentHistoryRestore,
} from "../../api/agent-maintenance";
import { useAgentMaintenanceStore } from "../../stores/agent-maintenance";
import type { AgentHistoryEntry } from "../../stores/agent-maintenance-types";
import { withTimeout } from "../../utils/promise-timeout";
import { agentEnvironmentErrorMessage } from "../../stores/agent-environment";
const props = defineProps<{ visible: boolean }>();
const emit = defineEmits<{ close: [] }>();
const maintenance = useAgentMaintenanceStore();
const entries = shallowRef<AgentHistoryEntry[]>([]);
const query = ref("");
const loading = ref(false);
const error = ref("");
const more = ref(false);
const planning = ref("");
let revision = 0;
let planRevision = 0;
const expanded = ref("");
const selectedPaths = ref<string[]>([]);
const fileQuery = ref("");
const filePage = ref(1);
watch(
  [expanded, selectedPaths],
  () => {
    planRevision += 1;
    planning.value = "";
  },
  { deep: true, flush: "sync" },
);
const selectedEntry = computed(() =>
  entries.value.find((entry) => entry.id === expanded.value),
);
const matchingPaths = computed(
  () =>
    selectedEntry.value?.paths.filter((path) =>
      path.toLowerCase().includes(fileQuery.value.toLowerCase()),
    ) ?? [],
);
const visiblePaths = computed(() =>
  matchingPaths.value.slice((filePage.value - 1) * 50, filePage.value * 50),
);
watch(fileQuery, () => {
  filePage.value = 1;
});
const filtered = computed(() =>
  entries.value.filter((entry) =>
    `${entry.title} ${entry.paths.join(" ")}`
      .toLowerCase()
      .includes(query.value.toLowerCase()),
  ),
);
async function load(append = false) {
  const request = ++revision;
  loading.value = true;
  error.value = "";
  try {
    const result = await withTimeout(
      listAgentHistory(append ? entries.value.length : 0),
      20_000,
      "读取配置历史超时",
    );
    if (request !== revision || !props.visible) return;
    entries.value = append ? [...entries.value, ...result] : result;
    more.value = result.length === 30;
  } catch (failure) {
    if (request === revision)
      error.value = agentEnvironmentErrorMessage(failure);
  } finally {
    if (request === revision) loading.value = false;
  }
}
async function restore(entry: AgentHistoryEntry) {
  const request = ++planRevision;
  planning.value = entry.id;
  error.value = "";
  try {
    await maintenance.presentWhenReady(
      planAgentHistoryRestore(
        entry.id,
        expanded.value === entry.id ? [...selectedPaths.value] : undefined,
      ),
      () => request === planRevision && props.visible,
      60_000,
      "读取恢复内容超时",
    );
  } catch (failure) {
    if (request === planRevision)
      error.value = agentEnvironmentErrorMessage(failure);
  } finally {
    if (request === planRevision) planning.value = "";
  }
}
function chooseFiles(entry: AgentHistoryEntry) {
  expanded.value = expanded.value === entry.id ? "" : entry.id;
  selectedPaths.value = [...entry.paths];
  fileQuery.value = "";
  filePage.value = 1;
}
function invalidate() {
  revision += 1;
  planRevision += 1;
  planning.value = "";
  loading.value = false;
}
watch(
  () => props.visible,
  (visible) => {
    invalidate();
    if (visible) void load();
  },
  { immediate: true },
);
watch(
  () => maintenance.historyRevision,
  () => {
    if (props.visible) void load();
  },
);
onScopeDispose(invalidate);
const date = (value: string) =>
  value
    ? new Date(value).toLocaleString("zh-CN", { hour12: false })
    : "时间未知";
</script>
<template>
  <a-drawer
    :visible="visible"
    title="配置历史"
    width="min(920px, calc(100vw - 32px))"
    :footer="false"
    closable
    mask-closable
    esc-to-close
    @cancel="emit('close')"
  >
    <div class="agent-history">
      <header>
        <a-input
          v-model="query"
          allow-clear
          placeholder="搜索操作或文件路径"
          aria-label="搜索配置历史"
        /><a-button :loading="loading" @click="load()"
          ><template #icon><RefreshCw :size="16" /></template>刷新</a-button
        >
      </header>
      <p class="agent-workspace-note">
        保存通过本 App 修改前的原始内容，可比较差异后恢复。
      </p>
      <p v-if="error" class="agent-workspace-error" role="alert">{{ error }}</p>
      <div v-if="!entries.length && !loading" class="agent-workspace-empty">
        <History :size="28" /><strong>还没有配置变更记录</strong>
        <p>编辑、配置或移除资产后，原始内容会保存在这里。</p>
      </div>
      <p v-else-if="!filtered.length && !loading" class="agent-workspace-note">
        没有匹配的变更记录。
      </p>
      <article v-for="entry in filtered" :key="entry.id">
        <div>
          <strong>{{ entry.title }}</strong>
          <span
            >{{ date(entry.createdAt) }} · {{ entry.fileCount }} 个文件 ·
            {{ (entry.bytes / 1024).toFixed(1) }} KiB</span
          >
          <p v-if="entry.error" class="agent-workspace-error">
            {{ entry.error }}
          </p>
          <a-button
            v-else
            type="text"
            size="small"
            @click="chooseFiles(entry)"
            >{{
              expanded === entry.id ? "收起文件选择" : "查看与选择文件"
            }}</a-button
          >
          <div v-if="expanded === entry.id" class="history-file-selection">
            <a-input
              v-model="fileQuery"
              allow-clear
              placeholder="搜索文件路径"
              aria-label="搜索历史文件"
            />
            <div class="history-file-actions">
              <span
                >已选 {{ selectedPaths.length }} /
                {{ entry.paths.length }}</span
              >
              <a-button
                type="text"
                size="small"
                @click="selectedPaths = [...matchingPaths]"
                >选择搜索结果</a-button
              >
              <a-button type="text" size="small" @click="selectedPaths = []"
                >清空</a-button
              >
            </div>
            <a-checkbox-group
              v-model="selectedPaths"
              class="history-file-checkboxes"
            >
              <a-checkbox
                v-for="path in visiblePaths"
                :key="path"
                :value="path"
                >{{ path }}</a-checkbox
              >
            </a-checkbox-group>
            <a-pagination
              v-if="matchingPaths.length > 50"
              v-model:current="filePage"
              :page-size="50"
              :total="matchingPaths.length"
              size="small"
              simple
            />
          </div>
        </div>
        <a-button
          :loading="planning === entry.id"
          :disabled="
            Boolean(entry.error) ||
            Boolean(planning && planning !== entry.id) ||
            (expanded === entry.id && !selectedPaths.length)
          "
          @click="restore(entry)"
          >比较并恢复</a-button
        >
      </article>
      <a-button v-if="more" :loading="loading" long @click="load(true)"
        >加载更多</a-button
      >
    </div>
  </a-drawer>
</template>
<style scoped>
.agent-history {
  padding: 4px 8px 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.agent-history > header {
  display: flex;
  align-items: center;
  gap: 12px;
}
.agent-history article {
  display: flex;
  align-items: flex-start;
  gap: 20px;
  padding: 18px 0;
  border-bottom: 1px solid var(--color-border-2);
}
.agent-history article > div {
  min-width: 0;
  flex: 1;
}
.agent-history article strong {
  display: block;
  color: var(--color-text-1);
}
.agent-history article span {
  display: block;
  margin-top: 6px;
  color: var(--color-text-3);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}
.agent-history details {
  margin-top: 10px;
  color: var(--color-text-2);
  font-size: 12px;
}
.agent-history summary {
  cursor: pointer;
}
.agent-history li {
  overflow-wrap: anywhere;
  line-height: 1.7;
}
.history-file-selection {
  display: grid;
  gap: 12px;
  margin-top: 12px;
}
.history-file-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.history-file-actions span {
  margin: 0;
}
.history-file-checkboxes {
  display: grid;
  gap: 8px;
  max-height: 280px;
  overflow: auto;
}
.history-file-checkboxes :deep(.arco-checkbox-label) {
  overflow-wrap: anywhere;
  font-size: 12px;
}
</style>
