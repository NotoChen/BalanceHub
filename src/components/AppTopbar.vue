<script setup lang="ts">
import {
  CalendarCheck2,
  CloudDownload,
  LoaderCircle,
  RefreshCw,
  Search,
  ServerPlus,
  SlidersHorizontal,
  X,
} from "@lucide/vue";
import { IconGithub } from "@arco-design/web-vue/es/icon";
import { formatAppVersionLabel } from "../utils/app-version";

defineProps<{
  refreshInProgress: boolean;
  globalCheckInInProgress: boolean;
  searchQuery: string;
  appVersion: string;
  checkingForUpdate: boolean;
}>();

const emit = defineEmits<{
  add: [];
  checkForUpdate: [];
  openGithub: [];
  refresh: [];
  checkIn: [];
  settings: [];
  startDrag: [event: MouseEvent];
  setSearchQuery: [value: string];
}>();

function updateSearchQuery(event: Event) {
  emit("setSearchQuery", (event.target as HTMLInputElement | null)?.value ?? "");
}
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
      <span class="topbar-action-divider" aria-hidden="true" />
      <div class="topbar-update-cluster">
        <a-tooltip content="检查更新">
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
        </a-tooltip>
        <span class="topbar-version" :title="`当前版本 ${formatAppVersionLabel(appVersion)}`">
          {{ formatAppVersionLabel(appVersion) }}
        </span>
      </div>
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
