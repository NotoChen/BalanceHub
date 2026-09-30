<script setup lang="ts">
import { computed } from "vue";
import { Check } from "@lucide/vue";
import ContextDetails from "../ContextDetails.vue";
import { agentCatalogSyncLabels } from "../../utils/agent-catalog-display";
import type { AgentCatalogAction, AgentCatalogAgentPanel, AgentCatalogPanelAction } from "../../stores/agent-catalog-types";

const props = withDefaults(defineProps<{
  actions: AgentCatalogPanelAction[];
  syncState?: AgentCatalogAgentPanel["entries"][number]["syncState"];
  busy?: boolean;
  excludedReasons?: (string | null | undefined)[];
  detailsNotes?: (string | null | undefined)[];
  detailsLabel?: string;
}>(), { busy: false, excludedReasons: () => [], detailsNotes: () => [], detailsLabel: "操作说明" });
const emit = defineEmits<{ action: [action: AgentCatalogAction, targets: string[]]; native: [id: string] }>();
const primary = computed(() => props.actions.filter((action) => action.available && action.action !== "removeBinding"));
const removals = computed(() => props.actions.filter((action) => action.available && action.action === "removeBinding"));
const parents = computed(() => [...new Set(props.actions.flatMap((action) => action.parentNativeAssetId ? [action.parentNativeAssetId] : []))]);
const notes = computed(() => {
  const reasons = new Map<string, Set<string>>();
  for (const note of props.detailsNotes) if (note && !props.excludedReasons.includes(note)) reasons.set(note, new Set());
  for (const action of props.actions) {
    const reason = action.reason || (!action.available ? "当前不可执行" : "");
    if (!reason || props.excludedReasons.includes(reason)) continue;
    const labels = reasons.get(reason) ?? new Set<string>();
    labels.add(action.label);
    reasons.set(reason, labels);
  }
  const values = [...reasons].map(([reason, labels]) => labels.size ? `${[...labels].join("、")}：${reason}` : reason);
  if (props.syncState === "unknown") values.unshift("内容比较：待核对");
  return values;
});
const showSync = computed(() => props.syncState === "current" || props.syncState === "different");
</script>

<template>
  <div class="agent-catalog-configuration-actions">
    <div v-if="primary.length || parents.length || showSync || $slots.primary" class="agent-catalog-scope-actions">
      <span v-if="showSync && syncState" class="agent-catalog-sync-state" :class="`is-${syncState}`" :title="syncState === 'current' ? '与同步来源的内容一致' : undefined"><Check v-if="syncState === 'current'" :size="13" aria-hidden="true" />{{ agentCatalogSyncLabels[syncState] }}</span>
      <a-button v-for="(action, index) in primary" :key="`${action.action}:${index}`" size="mini" :disabled="busy" @click="emit('action', action.action, [...action.targetIds])">{{ action.label }}</a-button>
      <a-button v-for="id in parents" :key="id" type="text" size="mini" @click="emit('native', id)">管理所属插件</a-button>
      <slot name="primary" />
    </div>
    <div v-if="removals.length || $slots.secondary" class="agent-catalog-scope-secondary">
      <slot name="secondary" />
      <button v-for="(action, index) in removals" :key="index" type="button" class="agent-catalog-remove-action" :disabled="busy" @click="emit('action', action.action, [...action.targetIds])">{{ action.label }}</button>
    </div>
    <ContextDetails v-if="notes.length || $slots.details" :label="detailsLabel" class="agent-catalog-action-details">
      <p v-for="note in notes" :key="note" class="agent-catalog-action-note">{{ note }}</p>
      <slot name="details" />
    </ContextDetails>
  </div>
</template>
