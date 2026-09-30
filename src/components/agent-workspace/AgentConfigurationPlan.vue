<script setup lang="ts">
import type { AgentConfigurationPlan } from "../../stores/agent-configuration-types";
import ContentChange from "../ContentChange.vue";
import AgentConfigurationDiagnostics from "./AgentConfigurationDiagnostics.vue";
defineProps<{ plan: AgentConfigurationPlan }>();
</script>

<template>
  <section class="agent-configuration-plan" aria-label="配置更改预览">
    <ContentChange v-for="(change, index) in plan.changes" :key="index" class="agent-configuration-plan-change" v-bind="change" />
    <AgentConfigurationDiagnostics :diagnostics="plan.diagnostics" />
    <ul v-if="plan.reloadHints.length" class="agent-configuration-reload-hints"><li v-for="hint in plan.reloadHints" :key="hint">{{ hint }}</li></ul>
  </section>
</template>
