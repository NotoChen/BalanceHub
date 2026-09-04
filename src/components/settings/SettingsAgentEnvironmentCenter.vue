<script setup lang="ts">
import { computed, onMounted } from "vue";
import { IconCommand, IconLoading, IconSearch } from "@arco-design/web-vue/es/icon";
import { useAgentEnvironmentCenter } from "../../composables/useAgentEnvironmentCenter";
import { useAgentHookConsole } from "../../composables/useAgentHookConsole";
import { agentHookTargetKey } from "../../utils/agent-runtime";
import { useWorkspaceStore } from "../../stores/workspaces";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentEnvironmentConsole from "./agent-environment/AgentEnvironmentConsole.vue";
import AgentHookPlanModal from "./agent-environment/AgentHookPlanModal.vue";
import AgentInstallationDetail from "./agent-environment/AgentInstallationDetail.vue";
import AgentWorkspaceScopeSelect from "./agent-environment/AgentWorkspaceScopeSelect.vue";
import type { AppSettings, CliToolProbeResult } from "../../stores/providers";

const props = defineProps<{ settings: AppSettings }>();

const {
  inventory,
  inventoryState,
  inventoryError,
  versionState,
  versionError,
  selectedInstallation,
  selectedWorkspacePath,
  activeDetailTab,
  assetCategory,
  assetQuery,
  selectedAssets,
  installationAssetCount,
  configurationAssets,
  selectedCategorySupported,
  configurationSupported,
  selectedConfigFileId,
  configPreview,
  configPreviewState,
  configPreviewError,
  copiedPathId,
  loadInventory,
  refreshVersions,
  openInstallation,
  closeInstallation,
  selectWorkspace,
  setDetailTab,
  selectConfig,
  openAsset,
  copyPath,
  deepScanState,
  deepScanCandidates,
  deepScanError,
  deepScanDraftChanged,
  startDeepScan,
  cancelDeepScan,
  canAdoptDeepScanCandidate,
  isDeepScanCandidateAdopted,
  adoptDeepScanCandidate,
} = useAgentEnvironmentCenter({ settings: props.settings });
const workspaceStore = useWorkspaceStore();
const agentKinds = computed(() => [...new Set((inventory.value?.installations ?? []).map((item) => item.agentKind))]);
const hookConsole = useAgentHookConsole(agentKinds);
const hookInspections = hookConsole.inspections;
const hookRowBusy = hookConsole.isRowBusy;
const hookRowError = hookConsole.rowError;
const hookPlanVisible = hookConsole.planVisible;
const hookPendingPlan = hookConsole.pendingPlan;
const hookCanApplyPlan = hookConsole.canApplyPlan;
const selectedHookInspection = computed(() => {
  const agentKind = selectedInstallation.value?.agentKind;
  return agentKind ? hookInspections.value[agentHookTargetKey(agentKind)] ?? null : null;
});
const workspaceOptions = computed(() => [
  { value: "", label: "全局与用户配置" },
  ...workspaceStore.workspaces.map((workspace) => ({
    value: workspace.path,
    label: workspace.path.split(/[\\/]/).filter(Boolean).slice(-1)[0] || workspace.path,
  })),
]);

function changeWorkspace(value: string) {
  selectWorkspace(value || undefined);
  void loadInventory();
}

function adoptCandidate(tool: CliToolProbeResult) {
  adoptDeepScanCandidate(tool);
}

async function loadAndInspect(forceRefresh = false) {
  const result = await loadInventory(forceRefresh);
  if (result) await hookConsole.inspectAll();
  return result;
}

function requestHookPlan(
  agentKind: Parameters<typeof hookConsole.requestPlan>[0],
  scope: Parameters<typeof hookConsole.requestPlan>[3],
  mutation: "install" | "enable" | "disable" | "remove",
) {
  void hookConsole.requestPlan(agentKind, mutation, false, scope);
}

function requestHookRepair(
  agentKind: Parameters<typeof hookConsole.requestPlan>[0],
  scope: Parameters<typeof hookConsole.requestPlan>[3],
) {
  void hookConsole.requestPlan(agentKind, "install", true, scope);
}

onMounted(() => {
  void loadAndInspect();
});
</script>

<template>
  <section class="settings-card settings-agent-environment-card">
    <header class="settings-card-header">
      <span class="settings-card-icon"><IconCommand /></span>
      <div><strong>Agent 环境</strong></div>
      <span class="settings-card-state">集中管理</span>
    </header>
    <div class="agent-environment-scope-row">
      <span>资产范围</span>
      <AgentWorkspaceScopeSelect
        :model-value="selectedWorkspacePath || ''"
        :options="workspaceOptions"
        @update:model-value="changeWorkspace"
      />
    </div>
    <details class="agent-environment-deep-scan">
      <summary class="agent-deep-scan-summary">
        <span><strong>GUI PATH 深度扫描</strong><small>仅在需要时读取登录 shell 的 Agent 路径</small></span>
        <IconSearch />
      </summary>
      <div class="agent-deep-scan-content">
      <div class="agent-deep-scan-header">
        <span class="agent-deep-scan-note">不会自动修改设置</span>
        <div class="agent-deep-scan-actions">
          <a-button
            size="small"
            :disabled="deepScanState === 'scanning'"
            @click="startDeepScan"
          >
            <template #icon>
              <IconLoading v-if="deepScanState === 'scanning'" class="agent-version-loading" />
              <IconSearch v-else />
            </template>
            {{ deepScanState === "scanning" ? "扫描中" : "深度扫描" }}
          </a-button>
          <a-button
            v-if="deepScanState === 'scanning'"
            size="small"
            type="text"
            @click="cancelDeepScan"
          >
            取消
          </a-button>
        </div>
      </div>
      <div v-if="deepScanState === 'scanning'" class="agent-deep-scan-status">
        <IconLoading class="agent-version-loading" /> 正在读取 GUI shell 环境，请稍候
      </div>
      <div v-else-if="deepScanState === 'error'" class="agent-deep-scan-status is-error">
        深度扫描失败：{{ deepScanError }}
      </div>
      <div v-else-if="deepScanState === 'ready'" class="agent-deep-scan-results">
        <p v-if="deepScanDraftChanged" class="agent-deep-scan-warning">
          设置中的 Agent 路径已发生变化，请重新扫描后再采用候选。
        </p>
        <div v-if="deepScanCandidates.length === 0" class="agent-deep-scan-status">
          未发现可用的 Agent CLI 路径
        </div>
        <div v-else class="agent-deep-scan-candidate-list">
          <div v-for="tool in deepScanCandidates" :key="tool.kind" class="agent-deep-scan-candidate">
            <AgentCliIcon :kind="tool.kind" :size="22" :label="tool.label" :decorative="false" />
            <div class="agent-deep-scan-candidate-copy">
              <strong>{{ tool.label }}</strong>
              <span :title="tool.path">{{ tool.path }}</span>
              <small>{{ tool.version.trim() || "版本未知" }}</small>
            </div>
            <a-button
              size="small"
              type="text"
              :disabled="!canAdoptDeepScanCandidate(tool)"
              @click="adoptCandidate(tool)"
            >
              {{ isDeepScanCandidateAdopted(tool) ? "已采用" : canAdoptDeepScanCandidate(tool) ? "采用路径" : "需重新扫描" }}
            </a-button>
          </div>
        </div>
      </div>
      </div>
    </details>
    <AgentInstallationDetail
      v-if="selectedInstallation"
      :installation="selectedInstallation"
      :hook-inspection="selectedHookInspection"
      :environment-name="inventory?.environment.displayName || '本机环境'"
      :assets="selectedAssets"
      :asset-count="installationAssetCount"
      :config-files="configurationAssets"
      :asset-category-supported="selectedCategorySupported"
      :configuration-supported="configurationSupported"
      :query="assetQuery"
      :category="assetCategory"
      :tab="activeDetailTab"
      :copied-path-id="copiedPathId"
      :selected-config-id="selectedConfigFileId"
      :preview="configPreview"
      :preview-state="configPreviewState"
      :preview-error="configPreviewError"
      :inventory-error="inventoryError"
      :version-error="versionError"
      :inventory-loading="inventoryState === 'loading' || inventoryState === 'refreshing'"
      :version-checking="versionState === 'loading' || versionState === 'refreshing'"
      @back="closeInstallation"
      @refresh="loadAndInspect(true)"
      @refresh-versions="refreshVersions"
      @update:query="assetQuery = $event"
      @update:category="assetCategory = $event"
      @update:tab="setDetailTab"
      @select-config="selectConfig"
      @open-asset="openAsset"
      @copy-path="copyPath"
    />
    <AgentEnvironmentConsole
      v-else
      :inventory="inventory"
      :loading="inventoryState === 'loading' || inventoryState === 'refreshing'"
      :version-checking="versionState === 'loading' || versionState === 'refreshing'"
      :error="inventoryError"
      :version-error="versionError"
      :inspections="hookInspections"
      :is-row-busy="hookRowBusy"
      :row-error="hookRowError"
      :inspect="(kind, scope) => hookConsole.inspect(kind, 'health', scope)"
      :verify="(kind, scope) => hookConsole.inspect(kind, 'verify', scope)"
      :repair="requestHookRepair"
      :mutate="requestHookPlan"
      @detail="openInstallation"
      @refresh="loadAndInspect(true)"
      @refresh-versions="refreshVersions"
    />
    <AgentHookPlanModal
      :visible="hookPlanVisible"
      :plan="hookPendingPlan"
      :can-apply="hookCanApplyPlan"
      @close="hookConsole.closePlan"
      @confirm="hookConsole.confirmPlan"
    />
  </section>
</template>
