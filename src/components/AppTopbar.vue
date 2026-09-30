<script setup lang="ts">
import { computed } from "vue";
import {
  CalendarCheck2,
  CloudDownload,
  Ellipsis,
  LoaderCircle,
  Megaphone,
  RefreshCw,
  Search,
  ServerPlus,
  SlidersHorizontal,
  Terminal,
  X,
} from "@lucide/vue";
import { IconGithub } from "@arco-design/web-vue/es/icon";
import type { AgentCliKind, AgentRuntimeSnapshot, CliRuntimeSnapshot } from "../stores/providers";
import type { BackgroundTask } from "../composables/useBackgroundTaskCenter";
import { formatAppVersionLabel } from "../utils/app-version";
import AgentCliIcon from "./AgentCliIcon.vue";
import BackgroundTaskIndicator from "./BackgroundTaskIndicator.vue";
import AppTopbarMenu from "./AppTopbarMenu.vue";
import { activeAgentRuntimeSessions, isConfirmedAgentRuntimeSession } from "../utils/agent-runtime";
import type { AppWorkspaceView } from "../stores/agent-workspace";

const props = withDefaults(defineProps<{
  workspaceView?: AppWorkspaceView;
  refreshInProgress: boolean;
  globalCheckInInProgress: boolean;
  searchQuery: string;
  searchPlaceholder?: string;
  appVersion: string;
  checkingForUpdate: boolean;
  cliRuntime: CliRuntimeSnapshot;
  agentRuntimeSnapshot: AgentRuntimeSnapshot;
  announcementsLoaded: boolean;
  announcementsLoading: boolean;
  announcementTotalCount: number;
  announcementUnreadCount: number;
  announcementErrorCount: number;
  backgroundTasks: BackgroundTask[];
  recentBackgroundTasks: BackgroundTask[];
  backgroundTaskCount: number;
}>(), { workspaceView: "providers", searchPlaceholder: "搜索名称、URL、用户或模型" });

const emit = defineEmits<{
  add: [];
  checkForUpdate: [];
  openGithub: [];
  refresh: [];
  checkIn: [];
  openCli: [kind: AgentCliKind];
  openAnnouncements: [];
  clearBackgroundTasks: [];
  settings: [];
  startDrag: [event: MouseEvent];
  setSearchQuery: [value: string];
  setWorkspaceView: [value: AppWorkspaceView];
}>();

function updateSearchQuery(event: Event) {
  emit("setSearchQuery", (event.target as HTMLInputElement | null)?.value ?? "");
}

const activeAgentCliSummaries = computed(() => {
  const activeInstances = activeAgentRuntimeSessions(props.agentRuntimeSnapshot).filter(isConfirmedAgentRuntimeSession);
  return props.cliRuntime.agents
    .filter((agent) => activeInstances.some((session) => session.agentKind === agent.kind))
    .map((agent) => ({
      ...agent,
      count: activeInstances.filter((session) => session.agentKind === agent.kind).length,
    }))
    .filter((agent) => agent.count > 0);
});

const activeAgentSessionCount = computed(() =>
  activeAgentCliSummaries.value.reduce((total, agent) => total + agent.count, 0),
);

const announcementTooltip = computed(() => {
  if (props.announcementsLoading) return "正在读取站点公告";
  if (props.announcementUnreadCount > 0) {
    return `站点公告：${props.announcementUnreadCount} 条未读`;
  }
  if (props.announcementErrorCount > 0) {
    return props.announcementTotalCount > 0
      ? `站点公告：${props.announcementTotalCount} 条，${props.announcementErrorCount} 个站点读取失败`
      : `站点公告：${props.announcementErrorCount} 个站点读取失败`;
  }
  if (!props.announcementsLoaded) return "站点公告将在后台读取";
  return props.announcementTotalCount > 0
    ? `站点公告：${props.announcementTotalCount} 条`
    : "暂无站点公告";
});
</script>

<template>
  <header class="topbar" data-tauri-drag-region @mousedown="emit('startDrag', $event)">
    <div class="workspace-view-switch" role="group" aria-label="主面板视角" @mousedown.stop>
      <button type="button" :aria-pressed="workspaceView === 'providers'" :class="{ active: workspaceView === 'providers' }" @click="emit('setWorkspaceView', 'providers')">中转站</button>
      <button type="button" :aria-pressed="workspaceView === 'agents'" :class="{ active: workspaceView === 'agents' }" @click="emit('setWorkspaceView', 'agents')">Agent</button>
    </div>
    <div class="topbar-search-cluster" @mousedown.stop>
      <label class="topbar-search-shell">
        <Search :size="16" :stroke-width="1.9" aria-hidden="true" />
        <input
          :value="searchQuery"
          type="search"
          :placeholder="searchPlaceholder"
          :aria-label="workspaceView === 'agents' ? searchPlaceholder : '搜索中转站名称、URL、用户信息或模型'"
          autocomplete="off"
          @input="updateSearchQuery"
        />
        <button
          v-if="searchQuery"
          type="button"
          class="topbar-search-clear"
          aria-label="清除搜索"
          @click="emit('setSearchQuery', '')"
        >
          <X :size="14" :stroke-width="2" />
        </button>
      </label>
    </div>

    <div class="topbar-drag-region" data-tauri-drag-region />

    <div class="topbar-actions" @mousedown.stop>
      <a-tooltip v-if="workspaceView === 'providers'" content="新建中转站">
        <a-button class="topbar-add-button" type="primary" aria-label="新建中转站" @click="emit('add')">
          <template #icon><ServerPlus :size="17" :stroke-width="1.9" /></template>
          <span>添加中转站</span>
        </a-button>
      </a-tooltip>
      <span v-if="workspaceView === 'providers'" class="topbar-action-divider" aria-hidden="true" />
      <a-tooltip :content="workspaceView === 'agents' ? '刷新 Agent 环境与资产' : '刷新全部中转站和模型列表'">
        <a-button
          class="topbar-icon-button topbar-icon-refresh"
          :class="{ 'is-loading': refreshInProgress }"
          shape="circle"
          :aria-busy="refreshInProgress"
          :aria-label="workspaceView === 'agents' ? '刷新 Agent 工作台' : '刷新全部中转站'"
          @click="emit('refresh')"
        >
          <template #icon><RefreshCw :class="{ 'topbar-action-spin': refreshInProgress }" :size="18" :stroke-width="1.9" /></template>
        </a-button>
      </a-tooltip>
      <a-tooltip v-if="workspaceView === 'providers'" :content="globalCheckInInProgress ? '查看签到进度' : '一键签到'">
        <a-button
          class="topbar-icon-button topbar-icon-checkin"
          shape="circle"
          :class="{ 'is-loading': globalCheckInInProgress }"
          :aria-busy="globalCheckInInProgress"
          :aria-label="globalCheckInInProgress ? '查看签到进度' : '一键签到'"
          @click="emit('checkIn')"
        >
          <template #icon><CalendarCheck2 :size="20" :stroke-width="1.8" /></template>
        </a-button>
      </a-tooltip>
      <BackgroundTaskIndicator
        :tasks="backgroundTasks"
        :recent-tasks="recentBackgroundTasks"
        :active-count="backgroundTaskCount"
        @clear-recent="emit('clearBackgroundTasks')"
      />
      <template v-if="activeAgentCliSummaries.length > 0">
        <span class="topbar-action-divider" aria-hidden="true" />
        <AppTopbarMenu v-if="activeAgentCliSummaries.length > 1" :label="`查看 ${activeAgentSessionCount} 个活动会话`">
          <template #icon>
            <Terminal :size="18" :stroke-width="1.9" aria-hidden="true" />
            <span class="topbar-action-badge">{{ activeAgentSessionCount > 99 ? "99+" : activeAgentSessionCount }}</span>
          </template>
          <button
            v-for="agent in activeAgentCliSummaries"
            :key="agent.kind"
            type="button"
            role="menuitem"
            @click="emit('openCli', agent.kind)"
          >
            <AgentCliIcon :kind="agent.kind" :size="18" />
            <span>{{ agent.label }}</span>
            <small>{{ agent.count }} 个会话</small>
          </button>
        </AppTopbarMenu>
        <template v-else>
          <a-tooltip
            v-for="agent in activeAgentCliSummaries"
            :key="agent.kind"
            :content="`${agent.label}：${agent.count} 个活动会话`"
          >
            <button
              type="button"
              class="topbar-runtime-button"
              :aria-label="`查看 ${agent.label} 的 ${agent.count} 个活动会话`"
              @click="emit('openCli', agent.kind)"
            >
              <AgentCliIcon
                :kind="agent.kind"
                :size="18"
                :label="agent.label"
                :decorative="false"
              />
              <span class="topbar-action-badge">{{ agent.count > 99 ? "99+" : agent.count }}</span>
            </button>
          </a-tooltip>
        </template>
      </template>
      <span class="topbar-action-divider" aria-hidden="true" />
      <a-tooltip v-if="workspaceView === 'providers'" :content="announcementTooltip">
        <span class="topbar-action-anchor">
          <a-button
            class="topbar-icon-button topbar-icon-announcements"
            :class="{
              'is-loading': announcementsLoading,
              'has-unread': announcementUnreadCount > 0,
            }"
            shape="circle"
            :aria-busy="announcementsLoading"
            aria-label="打开站点公告"
            @click="emit('openAnnouncements')"
          >
            <template #icon>
              <LoaderCircle
                v-if="announcementsLoading"
                class="topbar-action-spin"
                :size="18"
                :stroke-width="1.9"
              />
              <Megaphone v-else :size="18" :stroke-width="1.9" />
            </template>
          </a-button>
          <span v-if="announcementUnreadCount > 0" class="topbar-action-badge">
            {{ announcementUnreadCount > 99 ? "99+" : announcementUnreadCount }}
          </span>
        </span>
      </a-tooltip>
      <a-tooltip content="应用设置">
        <a-button
          class="topbar-icon-button topbar-icon-settings"
          shape="circle"
          aria-label="应用设置"
          @click="emit('settings')"
        >
          <template #icon><SlidersHorizontal :size="20" :stroke-width="1.8" /></template>
        </a-button>
      </a-tooltip>
      <AppTopbarMenu label="更多应用操作" :busy="checkingForUpdate">
        <template #icon>
          <LoaderCircle v-if="checkingForUpdate" class="topbar-action-spin" :size="18" :stroke-width="1.9" aria-hidden="true" />
          <Ellipsis v-else :size="20" :stroke-width="1.9" aria-hidden="true" />
        </template>
        <button type="button" role="menuitem" :disabled="checkingForUpdate" @click="emit('checkForUpdate')">
          <CloudDownload :size="17" :stroke-width="1.9" aria-hidden="true" />
          <span>{{ checkingForUpdate ? "正在检查更新" : "检查更新" }}</span>
        </button>
        <button type="button" role="menuitem" @click="emit('openGithub')">
          <IconGithub :size="17" aria-hidden="true" />
          <span>打开 GitHub 源码</span>
        </button>
        <template #footer>
          <span>BalanceHub</span>
          <span>{{ formatAppVersionLabel(appVersion) }}</span>
        </template>
      </AppTopbarMenu>
    </div>
  </header>
</template>
