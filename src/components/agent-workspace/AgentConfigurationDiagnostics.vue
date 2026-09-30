<script setup lang="ts">
import type { AgentConfigurationDiagnostic } from "../../stores/agent-configuration-types";
defineProps<{ diagnostics: AgentConfigurationDiagnostic[] }>();
</script>

<template>
  <ul v-if="diagnostics.length" class="agent-configuration-diagnostics">
    <li v-for="(diagnostic, index) in diagnostics" :key="diagnostic.code + ':' + index" :class="'agent-configuration-diagnostic-' + diagnostic.severity">
      <span>{{ diagnostic.message }}</span><small v-if="diagnostic.line !== null"> 第 {{ diagnostic.line }} 行{{ diagnostic.column !== null ? '，第 ' + diagnostic.column + ' 列' : '' }}</small>
    </li>
  </ul>
</template>
