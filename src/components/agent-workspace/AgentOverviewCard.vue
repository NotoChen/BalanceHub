<script setup lang="ts">
import { computed, shallowRef } from "vue";
import { IconDown } from "@arco-design/web-vue/es/icon";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentWorkspaceIcon from "./AgentWorkspaceIcon.vue";
import AgentCardConfigurationSummary from "./AgentCardConfigurationSummary.vue";
import AgentCardActions from "./AgentCardActions.vue";
import AgentVersionStatus from "./AgentVersionStatus.vue";
import WorkspaceCard from "../workspace-card/WorkspaceCard.vue";
import WorkspaceCardHeader from "../workspace-card/WorkspaceCardHeader.vue";
import { agentDocumentationUrls } from "../../api/agent-documentation";
import { agentCliVisuals } from "../../agent-cli/visuals";
import { agentCliVersionLabel } from "../../utils/cli-environment";
import { iconThemeColor } from "../../utils/icon-theme-color";
import type { AgentCliDescriptor, AgentInstallation } from "../../stores/provider-types";
import type { AgentLifecycleTarget } from "../../stores/agent-lifecycle-types";
import type { AgentSessionCount } from "../../stores/agent-session-types";
import type { AgentConfigurationSnapshot } from "../../stores/agent-configuration-types";
import "../../styles/modules/agent-card.css";

const props = withDefaults(defineProps<{
  agent: AgentCliDescriptor;
  installations: AgentInstallation[];
  lifecycleTargets: AgentLifecycleTarget[];
  versionChecking?: boolean;
  versionCheckError?: string;
  counts: Record<"skill" | "mcp" | "extension", number | null>;
  inventoryReady: boolean;
  hookCount: number | null;
  historyCount?: AgentSessionCount | null;
  historyLoading?: boolean;
  historyError?: string;
  runningCount: number;
  canLaunch: boolean;
  loading: boolean;
  refreshing?: boolean;
  selectedPath: string | null;
  selectedVersion: string | null;
  configurationSnapshot?: AgentConfigurationSnapshot | null;
  configurationLoading?: boolean;
  configurationError?: string;
  configurationStale?: boolean;
}>(), { configurationSnapshot: null, configurationLoading: false, configurationError: "", configurationStale: true });
const emit = defineEmits<{
  assets: [category: "skill" | "mcp" | "extension"];
  hooks: [];
  installation: [];
  history: [];
  runtime: [];
  launch: [];
  openFile: [sourceId: string];
  refresh: [];
  documentation: [];
  installationGuide: [];
}>();
const iconTheme = shallowRef<{ source: string; color: string | null } | null>(null);
const cardAccent = computed(() => {
  const visual = agentCliVisuals[props.agent.kind];
  return (iconTheme.value?.source === visual.source ? iconTheme.value.color : null) ?? visual.orbitColor;
});
function readIconTheme(event: Event) {
  const icon = event.target;
  if (!(icon instanceof HTMLImageElement)) return;
  const source = agentCliVisuals[props.agent.kind].source;
  if ((icon.currentSrc || icon.src) !== new URL(source, document.baseURI).href) return;
  iconTheme.value = { source, color: iconThemeColor(icon) };
}
const installed = computed(() => props.installations.filter((installation) => installation.availability === "available"));
const currentTarget = computed(() => props.lifecycleTargets.find((target) => target.isCurrent));
const versionSummary = computed(() => {
  const currentVersion = currentTarget.value?.installation.installedVersion;
  if (currentVersion) return agentCliVersionLabel(currentVersion);
  if (props.selectedVersion) return agentCliVersionLabel(props.selectedVersion);
  if (!props.selectedPath && !props.canLaunch && installed.value.length) {
    return [...new Set(installed.value.map((installation) => installation.installedVersion
      ? agentCliVersionLabel(installation.installedVersion) : "版本未读取"))].join(" / ");
  }
  return props.selectedPath || props.canLaunch ? "版本未读取" : "";
});
const showInstallationGuide = computed(() => props.inventoryReady && !props.installations.length
  && !props.selectedPath && !props.selectedVersion && !props.canLaunch && !props.loading);
const installationStatus = computed(() => {
  if (versionSummary.value) return null;
  if (props.installations.length) return { label: "安装不可用", action: true, warning: true };
  if (props.loading) return { label: "检测中", action: false, warning: false };
  return { label: props.inventoryReady ? "未安装" : "状态未读取", action: true, warning: false };
});
const hookSummary = computed(() => props.hookCount === null ? props.loading ? "正在读取" : "数量未确认" : `已配置 ${props.hookCount} 条规则`);
const historyValue = computed(() => {
  const count = props.historyCount;
  if (props.historyError) return "读取失败";
  return count?.total != null ? count.total : props.historyLoading ? "统计中" : "—";
});
const historySummary = computed(() => {
  const count = props.historyCount;
  const summary = props.historyError ? `会话数量读取失败：${props.historyError}`
    : count?.total != null ? `共 ${count.total} 条历史会话` : "数量尚未读取";
  return `主目录本身及已记录目录，${summary}${props.historyLoading ? "，正在统计" : ""}`;
});
const runtimeSummary = computed(() => `已观测并确认的活跃会话，共 ${props.runningCount} 个；未确认状态不计入`);
const assetLinks = [
  { value: "skill", label: "Skill" },
  { value: "mcp", label: "MCP" },
  { value: "extension", label: "插件" },
] as const;
function assetSummary(count: number | null) {
  return count === null ? props.loading ? "正在读取" : "数量未确认" : `共 ${count} 项`;
}
type Statistic = {
  key: typeof assetLinks[number]["value"] | "hook" | "history" | "activeSessions";
  label: string;
  displayLabel?: string;
  value: number | string;
  summary: string;
  title?: string;
  loading?: boolean;
};
const statistics = computed<Statistic[]>(() => [
  ...assetLinks.map((link) => ({
    key: link.value,
    label: link.label,
    value: props.counts[link.value] ?? "—",
    summary: assetSummary(props.counts[link.value]),
    loading: props.counts[link.value] === null && props.loading,
  })),
  {
    key: "hook", label: "Hook", value: props.hookCount ?? "—",
    summary: hookSummary.value, loading: props.hookCount === null && props.loading,
  },
  {
    key: "history", label: "历史会话", displayLabel: "历史", value: historyValue.value,
    summary: historySummary.value, loading: props.historyLoading,
  },
  {
    key: "activeSessions", label: "活跃会话", displayLabel: "活跃", value: props.runningCount,
    summary: `共 ${props.runningCount} 个`, title: runtimeSummary.value,
  },
]);
function openStatistic(key: Statistic["key"]) {
  switch (key) {
    case "hook": emit("hooks"); break;
    case "history": emit("history"); break;
    case "activeSessions": emit("runtime"); break;
    default: emit("assets", key);
  }
}
</script>

<template>
  <WorkspaceCard class="agent-overview-card" :fixed-height="false" :data-agent-kind="agent.kind" :aria-label="agent.label" :style="{ '--workspace-card-state': cardAccent }">
    <template #header>
      <WorkspaceCardHeader :title="agent.label" heading-tag="h2" :framed-icon="false">
        <template #title><a class="agent-overview-name" :href="agentDocumentationUrls[agent.kind]" title="打开官方文档" :aria-label="`打开 ${agent.label} 官方文档`" @click.prevent="emit('documentation')">{{ agent.label }}</a></template>
        <template #icon>
          <AgentCliIcon :kind="agent.kind" :label="agent.label" :decorative="false" @load="readIconTheme" />
        </template>
        <template #subtitle>
          <button v-if="versionSummary" type="button" class="agent-overview-version" title="版本与路径：查看本机安装、检查更新" :aria-label="`${agent.label} 版本与路径，${versionSummary}`" @click="emit('installation')"><span>{{ versionSummary }}</span><IconDown :size="10" aria-hidden="true" /></button>
          <button v-if="installationStatus?.action" type="button" class="agent-overview-status" :class="{ 'is-warning': installationStatus.warning }" :aria-label="`${agent.label} 安装状态，${installationStatus.label}`" :title="showInstallationGuide ? '打开官方安装说明' : '查看版本与路径'" @click="showInstallationGuide ? emit('installationGuide') : emit('installation')">{{ installationStatus.label }}</button>
          <span v-else-if="installationStatus" class="agent-overview-status" role="status">{{ installationStatus.label }}</span>
        </template>
        <template v-if="versionSummary" #meta>
          <button type="button" class="agent-overview-status" :aria-label="`查看 ${agent.label} 当前使用安装的版本状态`" @click="emit('installation')"><AgentVersionStatus :version="currentTarget?.version ?? null" :checking="versionChecking" :error="versionCheckError" compact /></button>
        </template>
      </WorkspaceCardHeader>
    </template>
    <nav class="agent-card-statistics" :aria-label="`${agent.label} 资源与会话`">
      <button v-for="statistic in statistics" :key="statistic.key" type="button" class="agent-card-statistic"
        :class="{ 'is-running': statistic.key === 'activeSessions' && runningCount > 0, 'is-error': statistic.key === 'history' && historyError }"
        :title="statistic.title ?? `${statistic.label} · ${statistic.summary}`"
        :aria-label="`查看 ${agent.label} 的 ${statistic.label}，${statistic.summary}`"
        :aria-busy="statistic.loading || undefined" @click="openStatistic(statistic.key)">
        <span class="agent-card-statistic-label"><AgentWorkspaceIcon :page="statistic.key" :size="12" /><span>{{ statistic.displayLabel ?? statistic.label }}</span></span>
        <strong :class="{ 'is-unknown': typeof statistic.value !== 'number', 'is-text': typeof statistic.value === 'string' && statistic.value !== '—' }">{{ statistic.value }}</strong>
      </button>
    </nav>
    <AgentCardConfigurationSummary :label="agent.label" :snapshot="configurationSnapshot" :loading="configurationLoading" :error="configurationError" :stale="configurationStale"
      @open-file="emit('openFile', $event)" @refresh="emit('refresh')" />
    <AgentCardActions :label="agent.label" :can-launch="canLaunch" :show-installation-guide="showInstallationGuide" :refreshing="refreshing || configurationLoading"
      @installation-guide="emit('installationGuide')" @launch="emit('launch')"
      @refresh="emit('refresh')" />
  </WorkspaceCard>
</template>
