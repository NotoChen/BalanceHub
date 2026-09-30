<script setup lang="ts">
import { CornerUpLeft } from "@lucide/vue";
import type { AgentSessionRow } from "../../stores/agent-session-types";
import { agentSessionRoleLabels } from "../../utils/agent-session-display";

defineProps<{ row: AgentSessionRow }>();
const emit = defineEmits<{ parent: [sessionRef: string] }>();
</script>

<template>
  <span class="agent-session-relation">
    <span class="agent-session-role" :class="`is-${row.role}`">{{ agentSessionRoleLabels[row.role] }}</span>
    <template v-if="row.parent.kind === 'known'">
      <button v-if="row.parent.parentRef" type="button" :title="`查看父会话：${row.parent.nativeId}`" :aria-label="`查看父会话 ${row.parent.nativeId}`" @click="emit('parent', row.parent.parentRef)"><CornerUpLeft :size="12" aria-hidden="true" /><span>查看父会话</span></button>
      <span v-else :title="`父会话 ${row.parent.nativeId} 暂不可读取`">父会话暂不可读取</span>
    </template>
    <span v-else-if="row.parent.kind === 'unknown'">父会话未知</span>
  </span>
</template>
