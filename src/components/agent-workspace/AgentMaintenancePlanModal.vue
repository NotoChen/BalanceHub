<script setup lang="ts">
import { computed, onScopeDispose, ref, shallowRef, watch } from "vue";
import { useAgentMaintenanceStore } from "../../stores/agent-maintenance";
import { getAgentMaintenanceChange } from "../../api/agent-maintenance";
import { agentEnvironmentErrorMessage } from "../../stores/agent-environment";
import { withTimeout } from "../../utils/promise-timeout";
import type { AgentAssetPlanChange } from "../../stores/provider-types";
import ContentDiff from "../ContentDiff.vue";
const maintenance = useAgentMaintenanceStore();
const index = ref(0);
const query = ref("");
const page = ref(1);
const current = shallowRef<AgentAssetPlanChange | null>(null);
const loading = ref(false);
const error = ref("");
let revision = 0;
watch(
  () => maintenance.preview?.token,
  () => {
    index.value = 0;
    query.value = "";
    page.value = 1;
  },
);
watch(query, () => {
  page.value = 1;
});
const documents = computed(
  () =>
    maintenance.preview?.changes
      .map((change, index) => ({ ...change, index }))
      .filter((change) =>
        `${change.label} ${change.path ?? ""}`
          .toLowerCase()
          .includes(query.value.toLowerCase()),
      ) ?? [],
);
const visibleDocuments = computed(() =>
  documents.value.slice((page.value - 1) * 50, page.value * 50),
);
async function load() {
  const token = maintenance.preview?.token;
  const request = ++revision;
  current.value = null;
  error.value = "";
  loading.value = false;
  if (!token || !maintenance.preview?.changes.length) return;
  loading.value = true;
  try {
    const change = await withTimeout(
      getAgentMaintenanceChange(token, index.value),
      20_000,
      "读取差异超时",
    );
    if (request === revision) current.value = change;
  } catch (failure) {
    if (request === revision)
      error.value = agentEnvironmentErrorMessage(failure);
  } finally {
    if (request === revision) loading.value = false;
  }
}
watch(() => [maintenance.preview?.token, index.value], load);
onScopeDispose(() => {
  revision += 1;
});
</script>
<template>
  <a-modal
    :visible="Boolean(maintenance.preview)"
    :title="maintenance.preview?.title"
    width="min(1320px, calc(100vw - 40px))"
    class="agent-maintenance-modal"
    closable
    mask-closable
    esc-to-close
    :footer="false"
    unmount-on-close
    @cancel="maintenance.closePreview"
  >
    <template v-if="maintenance.preview">
      <p class="maintenance-summary">
        {{ maintenance.preview.items.length }} 项操作 ·
        {{ maintenance.preview.changes.length }} 处内容比较
      </p>
      <div v-if="maintenance.preview.changes.length" class="maintenance-review">
        <aside aria-label="变更文件">
          <a-input
            v-model="query"
            allow-clear
            placeholder="查找文件"
            aria-label="查找变更文件"
          />
          <div class="maintenance-files">
            <button
              v-for="document in visibleDocuments"
              :key="document.index"
              type="button"
              :class="{ active: document.index === index }"
              :title="document.path ?? document.label"
              @click="index = document.index"
            >
              <strong>{{ document.label }}</strong
              ><span v-if="document.path">{{ document.path }}</span>
            </button>
          </div>
          <a-pagination
            v-if="documents.length > 50"
            v-model:current="page"
            :page-size="50"
            :total="documents.length"
            size="small"
            simple
          />
        </aside>
        <section class="maintenance-diff">
          <p v-if="loading" class="maintenance-summary" role="status">
            正在读取差异…
          </p>
          <div v-else-if="error" role="alert">
            <p class="agent-workspace-error">{{ error }}</p>
            <a-button size="small" @click="load">重新读取</a-button>
          </div>
          <template v-else-if="current"
            ><header>
              <strong>{{ current.label }}</strong
              ><span>当前内容 → 确认后的内容</span>
            </header>
            <ContentDiff
              :original-text="current.before ?? ''"
              :modified-text="current.after ?? ''"
          /></template>
        </section>
      </div>
      <ul v-else class="maintenance-items">
        <li v-for="item in maintenance.preview.items" :key="item.id">
          {{ item.label }}<span v-if="item.message"> · {{ item.message }}</span>
        </li>
      </ul>
      <div class="maintenance-notes">
        <p v-for="note in maintenance.preview.notes" :key="note">{{ note }}</p>
      </div>
      <footer class="maintenance-footer">
        <a-button @click="maintenance.closePreview">取消</a-button
        ><a-button
          type="primary"
          :disabled="loading || Boolean(error)"
          @click="maintenance.submit"
          >确认并开始</a-button
        >
      </footer>
    </template>
  </a-modal>
</template>
<style scoped>
.maintenance-summary {
  margin: 0 0 16px;
  color: var(--color-text-2);
  font-variant-numeric: tabular-nums;
}
.maintenance-review {
  display: grid;
  grid-template-columns: minmax(180px, 25%) minmax(0, 1fr);
  height: min(62vh, 680px);
  gap: 16px;
}
.maintenance-review aside {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.maintenance-files {
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.maintenance-files button {
  text-align: left;
  border: 1px solid transparent;
  background: transparent;
  color: var(--color-text-1);
  padding: 10px 12px;
  border-radius: 8px;
  cursor: pointer;
}
.maintenance-files button:hover {
  background: var(--color-fill-2);
}
.maintenance-files button.active {
  background: var(--color-fill-2);
  border-color: rgb(var(--primary-5));
}
.maintenance-files strong,
.maintenance-files span {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.maintenance-files span {
  margin-top: 4px;
  color: var(--color-text-3);
  font-size: 12px;
}
.maintenance-diff {
  min-width: 0;
  overflow: auto;
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
  background: var(--color-bg-2);
}
.maintenance-diff header {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  justify-content: space-between;
  padding: 12px 16px;
  border-bottom: 1px solid var(--color-border-2);
}
.maintenance-diff header span,
.maintenance-notes {
  font-size: 12px;
  color: var(--color-text-2);
}
.maintenance-notes {
  margin: 14px 0;
  line-height: 1.65;
}
.maintenance-notes p {
  margin: 4px 0;
}
.maintenance-items {
  max-height: 55vh;
  overflow: auto;
  line-height: 1.9;
}
.maintenance-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  padding-top: 12px;
}
@media (max-width: 760px) {
  .maintenance-review {
    grid-template-columns: 150px minmax(0, 1fr);
    gap: 8px;
  }
}
</style>
