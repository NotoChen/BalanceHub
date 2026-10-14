<script setup lang="ts">
import { computed, reactive, ref, toRef, watch } from "vue";
import { Activity, CircleCheck, CircleAlert } from "@lucide/vue";
import { useMcpConnectionCheck } from "../../composables/useMcpConnectionCheck";
import AgentMcpTools from './AgentMcpTools.vue';
const props = defineProps<{ connection: string; assetId?: string; workspace?: string; bindings?: { id: string; label: string }[] }>();
const check = reactive(useMcpConnectionCheck(toRef(props, "connection")));
const TOOLS_PER_PAGE = 50;
const query = ref("");
const page = ref(1);
const toolsExpanded = ref(false);
const toolsList = ref<HTMLUListElement | null>(null);
const tools = computed(() => {
  const available = check.result?.tools ?? [];
  const needle = query.value.trim().toLocaleLowerCase();
  if (!needle) return available;
  return available.filter((tool) =>
    `${tool.name} ${tool.description}`.toLocaleLowerCase().includes(needle),
  );
});
const pageTools = computed(() =>
  tools.value.slice(
    (page.value - 1) * TOOLS_PER_PAGE,
    page.value * TOOLS_PER_PAGE,
  ),
);
function toggleTools(event: Event) {
  toolsExpanded.value = (event.currentTarget as HTMLDetailsElement).open;
}
watch(query, () => {
  page.value = 1;
});
watch(
  () => check.result?.id,
  () => {
    query.value = "";
    page.value = 1;
    toolsExpanded.value = false;
  },
);
watch(
  page,
  () => {
    if (toolsList.value) toolsList.value.scrollTop = 0;
  },
  { flush: "post" },
);
</script>
<template>
  <section class="mcp-connection-check" aria-label="MCP 连接检查">
    <header>
      <strong>连接状态</strong
      ><a-button v-if="!check.running" size="small" @click="check.start"
        ><template #icon><Activity :size="14" /></template
        >{{ check.result ? "重新检测" : "检测连接" }}</a-button
      ><a-button v-else size="small" @click="check.cancel">取消检查</a-button>
    </header>
    <p v-if="!check.result && !check.error" class="mcp-check-note">
      {{
        check.running
          ? "正在提交检查…"
          : "检测会连接远程服务或临时启动本地服务，完成后关闭。"
      }}
    </p>
    <p v-if="check.error" class="agent-workspace-error" role="alert">
      {{ check.error }}
    </p>
    <div
      v-if="check.result"
      class="mcp-check-result"
      :class="{
        'is-error':
          check.result.outcome &&
          !['passed', 'canceled'].includes(check.result.outcome),
      }"
      role="status"
    >
      <CircleCheck
        v-if="check.result.outcome === 'passed'"
        :size="16"
      /><CircleAlert
        v-else-if="check.result.outcome && check.result.outcome !== 'canceled'"
        :size="16"
      /><a-spin v-else-if="check.running" :size="14" /><span>{{
        check.result.message
      }}</span
      ><small>{{ (check.result.elapsedMs / 1000).toFixed(1) }} 秒</small>
    </div>
    <p v-if="check.result?.serverName" class="mcp-check-note">
      {{ check.result.serverName }} · MCP {{ check.result.protocolVersion }}
    </p>
    <details
      v-if="check.result?.tools.length"
      :open="toolsExpanded"
      @toggle="toggleTools"
    >
      <summary>查看工具 · {{ check.result.tools.length }}</summary>
      <AgentMcpTools v-if="toolsExpanded && assetId && bindings?.length" :asset-id="assetId" :workspace="workspace" :bindings="bindings" :tools="check.result.tools" />
      <template v-else-if="toolsExpanded">
        <a-input
          v-model="query"
          size="small"
          placeholder="筛选工具"
          aria-label="筛选 MCP 工具"
          allow-clear
        />
        <ul v-if="tools.length" ref="toolsList">
          <li v-for="tool in pageTools" :key="tool.name">
            <strong>{{ tool.name }}</strong>
            <p>{{ tool.description || "服务未提供说明" }}</p>
          </li>
        </ul>
        <p v-else class="mcp-check-note" role="status">没有匹配的工具</p>
        <div v-if="tools.length > TOOLS_PER_PAGE" class="mcp-tools-pagination">
          <span>共 {{ tools.length }} 个 · 每页 {{ TOOLS_PER_PAGE }} 个</span>
          <a-pagination
            v-model:current="page"
            :total="tools.length"
            :page-size="TOOLS_PER_PAGE"
            size="small"
            simple
          />
        </div>
      </template>
    </details>
  </section>
</template>
<style scoped>
.mcp-connection-check {
  border-top: 1px solid var(--color-border-2);
  padding-top: 14px;
  margin-top: 14px;
  color: var(--color-text-1);
}
header,
.mcp-check-result {
  display: flex;
  align-items: center;
  gap: 8px;
}
header {
  justify-content: space-between;
}
.mcp-check-result {
  margin-top: 10px;
  align-items: flex-start;
  color: var(--color-text-2);
  font-size: 12px;
}
.mcp-check-result > span {
  flex: 1;
  overflow-wrap: anywhere;
}
.mcp-check-result.is-error {
  color: rgb(var(--danger-6));
}
.mcp-check-result small {
  white-space: nowrap;
  color: var(--color-text-3);
}
.mcp-check-note {
  color: var(--color-text-3);
  font-size: 12px;
  margin: 8px 0;
}
details {
  margin-top: 12px;
}
summary {
  cursor: pointer;
  margin-bottom: 10px;
  font-size: 12px;
}
ul {
  padding: 0;
  list-style: none;
  max-height: 320px;
  overflow: auto;
}
li {
  padding: 8px 0;
  border-bottom: 1px solid var(--color-border-2);
  font-size: 12px;
  overflow-wrap: anywhere;
}
li p {
  color: var(--color-text-3);
  margin: 5px 0 0;
}
.mcp-tools-pagination {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 8px;
  color: var(--color-text-3);
  font-size: 12px;
}
</style>
