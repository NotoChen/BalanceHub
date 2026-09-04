<script setup lang="ts">
import { IconArrowLeft, IconLoading, IconRefresh } from "@arco-design/web-vue/es/icon";
import AgentCliIcon from "../../AgentCliIcon.vue";
import AgentAssetInventory from "./AgentAssetInventory.vue";
import AgentConfigBrowser from "./AgentConfigBrowser.vue";
import AgentVersionStatus from "./AgentVersionStatus.vue";
import type {
  AgentAssetReadResult,
  AgentAssetRecord,
  AgentHookHealthState,
  AgentHookInspection,
  AgentHookTrust,
  AgentInstallation,
} from "../../../stores/provider-types";
import type { AgentEnvironmentDetailTab } from "../../../composables/useAgentEnvironmentCenter";

defineProps<{
  installation: AgentInstallation;
  hookInspection: AgentHookInspection | null;
  environmentName: string;
  assets: AgentAssetRecord[];
  assetCount: number;
  configFiles: AgentAssetRecord[];
  assetCategorySupported: boolean;
  configurationSupported: boolean;
  query: string;
  category: AgentAssetRecord["category"] | "all";
  tab: AgentEnvironmentDetailTab;
  copiedPathId: string | null;
  selectedConfigId: string | null;
  preview: AgentAssetReadResult | null;
  previewState: "idle" | "loading" | "refreshing" | "ready" | "error";
  previewError: string;
  inventoryError: string | null;
  versionError: string | null;
  inventoryLoading: boolean;
  versionChecking: boolean;
}>();
defineEmits<{
  back: [];
  refresh: [];
  "refresh-versions": [];
  "update:query": [value: string];
  "update:category": [value: AgentAssetRecord["category"] | "all"];
  "update:tab": [value: AgentEnvironmentDetailTab];
  selectConfig: [asset: AgentAssetRecord];
  openAsset: [asset: AgentAssetRecord];
  copyPath: [asset: AgentAssetRecord];
}>();

const channelLabels: Record<AgentInstallation["channel"], string> = {
  stable: "稳定",
  preview: "预览",
  nightly: "Nightly",
  unknown: "未知",
};
const versionSourceLabels: Record<AgentInstallation["installedVersionSource"], string> = {
  npmRegistry: "npm",
  localExecutable: "本地可执行文件",
  unknown: "未知",
};
const hookStateLabels: Record<AgentHookHealthState, string> = {
  not_installed: "未安装",
  installed_untrusted: "待信任",
  installed_unverified: "待验证",
  healthy: "运行正常",
  disabled: "已停用",
  conflict: "配置冲突",
  helper_missing: "辅助程序缺失",
  spool_blocked: "事件存储不可用",
  unsupported: "暂不支持",
};
const hookTrustLabels: Record<AgentHookTrust, string> = {
  unknown: "未知",
  trusted: "已信任",
  required: "需要信任",
  not_applicable: "不适用",
};

function formatHookTime(timestamp: number | null) {
  return timestamp === null ? "尚无事件" : new Date(timestamp).toLocaleString();
}
</script>

<template>
  <div class="agent-environment-detail">
    <header class="agent-detail-header">
      <a-button type="text" size="small" aria-label="返回 Agent 列表" @click="$emit('back')">
        <template #icon><IconArrowLeft /></template>返回
      </a-button>
      <span class="agent-detail-identity"><AgentCliIcon :kind="installation.agentKind" :size="24" /><strong>{{ installation.label }}</strong></span>
      <span class="agent-detail-environment">{{ environmentName }} · {{ installation.executablePath || "未提供可执行路径" }}</span>
      <div class="agent-detail-actions">
        <a-button type="text" size="small" :disabled="versionChecking" @click="$emit('refresh-versions')">
          <template #icon><IconLoading v-if="versionChecking" class="agent-version-loading" /><IconRefresh v-else /></template>检查版本
        </a-button>
        <a-button type="text" size="small" :loading="inventoryLoading" @click="$emit('refresh')">
          <template #icon><IconRefresh /></template>刷新
        </a-button>
      </div>
    </header>
    <div class="agent-detail-summary">
      <AgentVersionStatus :installation="installation" :checking="versionChecking" />
      <span v-if="installation.diagnostic" class="agent-detail-diagnostic">{{ installation.diagnostic }}</span>
    </div>
    <p v-if="inventoryError" class="agent-environment-stale-error">刷新失败，保留上次成功结果：{{ inventoryError }}</p>
    <p v-if="versionError" class="agent-environment-stale-error">版本检查失败，保留上次成功结果：{{ versionError }}</p>
    <div class="agent-detail-tabs" role="tablist" aria-label="Agent 详情">
      <button type="button" :class="{ active: tab === 'overview' }" @click="$emit('update:tab', 'overview')">概览</button>
      <button type="button" :class="{ active: tab === 'assets' }" @click="$emit('update:tab', 'assets')">资产 <small>{{ assetCount }}</small></button>
      <button type="button" :class="{ active: tab === 'configuration' }" @click="$emit('update:tab', 'configuration')">配置 <small>{{ configFiles.length }}</small></button>
    </div>
    <div v-if="tab === 'overview'" class="agent-detail-overview-panel">
      <div class="agent-detail-overview">
        <div class="agent-fact"><span>安装来源</span><strong>{{ installation.discoverySource === 'automatic' ? '自动发现' : '配置路径' }}</strong></div>
        <div class="agent-fact"><span>版本通道</span><strong>{{ channelLabels[installation.channel] }}</strong></div>
        <div class="agent-fact"><span>已安装版本来源</span><strong>{{ versionSourceLabels[installation.installedVersionSource] }}</strong></div>
        <div class="agent-fact"><span>最新版本来源</span><strong>{{ versionSourceLabels[installation.latestVersionSource] }}</strong></div>
        <div class="agent-fact"><span>可执行文件</span><strong :title="installation.executablePath || ''">{{ installation.executablePath || "未找到" }}</strong></div>
        <div class="agent-fact"><span>已盘点资产</span><strong>{{ assetCount }} 项</strong></div>
      </div>
      <section class="agent-hook-diagnostics" aria-label="会话 Hook 诊断">
        <header>
          <strong>会话 Hook 诊断</strong>
          <span v-if="hookInspection">{{ hookStateLabels[hookInspection.state] }}</span>
          <span v-else>尚未读取</span>
        </header>
        <template v-if="hookInspection">
          <div class="agent-hook-diagnostic-facts">
            <span>安装 <strong>{{ hookInspection.installed ? "是" : "否" }}</strong></span>
            <span>启用 <strong>{{ hookInspection.enabled ? "是" : "否" }}</strong></span>
            <span>信任 <strong>{{ hookTrustLabels[hookInspection.trusted] }}</strong></span>
            <span>辅助程序 <strong>{{ hookInspection.helperAvailable ? "可用" : "缺失" }}</strong></span>
            <span>事件存储 <strong>{{ hookInspection.spoolAvailable ? "可用" : "不可用" }}</strong></span>
            <span>最近事件 <strong>{{ formatHookTime(hookInspection.lastEventAt) }}</strong></span>
          </div>
          <div class="agent-hook-diagnostic-path">
            <span>配置文件</span>
            <code :title="hookInspection.configPath">{{ hookInspection.configPath }}</code>
            <small :title="hookInspection.revision">revision {{ hookInspection.revision }}</small>
          </div>
          <ul v-if="hookInspection.diagnostics.length" class="agent-hook-diagnostic-list">
            <li v-for="diagnostic in hookInspection.diagnostics" :key="diagnostic">{{ diagnostic }}</li>
          </ul>
          <details v-if="hookInspection.ownership" class="agent-hook-ownership">
            <summary>BalanceHub ownership · {{ hookInspection.ownership.resources.length }} 项</summary>
            <div v-for="resource in hookInspection.ownership.resources" :key="resource.structuralIdentity">
              <strong>{{ resource.eventName }}</strong>
              <code :title="resource.structuralIdentity">{{ resource.structuralIdentity }}</code>
              <small :title="resource.contentFingerprint">{{ resource.contentFingerprint }}</small>
            </div>
          </details>
        </template>
        <p v-else>返回列表刷新后可查看完整 Hook 状态。</p>
      </section>
    </div>
    <AgentAssetInventory
      v-else-if="tab === 'assets'"
      :assets="assets"
      :query="query"
      :category="category"
      :category-supported="assetCategorySupported"
      :copied-path-id="copiedPathId"
      @update:query="$emit('update:query', $event)"
      @update:category="$emit('update:category', $event)"
      @open="$emit('openAsset', $event)"
      @copy="$emit('copyPath', $event)"
    />
    <AgentConfigBrowser
      v-else
      :files="configFiles"
      :supported="configurationSupported"
      :selected-id="selectedConfigId"
      :preview="preview"
      :preview-state="previewState"
      :preview-error="previewError"
      :copied-path-id="copiedPathId"
      @select="$emit('selectConfig', $event)"
      @open="$emit('openAsset', $event)"
      @copy="$emit('copyPath', $event)"
    />
  </div>
</template>
