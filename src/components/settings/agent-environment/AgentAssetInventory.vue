<script setup lang="ts">
import { IconCopy, IconFolder, IconSearch } from "@arco-design/web-vue/es/icon";
import type { AgentAssetCategory, AgentAssetRecord } from "../../../stores/provider-types";
import { agentAssetCategoryLabels } from "../../../composables/useAgentEnvironmentCenter";

defineProps<{
  assets: AgentAssetRecord[];
  query: string;
  category: AgentAssetCategory | "all";
  categorySupported: boolean;
  copiedPathId: string | null;
}>();
const emit = defineEmits<{
  "update:query": [value: string];
  "update:category": [value: AgentAssetCategory | "all"];
  open: [asset: AgentAssetRecord];
  copy: [asset: AgentAssetRecord];
}>();

function updateCategory(value: unknown) {
  if (typeof value === "string" && categories.includes(value as AgentAssetCategory | "all")) {
    emit("update:category", value as AgentAssetCategory | "all");
  }
}

const categories: (AgentAssetCategory | "all")[] = [
  "all",
  "config",
  "skill",
  "plugin",
  "extension",
  "mcp",
  "hook",
  "statusUi",
];
const scopeLabels: Record<AgentAssetRecord["scope"], string> = {
  user: "用户",
  workspace: "工作区",
  local: "本机",
  system: "系统",
  managed: "托管",
};
const stateLabels: Record<AgentAssetRecord["effectiveState"], string> = {
  enabled: "生效",
  disabled: "禁用",
  shadowed: "被覆盖",
  blocked: "阻止",
  invalid: "无效",
  unknown: "未知",
};
const trustLabels: Record<NonNullable<AgentAssetRecord["trustState"]>, string> = {
  trusted: "已信任",
  untrusted: "未信任",
  required: "需要信任",
  unknown: "信任未知",
};
</script>

<template>
  <div class="agent-asset-inventory">
    <div class="agent-asset-filters">
      <a-input
        :model-value="query"
        allow-clear
        placeholder="搜索资产名称、ID 或路径"
        @update:model-value="$emit('update:query', $event)"
      >
        <template #prefix><IconSearch /></template>
      </a-input>
      <a-select
        :model-value="category"
        :options="categories.map((value) => ({ value, label: value === 'all' ? '全部分类' : agentAssetCategoryLabels[value] }))"
        @update:model-value="updateCategory"
      />
    </div>
    <div v-if="!categorySupported" class="agent-environment-empty">此安装不支持该类资产</div>
    <div v-else-if="assets.length === 0" class="agent-environment-empty">
      {{ query.trim() ? "没有匹配的 Agent 资产" : "未发现该类 Agent 资产" }}
    </div>
    <div v-else class="agent-asset-list">
      <div v-for="asset in assets" :key="asset.stableId" class="agent-asset-row">
        <div class="agent-asset-main">
          <span class="agent-asset-label">{{ asset.label || asset.nativeId }}</span>
          <span class="agent-asset-meta">
            {{ agentAssetCategoryLabels[asset.category] }} · {{ scopeLabels[asset.scope] }} · {{ stateLabels[asset.effectiveState] }}
            <template v-if="asset.declaredState !== asset.effectiveState">
              （声明 {{ stateLabels[asset.declaredState] }}）
            </template>
            <template v-if="asset.trustState"> · {{ trustLabels[asset.trustState] }}</template>
          </span>
          <span v-if="asset.path" class="agent-asset-path" :title="asset.path">{{ asset.path }}</span>
          <span v-if="asset.diagnostics.length" class="agent-asset-diagnostic" :title="asset.diagnostics.join('\n')">
            {{ asset.diagnostics[0] }}
          </span>
        </div>
        <span class="agent-asset-actions">
          <a-tooltip v-if="asset.path" content="复制路径">
            <a-button type="text" size="mini" @click.stop="$emit('copy', asset)">
              <template #icon><IconCopy /></template>
            </a-button>
          </a-tooltip>
          <a-tooltip v-if="asset.path" content="打开资产">
            <a-button type="text" size="mini" @click.stop="$emit('open', asset)">
              <template #icon><IconFolder /></template>
            </a-button>
          </a-tooltip>
          <span v-if="copiedPathId === asset.stableId" class="agent-asset-copied">已复制</span>
        </span>
      </div>
    </div>
  </div>
</template>
