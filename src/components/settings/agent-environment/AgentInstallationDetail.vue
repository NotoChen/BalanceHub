<script setup lang="ts">
import { computed } from "vue";
import type {
  AgentHookHealthState,
  AgentHookInspection,
  AgentHookTrust,
  AgentInstallation,
} from "../../../stores/provider-types";
import { formatAgentAssetDiagnostics } from "../../../utils/agent-environment-diagnostics";

const props = defineProps<{
  installation: AgentInstallation;
  hookInspection: AgentHookInspection | null;
}>();
defineEmits<{ hooks: [] }>();

const channelLabels: Record<AgentInstallation["channel"], string> = {
  stable: "稳定",
  preview: "预览",
  nightly: "Nightly",
  unknown: "未知",
};
const versionSourceLabels: Record<AgentInstallation["installedVersionSource"], string> = {
  npmRegistry: "npm",
  localExecutable: "本地可执行文件",
  unknown: "未知",
};
const hookStateLabels: Record<AgentHookHealthState, string> = {
  not_installed: "未安装",
  installed_untrusted: "待信任",
  installed_unverified: "待验证",
  healthy: "运行正常",
  disabled: "已停用",
  conflict: "配置冲突",
  helper_missing: "辅助程序缺失",
  spool_blocked: "事件存储不可用",
  unsupported: "暂不支持",
};
const hookTrustLabels: Record<AgentHookTrust, string> = {
  unknown: "未知",
  trusted: "已信任",
  required: "需要信任",
  not_applicable: "不适用",
};
const installationDiagnostics = computed(() => formatAgentAssetDiagnostics(props.installation.diagnostics));

function formatHookTime(timestamp: number | null) {
  return timestamp === null ? "尚无事件" : new Date(timestamp).toLocaleString();
}
</script>

<template>
  <section class="agent-installation-diagnostics" aria-label="安装诊断">
    <ul v-if="installationDiagnostics.length" class="agent-hook-diagnostic-list"><li v-for="diagnostic in installationDiagnostics" :key="diagnostic">{{ diagnostic }}</li></ul>
    <p v-else class="agent-workspace-note">未发现安装异常。</p>
    <dl class="agent-plan-facts">
      <div><dt>发现方式</dt><dd>{{ installation.discoverySource === 'automatic' ? '自动发现' : '配置路径' }}</dd></div>
      <div><dt>版本通道</dt><dd>{{ channelLabels[installation.channel] }}</dd></div>
      <div><dt>版本读取来源</dt><dd>{{ versionSourceLabels[installation.installedVersionSource] }}</dd></div>
      <div v-if="installation.executableIdentity?.canonicalPath && installation.executableIdentity.canonicalPath !== installation.executablePath"><dt>实际文件</dt><dd>{{ installation.executableIdentity.canonicalPath }}</dd></div>
    </dl>
      <details class="agent-hook-diagnostics" aria-label="会话 Hook 诊断">
        <summary>
          <strong>会话 Hook 诊断</strong>
          <span v-if="hookInspection">{{ hookStateLabels[hookInspection.state] }}</span>
          <span v-else>尚未读取</span>
        </summary>
        <template v-if="hookInspection">
          <div class="agent-hook-diagnostic-facts">
            <span>安装 <strong>{{ hookInspection.installed ? "是" : "否" }}</strong></span>
            <span>启用 <strong>{{ hookInspection.enabled ? "是" : "否" }}</strong></span>
            <span>信任 <strong>{{ hookTrustLabels[hookInspection.trusted] }}</strong></span>
            <span>辅助程序 <strong>{{ hookInspection.helperAvailable ? "可用" : "缺失" }}</strong></span>
            <span>事件存储 <strong>{{ hookInspection.spoolAvailable ? "可用" : "不可用" }}</strong></span>
            <span>最近事件 <strong>{{ formatHookTime(hookInspection.lastEventAt) }}</strong></span>
          </div>
          <div class="agent-hook-diagnostic-path">
            <span>配置文件</span>
            <code :title="hookInspection.configPath">{{ hookInspection.configPath }}</code>
            <small :title="hookInspection.revision">revision {{ hookInspection.revision }}</small>
          </div>
          <ul v-if="hookInspection.diagnostics.length" class="agent-hook-diagnostic-list">
            <li v-for="diagnostic in hookInspection.diagnostics" :key="diagnostic">{{ diagnostic }}</li>
          </ul>
          <details v-if="hookInspection.ownership" class="agent-hook-ownership">
            <summary>BalanceHub 管理的回调 · {{ hookInspection.ownership.resources.length }} 项</summary>
            <div v-for="resource in hookInspection.ownership.resources" :key="resource.structuralIdentity">
              <strong>{{ resource.eventName }}</strong>
              <code :title="resource.structuralIdentity">{{ resource.structuralIdentity }}</code>
              <small :title="resource.contentFingerprint">{{ resource.contentFingerprint }}</small>
            </div>
          </details>
        </template>
        <p v-else>尚未读取会话 Hook 状态，可重新检测或打开 Hook 管理。</p>
      </details>
    <a-button type="text" size="small" @click="$emit('hooks')">打开 Hook 管理</a-button>
  </section>
</template>
