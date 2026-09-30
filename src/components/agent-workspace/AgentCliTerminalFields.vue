<script setup lang="ts">
import { RefreshCw } from "@lucide/vue";
import type { AgentCliKind, TemporaryCliTerminalKind } from "../../stores/provider-types";
import type { SelectOption } from "../../utils/liveness-options";
import { agentCliVersionLabel } from "../../utils/cli-environment";
import AgentCliIcon from "../AgentCliIcon.vue";
import TerminalIconSelector from "../TerminalIconSelector.vue";

withDefaults(defineProps<{
  agentKind: AgentCliKind; label: string; cliValue: string;
  cliOptions: { value: string; path: string; version: string | null; preferred: boolean; available: boolean; reason: string | null }[];
  terminalKind: TemporaryCliTerminalKind; terminalOptions: SelectOption<TemporaryCliTerminalKind>[];
  probing: boolean; error: string; cliAriaLabel?: string; showTerminal?: boolean;
}>(), { showTerminal: true, cliAriaLabel: "使用的 Agent CLI 路径" });
const emit = defineEmits<{ refresh: []; "update:cliValue": [value: string]; "update:terminalKind": [value: TemporaryCliTerminalKind] }>();
function selectCli(value: unknown) { if (typeof value === "string") emit("update:cliValue", value); }
</script>

<template>
  <div class="agent-session-resume-environment">
    <header><strong>{{ showTerminal ? 'CLI 与终端' : 'Agent CLI' }}</strong><a-button size="mini" :loading="probing" @click="emit('refresh')"><template #icon><RefreshCw :size="13" /></template>重新检测</a-button></header>
    <label class="agent-session-form-field"><span>{{ label }} 路径</span><a-select :model-value="cliValue" :aria-label="cliAriaLabel" :placeholder="probing ? '正在检测 CLI' : '未检测到可用 CLI'" :disabled="!cliOptions.length" @change="selectCli"><a-option v-for="choice in cliOptions" :key="choice.value" :value="choice.value" :disabled="!choice.available" :title="choice.reason || undefined"><span class="agent-session-cli-option"><AgentCliIcon :kind="agentKind" :size="15" /><span>{{ choice.path }}{{ choice.preferred ? ' · 当前首选' : '' }}{{ choice.version ? ' · ' + agentCliVersionLabel(choice.version) : '' }}{{ !choice.available && choice.reason ? ' · ' + choice.reason : '' }}</span></span></a-option></a-select></label>
    <div v-if="showTerminal" class="agent-session-form-field"><span>终端</span><TerminalIconSelector :model-value="terminalKind" :options="terminalOptions" :loading="probing && !terminalOptions.length" @update:model-value="emit('update:terminalKind', $event)" /></div>
    <p v-if="error" class="agent-workspace-error" role="alert">{{ error }}</p>
  </div>
</template>
