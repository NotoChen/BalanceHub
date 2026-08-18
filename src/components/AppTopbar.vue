<script setup lang="ts">
import { computed } from "vue";
import {
  CalendarCheck2,
  CloudDownload,
  LoaderCircle,
  Megaphone,
  RefreshCw,
  Search,
  ServerPlus,
  SlidersHorizontal,
  X,
} from "@lucide/vue";
import { IconGithub } from "@arco-design/web-vue/es/icon";
import type { AgentCliKind, CliRuntimeSnapshot } from "../stores/providers";
import type { BackgroundTask } from "../composables/useBackgroundTaskCenter";
import { formatAppVersionLabel } from "../utils/app-version";
import AgentCliIcon from "./AgentCliIcon.vue";
import BackgroundTaskIndicator from "./BackgroundTaskIndicator.vue";

const props = defineProps<{
  refreshInProgress: boolean;
  globalCheckInInProgress: boolean;
  searchQuery: string;
  appVersion: string;
  checkingForUpdate: boolean;
  cliRuntime: CliRuntimeSnapshot;
  announcementsLoaded: boolean;
  announcementsLoading: boolean;
  announcementTotalCount: number;
  announcementUnreadCount: number;
  announcementErrorCount: number;
  backgroundTasks: BackgroundTask[];
  recentBackgroundTasks: BackgroundTask[];
  backgroundTaskCount: number;
}>();

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
}>();

function updateSearchQuery(event: Event) {
  emit("setSearchQuery", (event.target as HTMLInputElement | null)?.value ?? "");
}

const activeAgentCliSummaries = computed(() => {
  const activeInstances = props.cliRuntime.instances.filter(
    (instance) => instance.status !== "exited",
  );
  return props.cliRuntime.agents
    .filter((agent) => agent.capabilities.temporaryLaunch)
    .map((agent) => ({
      ...agent,
      count: activeInstances.filter((instance) => instance.cliKind === agent.kind).length,
    }))
    .filter((agent) => agent.count > 0);
});

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
    <div class="topbar-search-cluster">
      <label class="topbar-search-shell">
        <Search :size="16" :stroke-width="1.9" aria-hidden="true" />
        <input
          :value="searchQuery"
          type="search"
          placeholder="搜索名称、URL、用户或模型"
          aria-label="搜索中转站名称、URL、用户信息或模型"
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

    <div class="topbar-actions">
      <a-tooltip content="新建中转站">
        <a-button class="topbar-add-button" type="primary" aria-label="新建中转站" @click="emit('add')">
          <template #icon><ServerPlus :size="17" :stroke-width="1.9" /></template>
          <span>添加中转站</span>
        </a-button>
      </a-tooltip>
      <span class="topbar-action-divider" aria-hidden="true" />
      <a-tooltip content="刷新全部中转站和模型列表">
        <a-button
          class="topbar-icon-button topbar-icon-refresh"
          :class="{ 'is-loading': refreshInProgress }"
          shape="circle"
          :aria-busy="refreshInProgress"
          aria-label="刷新全部中转站"
          @click="emit('refresh')"
        >
          <template #icon><RefreshCw :class="{ 'topbar-action-spin': refreshInProgress }" :size="18" :stroke-width="1.9" /></template>
        </a-button>
      </a-tooltip>
      <a-tooltip content="一键签到">
        <a-button
          class="topbar-icon-button topbar-icon-checkin"
          shape="circle"
          :class="{ 'is-loading': globalCheckInInProgress }"
          :aria-busy="globalCheckInInProgress"
          aria-label="一键签到"
          @click="emit('checkIn')"
        >
          <template #icon><CalendarCheck2 :class="{ 'topbar-action-spin': globalCheckInInProgress }" :size="20" :stroke-width="1.8" /></template>
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
        <div class="topbar-runtime-cluster" aria-label="活动临时 CLI">
          <a-tooltip
            v-for="agent in activeAgentCliSummaries"
            :key="agent.kind"
            :content="`${agent.label}：${agent.count} 个活动临时 CLI`"
          >
            <button
              type="button"
              class="topbar-runtime-button"
              :aria-label="`查看 ${agent.label} 的 ${agent.count} 个活动临时 CLI`"
              @click="emit('openCli', agent.kind)"
            >
              <AgentCliIcon
                :kind="agent.kind"
                :size="18"
                :label="agent.label"
                :decorative="false"
              />
              <span class="topbar-action-badge">{{ agent.count }}</span>
            </button>
          </a-tooltip>
        </div>
      </template>
      <span class="topbar-action-divider" aria-hidden="true" />
      <a-tooltip :content="announcementTooltip">
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
      <a-tooltip :content="`检查更新（当前版本 ${formatAppVersionLabel(appVersion)}）`">
        <span class="topbar-action-anchor topbar-update-anchor">
          <a-button
            class="topbar-icon-button topbar-icon-update"
            :class="{ 'is-loading': checkingForUpdate }"
            shape="circle"
            :aria-busy="checkingForUpdate"
            aria-label="检查更新"
            @click="emit('checkForUpdate')"
          >
            <template #icon>
              <LoaderCircle
                v-if="checkingForUpdate"
                class="topbar-action-spin"
                :size="18"
                :stroke-width="1.9"
              />
              <CloudDownload v-else :size="18" :stroke-width="1.9" />
            </template>
          </a-button>
          <span class="topbar-action-badge topbar-version-badge">
            {{ formatAppVersionLabel(appVersion) }}
          </span>
        </span>
      </a-tooltip>
      <a-tooltip content="打开 GitHub 源码">
        <a-button
          class="topbar-icon-button topbar-icon-github"
          shape="circle"
          aria-label="打开 GitHub 源码"
          @click="emit('openGithub')"
        >
          <template #icon><IconGithub :size="19" /></template>
        </a-button>
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
    </div>
  </header>
</template>
