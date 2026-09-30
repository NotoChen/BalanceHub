<script setup lang="ts">
import { computed, nextTick, ref } from "vue";
import { Check, Minus, Share2 } from "@lucide/vue";
import AgentCliIcon from "../AgentCliIcon.vue";
import type { AgentAssetScope } from "../../stores/provider-types";
import type { AgentCatalogConfigurationChoice, AgentCatalogConfigurationSelection } from "../../stores/agent-catalog-types";
import { agentCatalogSyncLabels } from "../../utils/agent-catalog-display";
import { agentAssetScopeLabels } from "../../composables/useAgentAssetCatalog";

const props = defineProps<{ selection: AgentCatalogConfigurationSelection; selected: string[]; canCompare: boolean }>();
const emit = defineEmits<{ select: [ids: string[]]; compare: [assetId: string] }>();
const scope = ref<AgentAssetScope>("user");
const root = ref<HTMLElement | null>(null);
const scopes = computed(() => [...new Set(props.selection.choices.map((choice) => choice.scope))]);
const activeScope = computed(() => scopes.value.includes(scope.value) ? scope.value : scopes.value[0]);
const groups = computed(() => [false, true].map((shared) => ({
  shared,
  choices: props.selection.choices.filter((choice) => choice.scope === activeScope.value && choice.shared === shared),
})).filter((group) => group.choices.length));
function selectedInScope(value: AgentAssetScope) {
  return new Set(props.selection.choices.filter((choice) => choice.scope === value)
    .flatMap((choice) => choice.targetIds.filter((id) => props.selected.includes(id)))).size;
}
function isSelected(choice: AgentCatalogConfigurationChoice) {
  return choice.targetIds.length > 0 && choice.targetIds.every((id) => props.selected.includes(id));
}
function toggle(choice: AgentCatalogConfigurationChoice, checked: boolean) {
  if (!choice.available) return;
  emit("select", checked ? [...new Set([...props.selected, ...choice.targetIds])]
    : props.selected.filter((id) => !choice.targetIds.includes(id)));
}
async function viewShared(choice: AgentCatalogConfigurationChoice) {
  const shared = props.selection.choices.find((candidate) => candidate.id === choice.sharedChoiceId);
  if (!shared) return;
  scope.value = shared.scope;
  await nextTick();
  const row = root.value?.querySelector<HTMLElement>(`[data-choice-id="${CSS.escape(shared.id)}"]`);
  row?.focus({ preventScroll: true });
  row?.scrollIntoView({ block: "nearest" });
}
</script>

<template>
  <section ref="root" class="agent-configuration-selection" aria-label="选择 Agent">
    <header>
      <strong>选择 Agent</strong>
      <span aria-live="polite"><template v-if="scopes.length === 1 && activeScope">{{ agentAssetScopeLabels[activeScope] }} · </template>{{ selected.length ? `已选 ${selected.length} 项配置` : '尚未选择' }}</span>
    </header>
    <nav v-if="scopes.length > 1" class="agent-configuration-scopes" aria-label="配置范围">
      <button v-for="item in scopes" :key="item" type="button" :aria-pressed="item === activeScope" @click="scope = item">{{ agentAssetScopeLabels[item] }}<span v-if="selectedInScope(item)"> · 已选 {{ selectedInScope(item) }}</span></button>
    </nav>
    <p v-if="scopes.length > 1 && selected.some((id) => selection.choices.some((choice) => choice.scope !== activeScope && choice.targetIds.includes(id)))" class="agent-workspace-note">其他范围也有已选配置，切换范围不会取消选择；预览将包含全部已选配置。</p>
    <section v-for="group in groups" :key="String(group.shared)" class="agent-configuration-group" :class="{ 'is-shared': group.shared }">
      <h3 v-if="group.shared">共享配置</h3>
      <div class="agent-configuration-rows">
        <label v-for="choice in group.choices" :key="choice.id" :data-choice-id="choice.id" tabindex="-1" class="agent-configuration-row" :class="{ 'is-current': choice.state === 'current', 'is-selectable': choice.available, 'is-selected': isSelected(choice) }">
          <span class="agent-configuration-control">
            <input v-if="choice.available" type="checkbox" :checked="isSelected(choice)" :aria-label="`${choice.label} · ${agentAssetScopeLabels[choice.scope]} · ${choice.state === 'different' ? '同步' : '配置'}`" @change="toggle(choice, ($event.target as HTMLInputElement).checked)" />
            <Check v-else-if="choice.state === 'current'" :size="17" aria-hidden="true" />
            <Minus v-else :size="15" aria-hidden="true" />
          </span>
          <Share2 v-if="choice.shared" :size="22" class="agent-configuration-shared-icon" aria-hidden="true" />
          <AgentCliIcon v-else-if="choice.agentKinds[0]" :kind="choice.agentKinds[0]" :size="24" />
          <span class="agent-configuration-copy">
            <strong>{{ choice.label }}</strong>
            <small>{{ choice.detail }}</small>
            <small v-if="choice.reason" class="agent-configuration-reason">{{ choice.reason }}</small>
            <button v-if="choice.sharedChoiceId && choice.state !== 'current'" type="button" class="agent-catalog-text-action" @click.prevent.stop="viewShared(choice)">查看共享配置</button>
            <button v-for="(id, index) in (canCompare ? choice.relatedAssetIds : [])" :key="id" type="button" class="agent-catalog-text-action" @click.prevent.stop="emit('compare', id)">{{ choice.relatedAssetIds.length > 1 ? `查看并比较同名资源 ${index + 1}` : '查看并比较同名资源' }}</button>
          </span>
          <span class="agent-configuration-state">
            <span>{{ agentCatalogSyncLabels[choice.state] }}</span>
            <small v-if="choice.state !== 'current'">{{ choice.sharedChoiceId ? '使用共享配置' : choice.available ? (choice.state === 'different' ? '可同步' : '可配置') : '暂不可配置' }}</small>
          </span>
        </label>
      </div>
    </section>
    <p v-if="!selection.choices.length" class="agent-workspace-empty">没有可供选择的 Agent。</p>
  </section>
</template>
