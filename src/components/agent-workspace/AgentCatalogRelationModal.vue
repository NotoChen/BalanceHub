<script setup lang="ts">
import { computed } from "vue";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentCatalogComparisonDocuments from "./AgentCatalogComparisonDocuments.vue";
import AgentCatalogRelationDiff from "./AgentCatalogRelationDiff.vue";
import ContentChange from "../ContentChange.vue";
import ContextDetails from "../ContextDetails.vue";
import type { AgentCatalogRelationPreview, AgentCatalogRelationCapability, AgentCatalogRelationIntent } from "../../stores/agent-catalog-types";
import type { AgentCliDescriptor } from "../../stores/provider-types";
import { agentAssetScopeLabels } from "../../composables/useAgentAssetCatalog";
import { agentAssetProvenanceLabel } from "../../utils/agent-asset-provenance";
import { agentCatalogRelationActionLabels } from "../../utils/agent-catalog-display";

const props = defineProps<{ visible: boolean; preview: AgentCatalogRelationPreview | null; intent: AgentCatalogRelationIntent | null; agents: AgentCliDescriptor[]; loading: boolean; error: string; expired: boolean; canConfirm: boolean; canBack: boolean }>();
const emit = defineEmits<{ close: []; retry: []; back: []; choose: [capability: AgentCatalogRelationCapability]; confirm: [] }>();
const choices = computed(() => props.preview?.capabilities.filter((item) => item.intent.kind !== "keepSeparate" || item.intent.hideCandidate) ?? []);
const availableChoices = computed(() => choices.value.filter((item) => item.available));
const choiceNotes = computed(() => [...new Set(choices.value.flatMap((item) => item.reason ? [`${choiceLabel(item)}：${item.reason}`] : !item.available ? [`${choiceLabel(item)}：当前不可执行`] : []))]);
const notes = computed(() => [...new Set(props.preview?.notes ?? [])].filter((note) => note !== props.preview?.reason));
const title = computed(() => !props.intent || props.intent.kind === "compare" ? "比较同名资源" : agentCatalogRelationActionLabels[props.intent.kind]);
const differenceLabels = { added: "仅右侧有", removed: "仅左侧有", changed: "内容不同", unknown: "当前无法比较" };
function choiceLabel(capability: AgentCatalogRelationCapability) {
  const intent = capability.intent;
  if (intent.kind !== "merge") return agentCatalogRelationActionLabels[intent.kind];
  const index = props.preview?.sides.findIndex((side) => side.assetId === intent.destinationAssetId) ?? -1;
  return index >= 0 ? `合并展示到资源 ${index + 1}` : "合并展示";
}
</script>

<template>
  <a-modal :visible="visible" :width="1040" modal-class="surface-modal agent-workspace-modal agent-catalog-relation-modal" :footer="false" closable mask-closable esc-to-close unmount-on-close @cancel="emit('close')">
    <template #title>{{ title }}</template>
    <div class="agent-modal-body">
      <p v-if="loading" role="status" class="agent-workspace-note">正在读取内容与整理条件…</p>
      <div v-if="error" role="alert" class="agent-workspace-error">{{ error }}<a-button type="text" size="mini" @click="emit('retry')">重新读取</a-button></div>
      <template v-if="preview">
        <p v-if="preview.reason" class="agent-workspace-note">{{ preview.reason }}</p>
        <p class="agent-workspace-note">{{ preview.equality === 'equal' ? '完整内容一致' : preview.equality === 'different' ? '存在内容差异，合并展示仍会保留不同配置' : '当前内容不能完整比较，请核对来源和限制' }}</p>
        <AgentCatalogRelationDiff :sides="preview.sides" :agents="agents" />
        <details class="agent-catalog-comparison-provenance"><summary>来源详情与原文</summary>
        <div class="agent-catalog-comparison-sides">
          <section v-for="(side, index) in preview.sides" :key="side.assetId" class="agent-catalog-comparison-side">
            <header><small>资源 {{ index + 1 }}</small><strong>{{ side.name }}</strong><span>{{ side.ownership === 'managed' ? `共享库 v${side.version}` : 'Agent 配置' }}</span></header>
            <p v-if="side.reason" class="agent-workspace-note">{{ side.reason }}</p>
            <AgentCatalogComparisonDocuments :documents="side.definition" />
            <section v-for="binding in side.bindings" :key="binding.bindingId" class="agent-catalog-comparison-binding">
              <header><AgentCliIcon :kind="binding.agentKind" :size="17" /><strong>{{ agents.find((agent) => agent.kind === binding.agentKind)?.label || binding.agentKind }}</strong><span>{{ agentAssetScopeLabels[binding.scope] }}</span></header>
              <code v-if="binding.path">{{ binding.path }}</code>
              <p v-for="provenance in binding.provenance" :key="`${provenance.declarationId}:${provenance.sourceId}`" class="agent-workspace-note">{{ agentAssetProvenanceLabel(provenance) }} · {{ agentAssetScopeLabels[provenance.scope] }}</p>
              <p v-if="binding.reason" class="agent-workspace-note">{{ binding.reason }}</p>
              <AgentCatalogComparisonDocuments :documents="binding.documents" />
            </section>
          </section>
        </div>
        </details>
        <details v-if="preview.differences.length" class="agent-catalog-comparison-differences">
          <summary>完整内容比较摘要 · {{ preview.differences.length }}</summary>
          <section v-for="difference in preview.differences" :key="difference.path">
            <strong>{{ differenceLabels[difference.kind] }}</strong>
            <ContentChange v-if="difference.kind !== 'unknown'" :label="difference.path" :before="difference.leftSummary" :after="difference.rightSummary" />
            <template v-else><code>{{ difference.path }}</code><p v-if="difference.leftSummary">资源 1：{{ difference.leftSummary }}</p><p v-if="difference.rightSummary">资源 2：{{ difference.rightSummary }}</p></template>
            <p v-if="difference.reason">{{ difference.reason }}</p>
          </section>
        </details>
        <p v-if="preview.action !== 'compare'" class="agent-workspace-note">将整理 {{ preview.affectedBindingIds.length }} 个配置关联<template v-if="preview.affectedReceiptTargetIds.length">及 {{ preview.affectedReceiptTargetIds.length }} 个保留目标</template>。执行前会再次核对当前来源。</p>
        <div v-if="preview.action === 'compare' && availableChoices.length" class="agent-catalog-relation-choices"><a-button v-for="(capability, index) in availableChoices" :key="index" size="small" :disabled="loading" @click="emit('choose', capability)">{{ choiceLabel(capability) }}</a-button></div>
        <p v-if="preview.action !== 'compare'" class="agent-workspace-note">仅更新资源对应关系，原生配置保持原样。</p>
        <ContextDetails :key="preview.action" label="整理说明与限制">
          <p>合并展示保留各处配置；选择“不再提醒”并确认后会隐藏这组同名提示。</p>
          <p v-if="preview.action === 'compare'">查看或关闭比较不会修改配置或保存决定。</p>
          <p v-for="note in notes" :key="note">{{ note }}</p>
          <p v-for="note in choiceNotes" :key="note">{{ note }}</p>
        </ContextDetails>
      </template>
      <div v-if="expired" role="alert" class="agent-workspace-error">整理预览已过期，请重新读取。<a-button type="text" size="mini" :disabled="loading" @click="emit('retry')">重新读取整理预览</a-button></div>
      <footer class="agent-modal-actions"><a-button @click="emit('close')">{{ intent?.kind === 'compare' ? '关闭比较' : '取消' }}</a-button><a-button v-if="canBack" @click="emit('back')">返回比较</a-button><a-button v-if="preview?.token" type="primary" :disabled="!canConfirm" @click="emit('confirm')">确认{{ title }}</a-button></footer>
    </div>
  </a-modal>
</template>
