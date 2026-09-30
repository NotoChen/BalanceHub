<script setup lang="ts">
import { computed } from "vue";
import { CircleAlert, LoaderCircle, Plus } from "@lucide/vue";
import AgentCliIcon from "../AgentCliIcon.vue";
import WorkspaceCard from "../workspace-card/WorkspaceCard.vue";
import AgentWorkspaceIcon from "./AgentWorkspaceIcon.vue";
import AgentCatalogFeatures from "./AgentCatalogFeatures.vue";
import AgentCatalogAgentPanel from "./AgentCatalogAgentPanel.vue";
import type { AgentCatalogAction, AgentCatalogAsset, AgentCatalogAgentPanel as AgentPanel } from "../../stores/agent-catalog-types";
import type { AgentCliKind } from "../../stores/provider-types";
import type { AgentCatalogAgentSelection } from "../../composables/useAgentCatalogAgentPanel";
import { agentCatalogAgentControls } from "../../utils/agent-catalog-row";
import { agentAssetCategoryLabels } from "../../composables/useAgentAssetCatalog";

const props = defineProps<{
  asset: AgentCatalogAsset; labels: ReadonlyMap<AgentCliKind, string>; busy: boolean; error?: string; focused: boolean;
  agentFilter?: AgentCliKind | null;
  library?: boolean;
  timeField?: "createdAt" | "modifiedAt" | null;
  agentSelection?: AgentCatalogAgentSelection | null; agentPanel?: AgentPanel | null; agentLoading?: boolean; agentError?: string;
}>();
const emit = defineEmits<{
  detail: [id: string, feature?: string]; edit: [id: string]; action: [id: string, action: AgentCatalogAction, targets?: string[]];
  manage: [id: string, kind: AgentCliKind | null]; retry: []; native: [id: string];
}>();
const controls = computed(() => agentCatalogAgentControls(props.asset, props.labels, props.agentFilter));
const iconPage = computed(() => props.asset.category === "plugin" ? "extension"
  : props.asset.category === "statusUi" ? "overview" : props.asset.category);
const timeLabel = computed(() => props.timeField === "createdAt" ? "来源创建" : "来源修改");
const fileTime = computed(() => props.timeField ? props.asset[props.timeField] : null);
const formattedTime = computed(() => {
  if (!fileTime.value) return "时间未提供";
  const time = new Date(fileTime.value);
  return Number.isNaN(time.getTime()) ? "时间未提供" : time.toLocaleString("zh-CN", { hour12: false });
});
const triggers = computed(() => {
  const groups = new Map<string, { id: string; event: string; matcher: string | null }>();
  for (const rule of props.asset.hook?.rules ?? []) {
    const id = JSON.stringify([rule.event, rule.matcher]);
    groups.set(id, { id, event: rule.event, matcher: rule.matcher });
  }
  return [...groups.values()];
});
const execution = computed(() => [...new Set(props.asset.hook?.rules.map((rule) => rule.execution).filter((value) => value !== props.asset.name) ?? [])].join(" / "));
const sources = computed(() => {
  const sources = new Map<string, NonNullable<AgentCatalogAsset["hook"]>["sources"][number]>();
  for (const source of props.asset.hook?.sources ?? []) {
    sources.set(JSON.stringify([source.label, source.parentNativeAssetId ?? source.path]), source);
  }
  return [...sources.values()];
});
function expanded(kind: AgentCliKind) { return props.agentSelection?.assetId === props.asset.id && props.agentSelection.agentKind === kind; }
function showPanel(kind: AgentCliKind, visible: boolean) { if (visible || expanded(kind)) emit("manage", props.asset.id, visible ? kind : null); }
</script>

<template>
  <div class="agent-catalog-row" :class="{ 'is-focused': focused }" :data-global-asset-id="asset.id" :aria-current="focused ? 'true' : undefined" role="listitem">
    <WorkspaceCard class="agent-catalog-card" :class="`is-${asset.category}`" :fixed-height="false" :interacting="focused || agentSelection?.assetId === asset.id">
      <div class="agent-catalog-identity">
        <AgentWorkspaceIcon :page="iconPage" :size="24" />
        <div class="agent-catalog-copy">
          <div class="agent-catalog-title">
            <span v-if="library" class="agent-library-category-label">{{ agentAssetCategoryLabels[asset.category] }}</span>
            <button type="button" class="agent-catalog-name" :title="asset.name" :aria-label="`打开 ${asset.name} 详情`" @click="emit('detail', asset.id)">{{ asset.name }}</button>
            <a-tooltip v-if="error" :content="error" :trigger="['hover', 'focus']"><span class="agent-catalog-badge is-error" role="alert" tabindex="0" :aria-label="error"><CircleAlert :size="14" aria-hidden="true" /></span></a-tooltip>
          </div>
          <AgentCatalogFeatures :asset="asset" @select="emit('detail', asset.id, $event)" />
          <code v-if="execution" class="agent-hook-execution" :title="execution">{{ execution }}</code>
          <time v-if="timeField" class="agent-catalog-file-time" :datetime="fileTime || undefined">{{ timeLabel }} · {{ formattedTime }}</time>
        </div>
      </div>
      <div v-if="library || controls.length || asset.application.available || busy" class="agent-catalog-usage">
        <div v-if="controls.length" class="agent-catalog-bindings" role="group" :aria-label="`${asset.name} 的 Agent 配置`">
          <a-popover v-for="control in controls" :key="control.kind" :popup-visible="expanded(control.kind)" trigger="click" position="br" content-class="agent-catalog-agent-popover" @popup-visible-change="showPanel(control.kind, $event)">
            <button type="button" class="agent-catalog-agent-control" :class="`is-${control.state}`" :data-agent-kind="control.kind" :data-agent-state="control.state" :title="`${control.label}：${control.summary}`" :aria-label="`管理 ${asset.name} 在 ${control.label} 中的使用：${control.summary}`" aria-haspopup="dialog" :aria-expanded="expanded(control.kind)">
              <AgentCliIcon :kind="control.kind" :size="20" />
              <span class="agent-catalog-agent-copy">
                <span class="agent-catalog-agent-label">{{ control.label }}</span>
                <span class="agent-catalog-agent-state">{{ control.stateLabel }}</span>
              </span>
            </button>
            <template #content><AgentCatalogAgentPanel v-if="expanded(control.kind)" :panel="agentPanel ?? null" :label="control.label" :name="asset.name" :loading="Boolean(agentLoading)" :error="agentError || ''" :busy="busy" @close="showPanel(control.kind, false)" @retry="emit('retry')" @detail="emit('detail', asset.id, $event ? 'binding:' + $event : undefined)" @native="emit('native', $event)" @action="(action, targets) => emit('action', asset.id, action, targets)" /></template>
          </a-popover>
        </div>
        <div v-if="library || asset.application.available || busy" class="agent-catalog-row-actions">
          <a-button v-if="library" size="small" type="text" :disabled="busy" @click="emit('edit', asset.id)">编辑定义</a-button>
          <button v-if="asset.application.available" type="button" class="agent-catalog-apply-action" :disabled="busy" :aria-busy="busy" title="选择 Agent 和配置范围，预览后写入配置" :aria-label="`将 ${asset.name} 配置到 Agent`" @click="emit('action', asset.id, 'applyDefinition')">
            <LoaderCircle v-if="busy" class="agent-catalog-progress" :size="14" role="status" aria-label="正在准备资源操作" />
            <Plus v-else :size="14" aria-hidden="true" />配置到…
          </button>
          <LoaderCircle v-else-if="busy" class="agent-catalog-progress" :size="14" role="status" aria-label="正在准备资源操作" />
        </div>
      </div>
      <div v-if="asset.category === 'hook'" class="agent-hook-context">
        <div class="agent-hook-trigger">
          <span class="agent-hook-context-label">触发</span>
          <span v-for="trigger in triggers" :key="trigger.id" class="agent-hook-trigger-value">
            <span>{{ trigger.event || '事件未读取' }}</span>
            <code v-if="trigger.matcher" :title="`匹配条件：${trigger.matcher}`">{{ trigger.matcher }}</code>
          </span>
          <span v-if="!triggers.length">事件未读取</span>
        </div>
        <div class="agent-hook-sources" aria-label="Hook 来源">
          <span class="agent-hook-context-label">来源</span>
          <template v-for="source in sources" :key="source.bindingId">
            <button v-if="source.parentNativeAssetId" type="button" :title="`查看所属插件 ${source.label}`" @click="emit('native', source.parentNativeAssetId)">{{ source.label }}</button>
            <span v-else :title="source.path || undefined">{{ source.label }}</span>
          </template>
          <span v-if="!sources.length">{{ asset.ownership === 'managed' ? '共享库' : '来源未读取' }}</span>
          <small v-if="(asset.hook?.sources.length ?? 0) > 1">{{ asset.hook?.sources.length }} 个来源</small>
        </div>
      </div>
    </WorkspaceCard>
  </div>
</template>
