<script setup lang="ts">
import type { UnwrapNestedRefs } from "vue";
import { Download } from "@lucide/vue";
import AgentVersionStatus from "./AgentVersionStatus.vue";
import AgentInstallationDetail from "../settings/agent-environment/AgentInstallationDetail.vue";
import type { useAgentInstallationManager } from "../../composables/useAgentInstallationManager";
import type { useAgentHookConsole } from "../../composables/useAgentHookConsole";
import type { AgentLifecycleTarget } from "../../stores/agent-lifecycle-types";
import type { AgentCliKind } from "../../stores/provider-types";
import { agentCliVersionLabel } from "../../utils/cli-environment";
defineProps<{ target: AgentLifecycleTarget; model: UnwrapNestedRefs<ReturnType<typeof useAgentInstallationManager>>; hooks: UnwrapNestedRefs<ReturnType<typeof useAgentHookConsole>> }>();
const emit = defineEmits<{ hooks: [kind: AgentCliKind] }>();
</script>

<template>
  <article class="agent-installation-target">
    <header class="agent-installation-heading">
      <strong>{{ target.channelLabel }}</strong>
      <span class="agent-installation-version">{{ agentCliVersionLabel(target.installation.installedVersion ?? '') || '版本未读取' }}<template v-if="target.releaseTrack"> · {{ target.releaseTrack }}</template></span>
      <span v-if="target.isCurrent" class="agent-installation-current">当前使用</span>
    </header>
    <code>{{ target.installation.executablePath || target.directory || '未读取到安装路径' }}</code>
    <AgentVersionStatus :version="target.version" :checking="model.store.checkingVersions && target.version.source !== 'unknown'" />
    <div class="agent-installation-actions">
      <a-button v-if="model.canAdopt(target) && !target.isCurrent" size="small" :disabled="Boolean(model.savingPath)" @click="model.adopt(target)">采用此安装</a-button>
      <a-button type="text" size="small" :aria-expanded="model.diagnosticInstallationId === target.installation.id" @click="model.showDiagnostics(target.installation.id)">{{ model.diagnosticInstallationId === target.installation.id ? '收起诊断' : '诊断详情' }}</a-button>
      <div v-for="action in target.actions" :key="action.kind" class="agent-installation-action">
        <a-button v-if="action.available" size="small" type="primary" :disabled="Boolean(model.lifecycle.preparingTargetId)" :loading="model.lifecycle.preparingTargetId === target.id" @click="model.lifecycle.prepare(target, action.kind)"><template #icon><Download :size="14" /></template>{{ target.version.latestVersion && !target.version.stale ? `升级至 ${target.version.latestVersion}` : '检查并升级' }}</a-button>
        <small v-if="!action.available && target.version.state !== 'upToDate' && target.version.state !== 'aheadOfLatest'">{{ action.reasonMessage || '当前不可升级' }}</small>
      </div>
    </div>
    <AgentInstallationDetail v-if="model.diagnosticInstallationId === target.installation.id" :installation="target.installation" :hook-inspection="hooks.inspectionFor(target.agentKind)" @hooks="emit('hooks', target.agentKind)" />
  </article>
</template>
