<script setup lang="ts">
import { computed, nextTick, ref, watch, type UnwrapNestedRefs } from "vue";
import { Maximize2, Minimize2 } from "@lucide/vue";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentCatalogConfigurationActions from "./AgentCatalogConfigurationActions.vue";
import AgentConfigurationEditorBody from "./AgentConfigurationEditorBody.vue";
import AgentCatalogContent from "./AgentCatalogContent.vue";
import AgentWorkspaceIcon from "./AgentWorkspaceIcon.vue";
import type { useAgentResourceContent } from "../../composables/useAgentResourceContent";
import "../../styles/modules/agent-resource-content.css";
import type { AgentAssetCatalog, AgentCatalogAsset, AgentCatalogAction, AgentCatalogRelationIntent } from "../../stores/agent-catalog-types";
import type { AgentCliDescriptor } from "../../stores/provider-types";
import { agentAssetCategoryLabels, agentAssetScopeLabels, agentAssetStateLabels, agentAssetTrustLabels } from "../../composables/useAgentAssetCatalog";
import { agentCatalogActionLabels, agentCatalogDriftLabels, agentCatalogSyncLabels, agentCatalogUnresolvedLabel } from "../../utils/agent-catalog-display";
import { agentAssetFeatureMatches, agentAssetProvenanceEntries, agentAssetProvenanceLabel } from "../../utils/agent-asset-provenance";

const props = defineProps<{
  busy: boolean; visible: boolean; asset: AgentCatalogAsset | null; catalog: AgentAssetCatalog | null;
  agents: AgentCliDescriptor[]; libraryBusy: Record<string, boolean>; libraryErrors: Record<string, string>;
  focusedFeature?: string | null; resource: UnwrapNestedRefs<ReturnType<typeof useAgentResourceContent>>;
}>();
const emit = defineEmits<{
  close: []; native: [id: string]; adopt: [bindingId: string]; relation: [intent: AgentCatalogRelationIntent];
  delete: [id: string]; detail: [id: string]; library: [id: string];
  action: [action: AgentCatalogAction, targets?: string[]];
}>();
const detailRoot = ref<HTMLElement | null>(null);
const expanded = ref(false);
watch(() => props.visible, (visible) => { if (!visible) expanded.value = false; });
const drawerWidth = computed(() => expanded.value ? "calc(100vw - 32px)" : !props.resource.reading ? "min(1280px, calc(100vw - 48px))" : "min(920px, calc(100vw - 32px))");
const feature = ref<string | null>(null);
function focusFeature(value: string | null) {
  feature.value = value;
  props.resource.managementExpanded = Boolean(value && value !== "same-name" && value !== "variants");
  if (!value) return;
  const assetId = props.asset?.id;
  void nextTick(() => {
    if (!props.visible || props.asset?.id !== assetId || feature.value !== value) return;
    const target = [...(detailRoot.value?.querySelectorAll<HTMLElement>("[data-catalog-focus]") ?? [])].find((node) => node.dataset.catalogFocus === value);
    let ancestor = target?.parentElement;
    while (ancestor && ancestor !== detailRoot.value) {
      if (ancestor instanceof HTMLDetailsElement) ancestor.open = true;
      ancestor = ancestor.parentElement;
    }
    target?.scrollIntoView({ block: "nearest" });
    target?.focus({ preventScroll: true });
  });
}
watch(() => [props.visible, props.asset?.id, props.focusedFeature] as const, () => { if (props.visible) focusFeature(props.focusedFeature ?? null); }, { immediate: true });
watch(() => props.resource.reader.loading, (loading) => {
  if (!loading && props.visible && feature.value === "variants") focusFeature("variants");
});
const managementEntries = computed(() => new Map(props.resource.management.panel?.entries.map((entry) => [entry.targetId, entry]) ?? []));
const batchActions = computed(() => (props.resource.management.panel?.batchActions ?? []).filter((action) => action.available && action.targetIds.length > 1));
function toggleManagement(event: Event) {
  props.resource.managementExpanded = (event.currentTarget as HTMLDetailsElement).open;
}
const candidates = computed(() => (props.catalog?.assets ?? []).filter((candidate) => props.asset?.candidateIds.includes(candidate.id)));
const bindingDetails = computed(() => {
  const sources = props.catalog?.inventory.sources ?? [];
  return (props.asset?.bindings ?? []).map((binding) => ({
    binding, management: managementEntries.value.get(binding.id), provenance: agentAssetProvenanceEntries(binding.native.provenance, sources),
    source: props.asset?.hook?.sources.find((source) => source.bindingId === binding.id),
  }));
});
const editingSource = computed(() => props.asset?.hook?.sources.find((source) => source.bindingId === props.resource.binding?.id));
const agentLabel = (kind: string) => props.agents.find((agent) => agent.kind === kind)?.label || kind;
const separated = computed(() => (props.asset?.separatedAssetIds ?? []).map((id) => ({ id, name: props.catalog?.assets.find((item) => item.id === id)?.name || "当前未读取到的资源" })));
</script>

<template>
  <a-drawer :visible="visible" :width="drawerWidth" :footer="false" class="agent-workspace-drawer" closable mask-closable esc-to-close unmount-on-close @cancel="emit('close')">
    <template #title><div class="agent-resource-title"><span v-if="asset">{{ agentAssetCategoryLabels[asset.category] }}</span><strong>{{ asset?.name || '资源详情' }}</strong><a-button type="text" size="mini" :aria-label="expanded ? '还原面板' : '展开面板'" :title="expanded ? '还原面板' : '展开面板'" @click="expanded = !expanded"><Minimize2 v-if="expanded" :size="16" /><Maximize2 v-else :size="16" /></a-button></div></template>
    <div v-if="asset" :key="asset.id" ref="detailRoot" class="agent-modal-body">
      <div v-if="asset.ownership === 'managed' && resource.reading" class="agent-library-location"><span>已保存到共享库<template v-if="asset.version !== null"> · v{{ asset.version }}</template></span><a-button type="text" size="small" @click="emit('library', asset.id)">在共享库中查看</a-button></div>
      <header v-if="!resource.reading" class="agent-content-edit-heading"><a-button type="text" size="small" @click="resource.showContent">返回内容</a-button><span v-if="resource.sharedView">编辑共享定义</span><span v-else-if="resource.binding">编辑 <template v-if="editingSource">{{ editingSource.label }} · </template>{{ agentLabel(resource.binding.native.agentKind) }} · {{ agentAssetScopeLabels[resource.binding.native.scope] }}</span></header>
      <div v-for="parent in resource.parents" :key="parent.stableId" class="agent-resource-parent"><span>来自{{ agentAssetCategoryLabels[parent.category] }}</span><a-button type="text" size="small" @click="emit('native', parent.stableId)">{{ parent.label }}</a-button></div>
      <details v-if="candidates.length" class="agent-catalog-advanced" :open="feature === 'same-name'" data-catalog-focus="same-name" tabindex="-1">
        <summary>另有 {{ candidates.length }} 份同名资源</summary>
        <p class="agent-workspace-note">名称相同的独立资源，可分别使用；是否合并由你决定。</p>
        <div v-for="candidate in candidates" :key="candidate.id" class="agent-catalog-relation-row">
          <span><strong>{{ candidate.name }}</strong><small>{{ [...new Set(candidate.bindings.map(binding => agentLabel(binding.native.agentKind)))].join('、') || '共享库' }} · {{ candidate.bindings.length }} 处来源</small></span>
          <div class="agent-inline-actions"><a-button size="small" @click="emit('detail', candidate.id)">查看资源</a-button><a-button size="small" @click="emit('relation', { kind: 'compare', leftAssetId: asset.id, rightAssetId: candidate.id })">比较内容</a-button></div>
        </div>
      </details>
      <AgentCatalogContent v-if="resource.reading" :content="resource.reader.content" :loading="resource.reader.loading" :error="resource.reader.error" :agents="agents" @edit="resource.editContent" @retry="resource.reader.reload">
        <template #resources>
          <section v-if="resource.children.length" class="agent-resource-children" aria-label="包含的资源"><header>包含的资源 · {{ resource.children.length }}</header><button v-for="child in resource.children" :key="child.stableId" type="button" @click="emit('native', child.stableId)"><AgentWorkspaceIcon :page="child.category === 'plugin' ? 'extension' : child.category === 'statusUi' ? 'overview' : child.category" /><span>{{ child.label }}</span><small>{{ agentAssetCategoryLabels[child.category] }}</small></button></section>
        </template>
      </AgentCatalogContent>
      <p v-if="resource.reading && resource.draftBindingId" class="agent-workspace-note">{{ resource.editor.submitted ? '保存任务已提交，可在后台任务中查看结果。' : '有未保存的修改。' }}<a-button type="text" size="mini" @click="resource.selectBinding(resource.draftBindingId)">{{ resource.editor.submitted ? '查看草稿' : '继续编辑' }}</a-button></p>
      <AgentConfigurationEditorBody v-if="resource.editor.visible" v-show="!resource.sharedView" :model="resource.editor" resource-view start-in-source :initial-document-id="resource.editorDocumentId" @close="emit('close')" />
      <section v-show="resource.sharedView" class="agent-resource-shared-content">
        <header class="agent-resource-shared-heading"><strong>共享定义</strong><span>保存后可配置到其他 Agent</span></header>
        <slot name="definition" />
        <p v-if="!resource.definitionVisible" class="agent-workspace-note" role="status">正在读取共享定义…</p>
      </section>
      <details class="agent-resource-management" :open="resource.managementExpanded" @toggle="toggleManagement">
        <summary>配置来源与管理<span v-if="asset.bindings.length + asset.unresolvedTargets.length > 1" class="agent-source-count">{{ asset.bindings.length + asset.unresolvedTargets.length }} 处</span></summary>
        <div class="agent-resource-management-body">
          <header v-if="asset.ownership === 'managed' || asset.application.available" class="agent-source-toolbar" :data-catalog-focus="asset.ownership === 'managed' ? 'ownership-managed' : 'ownership-observed'" tabindex="-1">
            <span v-if="asset.ownership === 'managed'">共享库 v{{ asset.version }}</span>
            <div class="agent-inline-actions"><a-button v-if="asset.ownership === 'managed'" type="text" size="small" :disabled="busy || libraryBusy[asset.id]" @click="resource.showShared">编辑共享定义</a-button><a-button v-if="asset.application.available" type="primary" size="small" :disabled="busy || libraryBusy[asset.id]" @click="emit('action', 'applyDefinition')">{{ agentCatalogActionLabels.applyDefinition }}</a-button></div>
          </header>
          <p v-if="!asset.bindings.length && asset.application.reason" class="agent-workspace-note">{{ asset.application.reason }}</p>
          <p v-if="!asset.bindings.length && !asset.unresolvedTargets.length" data-catalog-focus="unapplied" tabindex="-1" class="agent-workspace-note">尚未配置到 Agent。选择 Agent 和范围后可预览更改。</p>
          <p v-if="resource.management.loading" role="status" class="agent-workspace-note">正在读取配置状态与可用操作…</p>
          <div v-else-if="resource.management.error" class="agent-workspace-error" role="alert"><p>{{ resource.management.error }}</p><a-button size="mini" @click="resource.management.retry">重新读取</a-button></div>
          <section v-if="batchActions.length" class="agent-catalog-batch-actions" aria-label="批量管理配置">
            <span class="agent-workspace-note">批量管理</span>
            <AgentCatalogConfigurationActions :actions="batchActions" :busy="busy" @action="(action, targets) => emit('action', action, targets)" />
          </section>
          <section v-for="{ binding, management, provenance, source } in bindingDetails" :key="binding.id" class="agent-binding-detail" :class="{ 'is-focused': feature === 'binding:' + binding.id }" :data-binding-id="binding.id" :data-catalog-focus="'binding:' + binding.id" tabindex="-1">
            <header><AgentCliIcon :kind="binding.native.agentKind" :size="20" /><strong>{{ agentLabel(binding.native.agentKind) }}</strong><span>{{ agentAssetScopeLabels[binding.native.scope] }}</span><span class="agent-binding-state">{{ agentAssetStateLabels[binding.native.effectiveState] }}</span><span v-if="management?.syncState === 'different'" class="agent-binding-difference">内容不同</span></header>
            <p v-if="source?.label" class="agent-workspace-note">{{ source.label }}</p>
            <code class="agent-binding-path" :title="binding.native.path || binding.native.nativeId">{{ binding.native.path || binding.native.nativeId }}</code>
            <p v-if="libraryErrors[binding.id]" class="agent-workspace-error" role="alert">{{ libraryErrors[binding.id] }}</p>
            <AgentCatalogConfigurationActions :actions="management?.actions ?? []" :busy="busy || libraryBusy[binding.id]" :details-notes="[management?.reason, binding.reason, asset.application.reason]" details-label="来源详情" @action="(action, targets) => emit('action', action, targets)" @native="emit('native', $event)">
              <template v-if="binding.canAdopt" #primary><a-button size="mini" :disabled="busy" :loading="libraryBusy[binding.id]" @click="emit('adopt', binding.id)">保存到共享库</a-button></template>
              <template #secondary><button type="button" class="agent-catalog-text-action" @click="resource.selectBinding(binding.id)">查看原生配置</button></template>
              <template #details>
                <dl class="agent-source-facts">
                  <div v-if="asset.ownership === 'managed' || binding.appliedVersion !== null"><dt>共享版本</dt><dd>{{ agentCatalogDriftLabels[binding.drift] }}<template v-if="binding.appliedVersion !== null"> · 已应用 v{{ binding.appliedVersion }}</template></dd></div>
                  <div v-if="management?.syncState"><dt>内容比较</dt><dd>{{ agentCatalogSyncLabels[management.syncState] }}</dd></div>
                  <div v-if="resource.management.panel?.syncSource"><dt>同步来源</dt><dd>{{ resource.management.panel.syncSource }}</dd></div>
                  <div><dt>信任状态</dt><dd>{{ agentAssetTrustLabels[binding.native.trustState] }}</dd></div>
                </dl>
                <p v-if="binding.usage?.primaryBindingId === binding.id" class="agent-workspace-note">{{ binding.usage.detail }}</p>
                <p v-if="binding.native.compatibleInstallationIds.length > 1" class="agent-workspace-note">此配置由 {{ binding.native.compatibleInstallationIds.length }} 个安装共用，修改将共同生效。</p>
                <ul v-if="provenance.length" class="agent-binding-provenance" aria-label="资产来源证据"><li v-for="entry in provenance" :key="`${entry.evidence.declarationId}:${entry.evidence.sourceId}`" :class="{ 'is-focused': agentAssetFeatureMatches(feature, entry.evidence) }" :data-catalog-focus="agentAssetFeatureMatches(feature, entry.evidence) ? feature : undefined" tabindex="-1"><span>{{ agentAssetProvenanceLabel(entry.evidence) }} · {{ agentAssetScopeLabels[entry.evidence.scope] }}</span><code>{{ entry.source?.path || '来源引用缺失' }}</code></li></ul>
                <p v-else class="agent-workspace-note">尚无可确认的来源证据</p>
              </template>
            </AgentCatalogConfigurationActions>
          </section>
          <section v-for="target in asset.unresolvedTargets" :key="target.targetId" class="agent-binding-detail" :class="{ 'is-focused': feature === 'binding:' + target.targetId }" :data-target-id="target.targetId" :data-catalog-focus="'binding:' + target.targetId" tabindex="-1">
            <header><AgentCliIcon :kind="target.agentKind" :size="20" /><strong>{{ agentLabel(target.agentKind) }}</strong><span>{{ agentAssetScopeLabels[target.scope] }}</span><span class="agent-binding-state">{{ agentCatalogUnresolvedLabel(target) }}</span></header>
            <p class="agent-workspace-note">{{ target.message }}</p>
            <AgentCatalogConfigurationActions :actions="managementEntries.get(target.targetId)?.actions ?? []" :sync-state="managementEntries.get(target.targetId)?.syncState" :busy="busy" :excluded-reasons="[target.message]" @action="(action, targets) => emit('action', action, targets)" @native="emit('native', $event)" />
          </section>
          <details v-if="separated.length || asset.manualAssociations.length" class="agent-catalog-advanced">
            <summary>整理资源关系</summary>
            <section v-if="separated.length" class="agent-association-section"><h3>已保留分开的资源 · {{ separated.length }}</h3><div v-for="item in separated" :key="item.id" class="agent-catalog-relation-row"><span>{{ item.name }}</span><a-button size="small" @click="emit('relation', { kind: 'restoreHint', leftAssetId: asset.id, rightAssetId: item.id })">恢复提示</a-button></div></section>
            <section v-if="asset.manualAssociations.length" class="agent-association-section"><h3>手动合并的对应关系</h3><div v-for="association in asset.manualAssociations" :key="association.id" class="agent-catalog-relation-row"><span><strong>{{ association.label }}</strong><small v-if="association.reason">{{ association.reason }}</small></span><a-button size="small" :disabled="!association.canDetach" :title="association.reason || undefined" @click="emit('relation', { kind: 'detach', associationId: association.id })">预览解除</a-button></div></section>
          </details>
          <section v-if="asset.definitionRemoval.available" class="agent-catalog-library-removal">
            <a-popconfirm content="删除这份未应用的共享定义及其版本记录？" ok-text="删除" cancel-text="取消" :ok-button-props="{ status: 'danger' }" @ok="emit('delete', asset.id)">
              <a-button type="text" status="danger" size="small" :loading="libraryBusy[asset.id]">从共享库删除</a-button>
            </a-popconfirm>
          </section>
          <p v-if="libraryErrors[asset.id]" class="agent-workspace-error" role="alert">{{ libraryErrors[asset.id] }}</p>
        </div>
      </details>
    </div>
  </a-drawer>
</template>
