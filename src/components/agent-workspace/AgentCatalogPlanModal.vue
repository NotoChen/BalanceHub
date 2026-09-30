<script setup lang="ts">
import { computed } from "vue";
import AgentCliIcon from "../AgentCliIcon.vue";
import ContentChange from "../ContentChange.vue";
import AgentCatalogConfigurationSelector from "./AgentCatalogConfigurationSelector.vue";
import type { AgentAssetCatalog, AgentCatalogAction, AgentCatalogConfigurationSelection, AgentCatalogPlan, AgentCatalogRelationIntent } from "../../stores/agent-catalog-types";
import type { AgentCatalogTargetChoice } from "../../composables/useAgentCatalogConsole";
import { agentCatalogActionLabels } from "../../utils/agent-catalog-display";
import { agentAssetCategoryLabels, agentAssetScopeLabels } from "../../composables/useAgentAssetCatalog";

const props = defineProps<{
  visible: boolean;
  comparisonAssetId: string | null;
  catalog: AgentAssetCatalog | null;
  name: string;
  action: AgentCatalogAction;
  choices: AgentCatalogTargetChoice[];
  selection: AgentCatalogConfigurationSelection | null;
  selected: string[];
  plan: AgentCatalogPlan | null;
  preparing: boolean;
  error: string;
  expired: boolean;
  canConfirm: boolean;
  canReturnToEditor?: boolean;
}>();
const emit = defineEmits<{ close: []; prepare: []; confirm: []; select: [ids: string[]]; compare: [intent: AgentCatalogRelationIntent] }>();
function compare(assetId: string) {
  if (props.comparisonAssetId && props.selection?.choices.some((choice) => choice.relatedAssetIds.includes(assetId))) {
    emit("compare", { kind: "compare", leftAssetId: props.comparisonAssetId, rightAssetId: assetId });
  }
}
const nativeAssets = computed(() => new Map((props.catalog?.inventory.assets ?? []).map((asset) => [asset.stableId, asset])));
function affectedAssets(ids: string[]) {
  return ids.map((id) => ({ id, native: nativeAssets.value.get(id) ?? null }));
}
function toggle(id: string, checked: boolean) {
  emit("select", checked ? [...new Set([...props.selected, id])] : props.selected.filter((value) => value !== id));
}
</script>

<template>
  <a-modal :visible="visible" :width="820" modal-class="surface-modal agent-workspace-modal" :footer="false" closable mask-closable esc-to-close unmount-on-close @cancel="emit('close')">
    <template #title>{{ action === 'applyDefinition' ? '配置到 Agent' : agentCatalogActionLabels[action] }} · {{ name }}</template>
    <div class="agent-modal-body">
      <p v-if="action === 'applyDefinition' && !selection" class="agent-workspace-note">确认后写入所选 Agent；是否启用由各 Agent 的配置决定。</p>
      <p v-if="canReturnToEditor" class="agent-workspace-note">草稿已保留，确认写入后才保存。</p>
      <p v-if="action === 'removeBinding'" class="agent-workspace-note">移除选中的原生配置；Skill 链接只解除链接，独立目录会删除预览中的文件。共享目录会影响读取该来源的 Agent，请核对下方差异与影响范围。共享库中的定义保留。</p>
      <AgentCatalogConfigurationSelector v-if="selection" :selection="selection" :selected="selected" :can-compare="Boolean(comparisonAssetId)" @select="emit('select', $event)" @compare="compare" />
      <section v-else-if="choices.length" class="agent-target-selection" aria-label="选择配置位置">
        <header><strong>{{ action === 'applyDefinition' ? '选择 Agent' : '选择要操作的配置' }}</strong><span aria-live="polite">{{ selected.length ? `已选 ${selected.length} 处` : '尚未选择' }}</span></header>
        <div class="agent-target-choices">
          <label v-for="choice in choices" :key="choice.id" class="agent-target-choice" :class="{ 'is-unavailable': !choice.available, 'is-selected': selected.includes(choice.id) }">
            <input type="checkbox" :checked="selected.includes(choice.id)" :disabled="!choice.available" @change="toggle(choice.id, ($event.target as HTMLInputElement).checked)" />
            <AgentCliIcon :kind="choice.agentKind" :size="20" /><span><strong>{{ choice.label }}</strong><small>{{ choice.detail }}</small><small v-if="choice.reason">{{ choice.reason }}</small></span>
          </label>
        </div>
      </section>
      <p v-if="preparing && !choices.length && !selection" class="agent-workspace-note" role="status">正在核对已有配置与可用范围…</p>
      <p v-else-if="!choices.length && !selection && !error" class="agent-workspace-empty">没有可供选择的目标。</p>
      <div v-if="error" class="agent-workspace-error" role="alert">{{ error }}<a-button size="mini" type="text" @click="emit('prepare')">重新读取</a-button></div>
      <p v-if="expired" class="agent-workspace-error">更改预览已过期，请重新预览。</p>
      <section v-if="plan?.definitionChange && selected.length" class="agent-catalog-version-change"><strong>确认后{{ plan.definitionChange.kind === 'adopt' ? '收录' : '保存' }}共享版本</strong><span>{{ plan.definitionChange.beforeVersion === null ? '未收录' : `v${plan.definitionChange.beforeVersion}` }} → v{{ plan.definitionChange.afterVersion }}</span><p>随后分别写入所选 Agent；某个目标失败不会撤销已保存版本。</p></section>
      <template v-if="plan && selected.length">
        <p v-for="note in plan.notes" :key="note" class="agent-workspace-note">{{ note }}</p>
        <section v-for="target in plan.targets" :key="target.targetId" class="agent-target-plan">
          <header><AgentCliIcon :kind="target.agentKind" :size="18" /><strong>{{ target.label }}</strong><span>{{ agentAssetScopeLabels[target.scope] }}</span><span>{{ target.available ? '可执行' : '不可执行' }}</span></header>
          <p v-if="target.reason" class="agent-workspace-error">{{ target.reason }}</p>
          <ContentChange v-for="(change, index) in target.changes" :key="index" class="agent-catalog-plan-change" v-bind="change" />
          <details v-if="target.affectedAssetIds.length" class="agent-plan-impact" :open="action === 'removeBinding'">
            <summary>影响 {{ target.affectedAssetIds.length }} 个来源入口</summary>
            <ul><li v-for="impact in affectedAssets(target.affectedAssetIds)" :key="impact.id">
              <template v-if="impact.native"><span><AgentCliIcon :kind="impact.native.agentKind" :size="16" /><strong>{{ impact.native.label || impact.native.nativeId }}</strong> · {{ agentAssetCategoryLabels[impact.native.category] }} · {{ agentAssetScopeLabels[impact.native.scope] }}</span><code v-if="impact.native.path">{{ impact.native.path }}</code></template>
              <span v-else>当前结果中未找到此资源，执行前会再次核对。</span>
            </li></ul>
          </details>
        </section>
        <p v-if="plan.targets.some((target) => !target.available)" class="agent-workspace-error">有目标无法执行，请调整选择后重新预览。</p>
      </template>
      <footer class="agent-modal-actions"><a-button @click="emit('close')">{{ canReturnToEditor ? '返回编辑' : '取消' }}</a-button><a-button :type="plan?.token ? 'secondary' : 'primary'" :loading="preparing" :disabled="!selected.length" @click="emit('prepare')">{{ plan?.token ? '重新预览' : '预览更改' }}</a-button><a-button v-if="plan?.token" type="primary" :status="action === 'removeBinding' ? 'danger' : 'normal'" :disabled="!canConfirm" @click="emit('confirm')">{{ action === 'removeBinding' ? '确认移除' : action === 'enable' ? '确认启用' : action === 'disable' ? '确认停用' : '确认写入' }}</a-button></footer>
    </div>
  </a-modal>
</template>
