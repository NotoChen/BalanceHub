<script setup lang="ts">
import { computed, onScopeDispose, ref } from "vue";
import type { AgentLifecycleOperation } from "../../stores/agent-lifecycle-types";
import { agentAssetOperationPhaseLabels } from "../../composables/useAgentAssetConsole";

const props = defineProps<{ operation: AgentLifecycleOperation }>();
const now = ref(Date.now());
const timer = globalThis.setInterval(() => { now.value = Date.now(); }, 1000);
onScopeDispose(() => globalThis.clearInterval(timer));
const running = computed(() => props.operation.phase !== "completed");
const elapsed = computed(() => {
  const end = running.value ? now.value : Date.parse(props.operation.updatedAt);
  const seconds = Math.max(0, Math.floor((end - Date.parse(props.operation.createdAt)) / 1000));
  return Number.isFinite(seconds) ? `${Math.floor(seconds / 60)} 分 ${seconds % 60} 秒` : "—";
});
// An argv display, not a shell script: quoting preserves argument boundaries.
const command = computed(() => props.operation.commandPreview.map((argument) => /^[a-zA-Z0-9_./:@=+-]+$/.test(argument) ? argument : JSON.stringify(argument)).join(" "));
const logStreams = computed(() => [
  { source: "stdout", text: props.operation.diagnostics?.stdout },
  { source: "stderr", text: props.operation.diagnostics?.stderr },
].filter((stream) => Boolean(stream.text)));
const stages = [
  { phase: "preparing", label: "准备任务" },
  { phase: "waitingForLock", label: "等待安装目录" },
  { phase: "revalidating", label: "核对安装" },
  { phase: "applying", label: "执行升级" },
  { phase: "verifying", label: "核对版本" },
] as const;
</script>

<template>
  <section class="upgrade-diagnostics" aria-label="升级执行详情">
    <div class="upgrade-progress" role="status">
      <a-spin v-if="running" :size="16" />
      <strong>{{ agentAssetOperationPhaseLabels[operation.phase] }}</strong>
      <span>耗时 {{ elapsed }}</span>
      <span v-if="operation.diagnostics?.exitCode != null">退出码 {{ operation.diagnostics.exitCode }}</span>
    </div>
    <ol class="upgrade-stages" aria-label="升级阶段">
      <li v-for="stage in stages" :key="stage.phase" :aria-current="operation.phase === stage.phase ? 'step' : undefined">{{ stage.label }}</li>
    </ol>
    <div v-if="command" class="upgrade-output"><strong>执行命令</strong><pre>{{ command }}</pre></div>
    <p class="upgrade-directory">检查时可用版本：{{ operation.toVersion }}<template v-if="operation.observedVersion"> · 实际版本：{{ operation.observedVersion }}</template></p>
    <p class="upgrade-directory">安装目录：<code>{{ operation.directory }}</code></p>
    <p v-if="operation.diagnostics?.error" class="agent-workspace-error" role="alert">{{ operation.diagnostics.error }}</p>
    <p v-if="operation.timedOut" class="agent-workspace-error">安装命令已超时；请以任务的实际版本核对结果为准。</p>
    <div class="upgrade-output">
      <strong>运行日志</strong>
      <div v-for="stream in logStreams" :key="stream.source" class="upgrade-log-stream">
        <span v-if="logStreams.length > 1" class="upgrade-log-source">{{ stream.source }}</span>
        <pre>{{ stream.text }}</pre>
      </div>
      <p v-if="!logStreams.length">{{ running ? '等待安装程序输出，内容会自动更新…' : '安装程序未产生运行日志。' }}</p>
    </div>
    <p v-if="operation.outputTruncated || operation.diagnostics?.truncated" class="agent-workspace-note">输出较长，每个输出流仅保留末尾 16 KiB。</p>
  </section>
</template>

<style scoped>
.upgrade-diagnostics { display: grid; gap: 12px; margin-block: 16px; min-width: 0; }
.upgrade-progress { display: flex; align-items: center; flex-wrap: wrap; gap: 12px; }
.upgrade-progress > span, .upgrade-directory { color: var(--color-text-2); font-size: 12px; }
.upgrade-stages { display: flex; flex-wrap: wrap; gap: 8px 20px; padding: 0; margin: 0; list-style: none; font-size: 12px; color: var(--color-text-3); }
.upgrade-stages li[aria-current="step"] { color: rgb(var(--primary-6)); font-weight: 600; border-bottom: 2px solid currentColor; padding-bottom: 4px; }
.upgrade-output { min-width: 0; padding: 12px 14px; border: 1px solid var(--color-border-2); border-radius: 8px; background: var(--color-fill-1); }
.upgrade-output > strong { font-size: 12px; }
.upgrade-output pre { max-height: 260px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font: 12px/1.6 ui-monospace, SFMono-Regular, Menlo, monospace; margin: 8px 0 0; user-select: text; }
.upgrade-log-stream + .upgrade-log-stream { margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--color-border-2); }
.upgrade-log-source { display: block; margin-top: 8px; color: var(--color-text-3); font: 11px ui-monospace, SFMono-Regular, Menlo, monospace; }
.upgrade-output p { margin: 8px 0 0; color: var(--color-text-3); font-size: 12px; }
.upgrade-directory { margin: 0; overflow-wrap: anywhere; }
</style>
