<script setup lang="ts">
import { IconClockCircle, IconLoading } from "@arco-design/web-vue/es/icon";
import type { AgentInstallation } from "../../../stores/provider-types";

defineProps<{ installation: AgentInstallation; compact?: boolean; checking?: boolean }>();

const stateLabels: Record<AgentInstallation["versionState"], string> = {
  upToDate: "已是最新稳定版",
  updateAvailable: "有可用更新",
  aheadOfStable: "高于稳定版",
  unknown: "版本未知",
  unavailable: "无法检查",
};
</script>

<template>
  <div class="agent-version-status" :class="{ compact }">
    <span class="agent-version-installed">
      {{ installation.availability === "unavailable" ? "未检测到安装" : installation.installedVersion || "未检测到版本" }}
    </span>
    <span class="agent-version-state" :class="`is-${installation.versionState}`">
      <IconLoading v-if="checking" class="agent-version-loading" />
      {{ checking ? "检查中" : installation.availability === "unavailable" ? "不可用" : stateLabels[installation.versionState] }}
    </span>
    <span v-if="!compact && installation.latestStableVersion" class="agent-version-latest">
      稳定版 {{ installation.latestStableVersion }}
    </span>
    <span v-if="!compact && installation.versionCheckedAt" class="agent-version-checked">
      <IconClockCircle /> {{ installation.versionCheckedAt }}
    </span>
  </div>
</template>
