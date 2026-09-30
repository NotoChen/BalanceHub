<script setup lang="ts">
import type { Component } from "vue";
import { IconApps, IconBook, IconBranch, IconCode, IconDashboard, IconHistory, IconLink, IconStorage, IconThunderbolt } from "@arco-design/web-vue/es/icon";
import type { AgentWorkspacePage } from "../../stores/agent-workspace";

type WorkspaceIcon = AgentWorkspacePage | "history" | "activeSessions";
withDefaults(defineProps<{ page: WorkspaceIcon; size?: number }>(), { size: 15 });

const icons = {
  overview: IconDashboard,
  library: IconStorage,
  skill: IconBook,
  mcp: IconLink,
  extension: IconApps,
  hook: IconBranch,
  sessions: IconCode,
  history: IconHistory,
  activeSessions: IconThunderbolt,
} satisfies Record<WorkspaceIcon, Component>;
</script>

<template>
  <component :is="icons[page]" class="agent-workspace-icon" :class="`agent-workspace-icon--${page}`" :size="size" aria-hidden="true" focusable="false" />
</template>
