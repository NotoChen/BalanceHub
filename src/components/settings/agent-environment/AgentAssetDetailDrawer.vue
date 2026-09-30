<script setup lang="ts">
import { computed } from "vue";
import { IconCopy } from "@arco-design/web-vue/es/icon";
import AgentAssetPreview from "./AgentAssetPreview.vue";
import ContextDetails from "../../ContextDetails.vue";
import type {
  AgentAssetActionKind,
  AgentAssetDeclarationRole,
  AgentAssetReadResult,
  AgentAssetSource,
  AgentConfigurationContext,
  AgentInstallation,
} from "../../../stores/provider-types";
import {
  agentAssetActionLabels,
  agentAssetActionReason,
  agentAssetCategoryLabels,
  agentAssetScopeLabels,
  agentAssetStateLabels,
  agentAssetTrustLabels,
  type AgentAssetDetailView,
} from "../../../composables/useAgentAssetCatalog";
import { agentAssetAccessRiskLabels, type AgentAssetDetailTarget, type AgentAssetPreviewState } from "../../../composables/useAgentAssetConsole";
import { formatAgentAssetDiagnostics } from "../../../utils/agent-environment-diagnostics";
import { agentAssetInstallationLabels, agentAssetProvenanceEntries, agentAssetProvenanceLabel } from "../../../utils/agent-asset-provenance";

const props = defineProps<{
  visible: boolean;
  detail: AgentAssetDetailView | null;
  source: AgentAssetSource | null;
  sourceContext: AgentConfigurationContext | null;
  sourceInstallations: AgentInstallation[];
  preview: AgentAssetReadResult | null;
  previewState: AgentAssetPreviewState;
  previewError: string;
  copiedPathId: string | null;
  busy: (id: string) => boolean;
  error: (id: string) => string | null;
}>();
const emit = defineEmits<{
  close: [];
  detail: [id: string];
  source: [id: string];
  action: [kind: AgentAssetDetailTarget, id: string, action: AgentAssetActionKind];
  copy: [kind: AgentAssetDetailTarget, id: string];
  hook: [id: string];
}>();
const selection = computed(() => {
  if (props.detail) return { kind: "asset" as const, id: props.detail.row.asset.stableId, item: props.detail.row.asset };
  if (props.source) return { kind: "source" as const, id: props.source.id, item: props.source };
  return null;
});
const context = computed(() => props.detail?.row.context ?? props.sourceContext);
const installations = computed(() => props.detail?.compatibleInstallations ?? props.sourceInstallations);
const diagnostics = computed(() => props.detail?.diagnostics ?? formatAgentAssetDiagnostics(props.source?.diagnostics ?? []));
const provenance = computed(() => agentAssetProvenanceEntries(props.detail?.row.asset.provenance ?? [], props.detail?.row.sources ?? []));
const previewAction = computed(() => selection.value?.item.actions.find((item) => item.action === "preview"));
const canCopyPath = computed(() => Boolean(selection.value?.item.path && selection.value.item.actions.some((item) => item.action === "inspect" && item.available)));
const actions = computed(() => selection.value?.item.actions.filter((item) => item.action !== "inspect" && item.action !== "preview"
  && !(props.detail?.row.asset.category === "hook" && (item.action === "enable" || item.action === "disable"))) ?? []);
const availableActions = computed(() => actions.value.filter((action) => action.available));
const actionNotes = computed(() => actions.value.flatMap((action) => {
  const notes = [
    ...(!action.available ? [agentAssetActionReason(action)] : []),
    ...(action.reloadEffect ? [`生效方式：${action.reloadEffect}`] : []),
    ...(action.trustEffect ? [`信任影响：${action.trustEffect}`] : []),
    ...action.risks.map((risk) => agentAssetAccessRiskLabels[risk]),
  ].filter(Boolean);
  return notes.length ? [{ action: action.action, label: agentAssetActionLabels[action.action], notes: [...new Set(notes)] }] : [];
}));
const relatedReferences = computed(() => props.detail ? [
  { label: "提供方", reference: props.detail.provider }, { label: "操作归属", reference: props.detail.actionOwner },
  { label: "替换生效项", reference: props.detail.winner },
].filter((entry) => entry.reference) : []);
const roleLabels: Record<AgentAssetDeclarationRole, string> = { definition: "定义", stateOverlay: "状态叠加", policyOverlay: "策略叠加" };
const declaredLabels = { enabled: "启用", disabled: "停用", pending: "待确认", rejected: "已拒绝", unknown: "未知" };
</script>

<template>
  <a-drawer :visible="visible" width="min(720px, calc(100vw - 24px))" popup-container="body" class="agent-asset-detail-drawer" closable mask-closable esc-to-close unmount-on-close :footer="false" @update:visible="(value: boolean) => !value && emit('close')">
    <template #title>{{ selection?.item.label || "资产详情" }}</template>
    <div v-if="selection" :key="`${selection.kind}:${selection.id}`" class="agent-asset-detail-content">
      <section class="agent-asset-detail-section">
        <header><strong>{{ detail ? agentAssetCategoryLabels[detail.row.asset.category] : "配置来源" }}</strong><span v-if="detail && detail.row.asset.category !== 'hook'" class="agent-asset-state" :class="`is-${detail.row.asset.effectiveState}`">{{ agentAssetStateLabels[detail.row.asset.effectiveState] }}</span><span>{{ agentAssetScopeLabels[selection.item.scope] }}</span></header>
        <p v-if="selection.item.path" class="agent-asset-detail-path">{{ selection.item.path }}</p>
        <div v-if="detail?.facts.length" class="agent-asset-detail-facts"><span v-for="fact in detail.facts" :key="fact.label">{{ fact.label }}<strong>{{ fact.value }}</strong></span></div>
        <div v-else-if="!detail && source" class="agent-asset-detail-facts"><span>类型<strong>{{ source.sourceKind === 'directory' ? "目录" : "文件" }}</strong></span><span>安装来源<strong>{{ agentAssetInstallationLabels[source.origin] }}</strong></span><span>当前状态<strong>{{ source.revision.isMissing ? "尚未创建" : "已发现" }}</strong></span></div>
        <p v-if="detail?.row.asset.category === 'hook'" class="agent-asset-detail-note"><a-button type="text" size="small" @click="emit('hook', detail.row.asset.stableId)">进入 Hook 页面</a-button></p>
        <div v-if="availableActions.length || canCopyPath" class="agent-asset-detail-actions">
          <a-button v-for="item in availableActions" :key="item.action" size="small" :type="item.action === 'remove' ? 'text' : 'secondary'" :status="item.action === 'remove' ? 'danger' : 'normal'" :disabled="busy(selection.id)" @click="emit('action', selection.kind, selection.id, item.action)">{{ agentAssetActionLabels[item.action] }}</a-button>
          <a-button v-if="canCopyPath" type="text" size="small" @click="emit('copy', selection.kind, selection.id)"><template #icon><IconCopy /></template>{{ copiedPathId === selection.id ? "已复制路径" : "复制路径" }}</a-button>
        </div>
        <p v-if="error(selection.id)" class="agent-environment-stale-error" role="alert">{{ error(selection.id) }}</p>
        <ContextDetails v-if="actionNotes.length" label="操作说明">
          <div v-for="item in actionNotes" :key="item.action" class="agent-asset-action-note"><strong>{{ item.label }}</strong><p v-for="note in item.notes" :key="note">{{ note }}</p></div>
        </ContextDetails>
      </section>

      <section v-if="previewAction" class="agent-asset-detail-section">
        <header><strong>配置预览</strong><a-button v-if="previewAction.available" size="small" :loading="previewState === 'loading'" :disabled="busy(selection.id)" @click="emit('action', selection.kind, selection.id, 'preview')">{{ previewState === 'idle' ? '读取预览' : '重新读取' }}</a-button></header>
        <p v-if="!previewAction.available" class="agent-asset-detail-note">{{ agentAssetActionReason(previewAction) }}</p>
        <AgentAssetPreview v-else :preview="preview" :state="previewState" :error="previewError" />
      </section>

      <section v-if="diagnostics.length" class="agent-asset-detail-section"><header><strong>读取与操作提示</strong><span>{{ diagnostics.length }}</span></header><ul class="agent-asset-diagnostics"><li v-for="(diagnostic, index) in diagnostics" :key="index">{{ diagnostic }}</li></ul></section>

      <section v-if="detail && (relatedReferences.length || detail.children.length || detail.affected.length)" class="agent-asset-detail-section">
        <header><strong>关系与操作归属</strong></header>
        <div v-if="relatedReferences.length" class="agent-asset-relations">
          <div v-for="entry in relatedReferences" :key="entry.label"><span>{{ entry.label }}</span><a-button v-if="entry.reference && !entry.reference.missing" type="text" size="small" @click="emit('detail', entry.reference.id)">{{ entry.reference.label }}</a-button><span v-else>{{ entry.reference?.label }}（未找到）</span></div>
        </div>
        <div v-if="detail.children.length || detail.affected.length" class="agent-asset-related-groups"><div v-for="entry in [{ label: '提供的子资产', items: detail.children }, { label: '已知受影响资产', items: detail.affected }].filter((group) => group.items.length)" :key="entry.label"><strong>{{ entry.label }} · {{ entry.items.length }}</strong><a-button v-for="item in entry.items" :key="item.id" type="text" size="small" :disabled="item.missing" @click="emit('detail', item.id)">{{ item.label }}{{ item.missing ? "（未找到）" : "" }}</a-button></div></div>
      </section>

      <ContextDetails label="来源与诊断" class="agent-asset-advanced">
        <section class="agent-asset-detail-section">
          <header><strong>配置位置与关联安装</strong></header>
          <template v-if="context"><p class="agent-asset-detail-path">{{ context.profile || "默认配置" }} · {{ context.configRoot }}</p><p class="agent-asset-detail-note">{{ context.workspaceId || "全局与用户配置" }} · {{ agentAssetTrustLabels[context.trustContext] }}</p><p v-if="context.compatibleInstallationIds.length > 1" class="agent-asset-shared">此配置由 {{ context.compatibleInstallationIds.length }} 个安装共享，一次修改可能共同影响这些安装。</p></template>
          <p v-else class="agent-asset-detail-note">未找到配置上下文，请刷新盘点。</p>
          <ul v-if="installations.length" class="agent-asset-detail-list"><li v-for="installation in installations" :key="installation.id"><strong>{{ installation.label }} · {{ installation.installedVersion || "版本未知" }}</strong><code>{{ installation.executablePath || "可执行文件不可用" }}</code><span v-if="detail?.selectedInstallation?.id === installation.id">当前操作使用此安装</span></li></ul>
        </section>
        <div v-if="detail" class="agent-asset-advanced-content">
          <div class="agent-asset-detail-facts"><span>关系<strong>{{ detail.row.relations.join(" · ") }}</strong></span><span>信任<strong>{{ agentAssetTrustLabels[detail.row.asset.trustState] }}</strong></span></div>
          <section class="agent-asset-detail-section">
            <header><strong>提供方式与安装来源</strong></header>
            <code class="agent-asset-detail-id">原生 ID：{{ detail.row.asset.nativeId }}</code>
            <div v-for="entry in provenance" :key="`${entry.evidence.declarationId}:${entry.evidence.sourceId}`" class="agent-asset-declaration"><strong>{{ agentAssetProvenanceLabel(entry.evidence) }}</strong><small>配置范围：{{ agentAssetScopeLabels[entry.evidence.scope] }}</small><code>{{ entry.source?.path || '来源引用缺失' }}</code></div>
            <p v-if="!provenance.length" class="agent-asset-detail-note">尚无可确认的来源证据</p>
            <p v-if="detail.row.asset.resolution.controlSource" class="agent-asset-detail-note">策略证据：{{ detail.row.asset.resolution.controlSource.kind === 'source' ? detail.row.asset.resolution.controlSource.sourceId : detail.row.asset.resolution.controlSource.declarationId }}</p>
          </section>
          <section class="agent-asset-detail-section">
            <header><strong>声明与贡献来源</strong><span>{{ detail.row.asset.representedDeclarationIds.length }} 个代表声明 · {{ detail.row.asset.resolution.contributorIds.length }} 个贡献者</span></header>
            <div v-for="entry in detail.declarations" :key="entry.declaration.id" class="agent-asset-declaration">
              <div><strong>{{ entry.declaration.label || entry.declaration.nativeId }}</strong><small>{{ roleLabels[entry.declaration.role] }} · {{ agentAssetScopeLabels[entry.declaration.scope] }} · {{ declaredLabels[entry.declaration.declaredState] }}</small></div>
              <p>{{ entry.represented ? "代表声明" : "" }}{{ entry.represented && entry.contributor ? " · " : "" }}{{ entry.contributor ? "参与有效结果" : "" }} · {{ entry.declaration.participation.kind === 'participates' ? "参与解析" : "已抑制" }} · {{ agentAssetTrustLabels[entry.declaration.trustState] }}</p>
              <code>{{ entry.source?.path || "来源引用缺失" }}</code>
              <small>原生 ID：{{ entry.declaration.nativeId }} · 声明位置：{{ entry.declaration.declarationKey }} · 解析器 {{ entry.declaration.evidence.parserVersion }}</small>
              <p v-for="(diagnostic, index) in entry.diagnostics" :key="index" class="agent-asset-diagnostic">{{ diagnostic }}</p>
            </div>
            <div v-if="detail.row.sources.length" class="agent-asset-source-links"><a-button v-for="item in detail.row.sources" :key="item.id" type="text" size="small" @click="emit('source', item.id)">查看来源：{{ item.label }}</a-button></div>
          </section>

          <section v-if="detail.mechanisms.length" class="agent-asset-detail-section">
            <header><strong>原生操作机制</strong></header>
            <details v-for="mechanism in detail.mechanisms" :key="mechanism.id" class="agent-asset-mechanism">
              <summary>{{ agentAssetActionLabels[mechanism.action] }} · {{ mechanism.executableArgv.length ? 'Agent 原生命令' : '原生配置' }}</summary>
              <dl><dt>配置范围</dt><dd>{{ mechanism.scopes.map((scope) => agentAssetScopeLabels[scope]).join(' · ') }}</dd><dt>状态核对</dt><dd>{{ mechanism.inspection }}</dd><dt>生效方式</dt><dd>{{ mechanism.reloadEffect || "未声明" }}</dd></dl>
              <pre v-if="mechanism.sourceSchema" class="agent-asset-mechanism-preview">{{ mechanism.sourceSchema }}</pre>
              <pre v-if="mechanism.executableArgv.length" class="agent-asset-mechanism-preview">{{ mechanism.executableArgv.join(' ') }}</pre>
            </details>
          </section>
        </div>
      </ContextDetails>
    </div>
  </a-drawer>
</template>
