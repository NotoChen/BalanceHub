<script setup lang="ts">
import { computed } from "vue";
import type { AgentCatalogDefinition, AgentCatalogSaveRequest } from "../../stores/agent-catalog-types";
import type { AgentCliDescriptor } from "../../stores/provider-types";
import ContentChange from "../ContentChange.vue";

const props = defineProps<{ definition: AgentCatalogDefinition | null; request: AgentCatalogSaveRequest; mcpDraft: string; agents: AgentCliDescriptor[] }>();
const changes = computed(() => {
  const before = props.definition;
  const after = props.request;
  const entries: Array<{ label: string; before: string | null; after: string | null }> = [
    { label: "名称", before: before?.name ?? null, after: after.name },
  ];
  if (after.category === "mcp") entries.push({ label: "MCP 连接配置", before: before?.mcp ? JSON.stringify(before.mcp, null, 2) : null, after: props.mcpDraft });
  if (after.category === "skill") entries.push({ label: "SKILL.md", before: before?.skillMarkdown ?? null, after: after.skillMarkdown });
  if (after.category === "hook") {
    const oldVariants = new Map(before?.hook?.variants.map((variant) => [variant.agentKind, variant]));
    const newVariants = new Map(after.hook?.variants.map((variant) => [variant.agentKind, variant]));
    for (const kind of new Set([...oldVariants.keys(), ...newVariants.keys()])) {
      const oldVariant = oldVariants.get(kind);
      const newVariant = newVariants.get(kind);
      entries.push({
        label: `${props.agents.find((agent) => agent.kind === kind)?.label ?? kind} · Hook`,
        before: oldVariant ? `事件：${oldVariant.event}\n${oldVariant.groupJson}` : null,
        after: newVariant ? `事件：${newVariant.event}\n${newVariant.groupJson}` : null,
      });
    }
  }
  return entries.filter((entry) => entry.before !== entry.after);
});
</script>

<template>
  <section class="agent-definition-comparison" aria-label="共享定义修改差异">
    <ContentChange v-for="change in changes" :key="change.label" v-bind="change" />
    <p v-if="!changes.length" class="agent-workspace-note">内容未修改。</p>
  </section>
</template>

<style scoped>
.agent-definition-comparison { display: grid; min-width: 0; gap: 16px; }
</style>
