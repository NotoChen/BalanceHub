<script setup lang="ts">
import { computed, type UnwrapNestedRefs } from "vue";
import { ChevronRight, RefreshCw, Search } from "@lucide/vue";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentInstallationTarget from "./AgentInstallationTarget.vue";
import type { useAgentInstallationManager } from "../../composables/useAgentInstallationManager";
import type { useAgentEnvironmentCenter } from "../../composables/useAgentEnvironmentCenter";
import type { useAgentHookConsole } from "../../composables/useAgentHookConsole";
import type { AgentCliKind } from "../../stores/provider-types";
import { agentCliVersionLabel } from "../../utils/cli-environment";

const props = defineProps<{
  model: UnwrapNestedRefs<ReturnType<typeof useAgentInstallationManager>>;
  center: UnwrapNestedRefs<ReturnType<typeof useAgentEnvironmentCenter>>;
  hooks: UnwrapNestedRefs<ReturnType<typeof useAgentHookConsole>>;
}>();
const emit = defineEmits<{ hooks: [kind: AgentCliKind]; installationGuide: [kind: AgentCliKind] }>();
const targets = computed(() => [...props.model.lifecycle.targets].sort((left, right) =>
  Number(right.isCurrent) - Number(left.isCurrent)));
const otherCount = computed(() => targets.value.filter((target) => !target.isCurrent).length);
const commandPreview = computed(() => props.model.lifecycle.plan?.commandPreview.map((argument) => /^[a-zA-Z0-9_./:@=+-]+$/.test(argument) ? argument : JSON.stringify(argument)).join(" ") ?? "");
const scanning = computed(() => props.center.deepScanState === "scanning");
const candidates = computed(() => props.center.deepScanCandidates.filter((tool) => tool.kind === props.model.lifecycle.agentKind));
</script>

<template>
  <a-modal :visible="model.lifecycle.visible" width="min(780px, calc(100vw - 32px))" modal-class="surface-modal agent-workspace-modal agent-installation-modal" :footer="false" closable mask-closable esc-to-close unmount-on-close @cancel="model.close">
    <template #title><div class="agent-modal-heading"><AgentCliIcon v-if="model.lifecycle.agentKind" :kind="model.lifecycle.agentKind" :size="25" /><strong>{{ model.label }} · 版本与路径</strong></div></template>
    <div class="agent-modal-body">
      <p v-if="model.lifecycle.error || model.store.error" class="agent-workspace-error" role="alert">{{ model.lifecycle.error || model.store.error }}</p>
      <p v-if="model.lifecycle.preparingTargetId" role="status"><a-spin :size="14" /> 正在核对安装渠道、目标版本与升级命令，尚未开始升级…</p>
      <template v-if="model.lifecycle.plan">
        <div class="agent-plan-summary"><strong>检查并升级</strong><span>{{ model.lifecycle.plan.channelLabel }} · 当前可用 {{ model.lifecycle.plan.toVersion }}<template v-if="model.lifecycle.plan.fromVersion"> · 当前 {{ model.lifecycle.plan.fromVersion }}</template></span></div>
        <p>{{ model.lifecycle.plan.confirmationMessage }}</p>
        <details open class="agent-plan-command"><summary>安装位置与执行详情</summary><dl class="agent-plan-facts"><div><dt>安装目录</dt><dd>{{ model.lifecycle.plan.directory }}</dd></div><div><dt>安装命令超时</dt><dd>{{ model.lifecycle.plan.timeoutSeconds }} 秒</dd></div></dl>
        <ul class="agent-plan-notes"><li v-for="change in model.lifecycle.plan.changes" :key="change">{{ change }}</li></ul>
        <pre v-if="commandPreview">{{ commandPreview }}</pre></details>
        <p class="agent-workspace-note">{{ model.lifecycle.plan.cancellationBoundary }}</p>
        <p v-if="model.lifecycle.expired" class="agent-workspace-error">计划已过期，请返回后重新生成。</p>
        <footer class="agent-modal-actions">
          <a-button @click="model.lifecycle.clearPlan">返回版本与路径</a-button>
          <a-button type="primary" :disabled="model.lifecycle.expired || Boolean(model.lifecycle.preparingTargetId)" @click="model.lifecycle.confirm">确认升级</a-button>
        </footer>
      </template>
      <template v-else-if="model.lifecycle.agentKind">
        <section class="agent-installation-sources" aria-label="本机安装">
          <header class="agent-section-toolbar">
            <h3>当前安装</h3>
            <div class="agent-inline-actions">
              <a-button type="text" size="small" :loading="model.store.loading && !model.store.checkingVersions" :disabled="model.store.loading" @click="model.refresh()">重新检测</a-button>
              <a-button size="small" :loading="model.store.checkingVersions" :disabled="model.store.loading || !targets.length" @click="model.refresh('force')"><template #icon><RefreshCw :size="14" /></template>检查更新</a-button>
            </div>
          </header>
          <p v-if="!targets.length" class="agent-workspace-note" role="status">{{ model.store.loading ? '正在检测本机安装…' : model.store.error ? '未能读取本机安装，请重新检测。' : '未检测到本机安装。可查看官方安装说明，安装后点击“重新检测”。' }}</p>
          <p v-if="targets.length && !targets.some((target) => target.isCurrent)" class="agent-workspace-note">尚未确定当前启动安装，请展开其他安装并选择启动路径。</p>
          <AgentInstallationTarget v-for="target in targets.filter((item) => item.isCurrent)" :key="target.id" :target="target" :model="model" :hooks="hooks" @hooks="emit('hooks', $event)" />
        </section>
        <details v-if="otherCount" class="agent-path-settings" :open="targets.some((target) => !target.isCurrent && target.installation.id === model.diagnosticInstallationId)">
          <summary>其他安装（{{ otherCount }}）</summary>
          <AgentInstallationTarget v-for="target in targets.filter((item) => !item.isCurrent)" :key="target.id" :target="target" :model="model" :hooks="hooks" @hooks="emit('hooks', $event)" />
        </details>
        <details class="agent-path-settings"><summary>启动路径设置</summary>
          <div class="agent-inline-actions"><a-input v-model="model.pathDraft" aria-label="Agent 启动路径" placeholder="自动查找，或填写可执行文件路径" /><a-button :loading="model.savingPath === model.lifecycle.agentKind" :disabled="Boolean(model.savingPath)" @click="model.savePath(model.lifecycle.agentKind, model.pathDraft)">保存路径</a-button></div>
          <p class="agent-workspace-note">留空时自动查找。<template v-if="model.launchPath">当前使用：<code>{{ model.launchPath }}</code></template></p>
          <p v-if="model.pathError" class="agent-workspace-error" role="alert">{{ model.pathError }}</p>
        </details>
        <details class="agent-environment-deep-scan">
          <summary class="agent-deep-scan-summary"><strong>没有找到安装？</strong><ChevronRight :size="16" class="agent-deep-scan-chevron" aria-hidden="true" /></summary>
          <div class="agent-deep-scan-content">
            <p class="agent-workspace-note">深度扫描其他安装位置，找到后可直接采用其启动路径。</p>
            <div class="agent-inline-actions"><a-button size="small" :loading="scanning" @click="center.startDeepScan"><template #icon><Search :size="14" /></template>深度扫描</a-button><a-button v-if="scanning" size="small" @click="center.cancelDeepScan">取消扫描</a-button></div>
            <p v-if="center.deepScanError" class="agent-workspace-error" role="alert">{{ center.deepScanError }}</p>
            <p v-if="center.deepScanDraftChanged" class="agent-workspace-note">路径配置已变化，请重新扫描。</p>
            <div v-for="tool in candidates" :key="tool.kind" class="agent-deep-scan-candidate"><div class="agent-deep-scan-candidate-copy"><strong>{{ tool.label }} · {{ agentCliVersionLabel(tool.version) || '版本未读取' }}</strong><code>{{ tool.path }}</code></div><a-button size="small" :disabled="!center.canAdoptDeepScanCandidate(tool) || Boolean(model.savingPath)" @click="model.adoptCandidate(tool)">采用路径</a-button></div>
            <p v-if="center.deepScanState === 'ready' && !candidates.length" class="agent-workspace-note">未发现可用安装，请按官方安装说明安装后重新检测。</p>
          </div>
        </details>
        <footer class="agent-modal-actions">
          <a-button type="text" @click="emit('installationGuide', model.lifecycle.agentKind)">官方安装说明</a-button>
          <a-button @click="model.close">关闭</a-button>
        </footer>
      </template>
    </div>
  </a-modal>
</template>

<style scoped>
.agent-path-settings > summary, .agent-plan-command > summary { cursor: pointer; padding: 10px 0; color: var(--color-text-2); font-weight: 500; }
.agent-path-settings[open] > summary { margin-bottom: 8px; }
.agent-plan-command pre { white-space: pre-wrap; overflow-wrap: anywhere; padding: 12px; background: var(--color-fill-2); border-radius: 6px; }
.agent-path-settings { border-top: 1px solid var(--color-border-2); padding-top: 8px; }
</style>
