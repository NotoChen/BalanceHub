<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { RefreshCw } from "@lucide/vue";
import type {
  AgentCliDescriptor,
  AgentCliKind,
  AgentRuntimeScope,
} from "../../stores/provider-types";
import type { WslDistribution } from "../../api/agent-wsl";
import { useAgentWslStore } from "../../stores/agent-wsl";
import { useAgentWorkspaceStore } from "../../stores/agent-workspace";
import { useAgentConfigurationSourcesStore } from "../../stores/agent-configuration-sources";
import { useAgentConfiguration } from "../../composables/useAgentConfiguration";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentConfigurationEditorModal from "./AgentConfigurationEditorModal.vue";
import AgentConfigurationPreviewModal from "./AgentConfigurationPreviewModal.vue";
import AgentConfigurationFileActions from "./AgentConfigurationFileActions.vue";
import AgentAssetAccessModal from "../settings/agent-environment/AgentAssetAccessModal.vue";
const props = defineProps<{
  query: string;
  agents: AgentCliDescriptor[];
  active: boolean;
}>();
const wsl = useAgentWslStore();
const navigation = useAgentWorkspaceStore();
const publications = useAgentConfigurationSourcesStore();
const selected = ref<WslDistribution | null>(null);
const agentKind = ref<AgentCliKind | null>(null);
const workspace = ref<string | undefined>();
const runtimeScope = computed<AgentRuntimeScope>(() => ({
  kind: "wsl",
  distro_id: selected.value?.id ?? "",
}));
const model = reactive(
  useAgentConfiguration({ agentKind, workspace, runtimeScope }),
);
const entry = computed(() =>
  agentKind.value
    ? publications.forRuntime(runtimeScope.value).get(agentKind.value)
    : null,
);
const rows = computed(
  () =>
    wsl.inventory?.distributions.filter((item) =>
      item.name
        .toLocaleLowerCase()
        .includes(props.query.trim().toLocaleLowerCase()),
    ) ?? [],
);
function configure(distro: WslDistribution, kind: AgentCliKind) {
  selected.value = distro;
  agentKind.value = kind;
}
function close() {
  selected.value = null;
  agentKind.value = null;
}
function history(distro: WslDistribution, kind: AgentCliKind) {
  navigation.sessionEnvironment = { kind: "wsl", distro_id: distro.id };
  navigation.openSessions("history", kind);
}
function refreshSources() {
  if (agentKind.value)
    void publications.forRuntime(runtimeScope.value).refresh(agentKind.value);
}
watch(
  () => props.active,
  (active) => {
    if (active) void wsl.ensure();
  },
  { immediate: true },
);
defineExpose({ refresh: () => wsl.refresh() });
</script>

<template>
  <section class="wsl-panel" aria-label="WSL 环境">
    <header class="agent-section-toolbar">
      <div>
        <strong>WSL 环境</strong>
        <p>各发行版使用自己的 Agent、配置与会话记录。</p>
      </div>
      <a-button size="small" :loading="wsl.loading" @click="wsl.refresh()"
        ><template #icon><RefreshCw :size="14" /></template>检测环境</a-button
      >
    </header>
    <p v-if="wsl.error" class="agent-workspace-error" role="alert">
      {{ wsl.error }}
    </p>
    <div v-if="!rows.length" class="agent-workspace-empty">
      {{
        wsl.loading
          ? "正在检测 WSL…"
          : query
            ? "没有匹配的发行版"
            : "尚未检测到 WSL 发行版。安装后点击检测环境。"
      }}
    </div>
    <article v-for="distro in rows" :key="distro.id" class="wsl-distro">
      <header>
        <h3>{{ distro.name }}</h3>
        <span :class="{ 'is-running': distro.running }">{{
          distro.running ? "运行中" : "已停止"
        }}</span
        ><a-button
          v-if="!distro.running"
          size="small"
          :loading="wsl.starting === distro.id"
          :disabled="wsl.loading && wsl.starting !== distro.id"
          @click="wsl.start(distro.id)"
          >启动并检测</a-button
        >
      </header>
      <p v-if="distro.error" class="agent-workspace-error" role="alert">
        {{ distro.error }}
      </p>
      <p v-else-if="!distro.running" class="wsl-hint">
        {{
          distro.agents.length
            ? "保留上次检测结果。启动后可读取配置和会话。"
            : "按需启动后检测 Agent。"
        }}
      </p>
      <div
        v-for="agent in distro.agents"
        :key="agent.agentKind"
        class="wsl-agent"
      >
        <AgentCliIcon :kind="agent.agentKind" :size="22" />
        <div class="wsl-agent-info">
          <strong>{{
            agents.find((item) => item.kind === agent.agentKind)?.label ||
            agent.agentKind
          }}</strong
          ><span>{{
            agent.executable
              ? agent.version || "已安装 · 版本未读取"
              : "未检测到 CLI"
          }}</span
          ><code>{{ agent.configRoot }}</code>
        </div>
        <div class="wsl-actions">
          <a-button
            size="small"
            :disabled="!distro.running || Boolean(distro.error)"
            @click="configure(distro, agent.agentKind)"
            >配置文件</a-button
          ><a-button
            size="small"
            :disabled="!distro.running || Boolean(distro.error)"
            @click="history(distro, agent.agentKind)"
            >历史会话</a-button
          >
        </div>
      </div>
    </article>
    <a-drawer
      :visible="Boolean(selected)"
      :title="`${selected?.name ?? ''} · 配置文件`"
      width="min(760px, calc(100vw - 32px))"
      :footer="false"
      closable
      mask-closable
      esc-to-close
      @cancel="close"
    >
      <div class="wsl-sources">
        <a-button size="small" :loading="entry?.loading" @click="refreshSources"
          >刷新文件</a-button
        >
        <p v-if="entry?.error" class="agent-workspace-error" role="alert">
          {{ entry.error }}
        </p>
        <p v-if="!entry?.value && entry?.loading">正在读取原生配置…</p>
        <article
          v-for="source in entry?.value?.sources ?? []"
          :key="source.sourceId"
        >
          <div>
            <strong>{{ source.label }}</strong
            ><code>{{ source.path }}</code>
            <p v-if="source.diagnostics.length">
              {{ source.diagnostics.map((item) => item.message).join("；") }}
            </p>
          </div>
          <a-button
            size="small"
            :disabled="source.access.kind !== 'ready'"
            @click="model.openFile(source.sourceId, source)"
            >{{ source.revision.isMissing ? "新建" : "打开" }}</a-button
          >
        </article>
      </div>
    </a-drawer>
    <AgentConfigurationEditorModal :model="model.editor" :sources="model.sources" @open-source="model.openFile"
      ><template #file-actions="{ sourceId }"
        ><AgentConfigurationFileActions
          :source="model.editorSource(sourceId)"
          :busy="model.opening[sourceId]"
          @action="
            (source, action) => model.action(source.sourceId, action, source)
          " /></template
    ></AgentConfigurationEditorModal>
    <AgentConfigurationPreviewModal :model="model" />
    <AgentAssetAccessModal
      :confirmation="model.accessConfirmation"
      @close="model.closeAccessConfirmation"
      @confirm="model.confirmAccess"
    />
  </section>
</template>

<style scoped>
.wsl-panel {
  padding: 16px;
  min-width: 0;
}
.agent-section-toolbar p,
.wsl-hint {
  margin: 6px 0;
  color: var(--color-text-3);
  font-size: 12px;
}
.wsl-distro {
  padding: 20px 0;
  border-bottom: 1px solid var(--color-border-2);
}
.wsl-distro > header {
  display: flex;
  align-items: center;
  gap: 12px;
}
.wsl-distro h3 {
  margin: 0;
  color: var(--color-text-1);
  font-size: 15px;
}
.wsl-distro > header > span {
  font-size: 12px;
  color: var(--color-text-3);
}
.wsl-distro > header > span.is-running {
  color: rgb(var(--success-6));
}
.wsl-agent {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
  padding: 14px 0;
}
.wsl-agent-info {
  display: flex;
  flex-direction: column;
  gap: 5px;
  min-width: 0;
  flex: 1 1 240px;
}
.wsl-agent-info strong {
  color: var(--color-text-1);
  font-size: 13px;
}
.wsl-agent-info span {
  color: var(--color-text-2);
  font-size: 12px;
}
code {
  color: var(--color-text-3);
  font-size: 12px;
  overflow-wrap: anywhere;
}
.wsl-actions {
  display: flex;
  gap: 8px;
}
.wsl-sources {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 16px;
}
.wsl-sources > article {
  display: flex;
  align-items: center;
  gap: 16px;
  border-bottom: 1px solid var(--color-border-2);
  padding-bottom: 16px;
}
.wsl-sources > article > div {
  flex: 1;
  min-width: 0;
}
.wsl-sources code {
  display: block;
  margin: 6px 0;
}
.wsl-sources p {
  font-size: 12px;
  color: var(--color-text-3);
  margin: 0;
}
</style>
