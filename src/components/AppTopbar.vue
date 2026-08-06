<script setup lang="ts">
import {
  CalendarCheck2,
  CalendarX2,
  CircleAlert,
  KeyRound,
  ListFilter,
  RefreshCw,
  Search,
  ServerPlus,
  SlidersHorizontal,
  UsersRound,
  X,
  UserRoundKey,
} from "@lucide/vue";
import type { ProviderAuthFilter, ProviderStatusFilter } from "../utils/provider-filters";

defineProps<{
  refreshInProgress: boolean;
  globalCheckInInProgress: boolean;
  authFilter: ProviderAuthFilter;
  statusFilter: ProviderStatusFilter;
  searchQuery: string;
  visibleProviderCount: number;
  totalProviderCount: number;
  hasActiveFilters: boolean;
}>();

const emit = defineEmits<{
  add: [];
  refresh: [];
  checkIn: [];
  settings: [];
  startDrag: [event: MouseEvent];
  setAuthFilter: [value: ProviderAuthFilter];
  setStatusFilter: [value: ProviderStatusFilter];
  toggleStatusFilter: [value: Exclude<ProviderStatusFilter, "all">];
  setSearchQuery: [value: string];
  resetFilters: [];
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
      <div class="topbar-result-count" aria-live="polite">
        <strong>{{ visibleProviderCount }}</strong>
        <span>/ {{ totalProviderCount }}</span>
      </div>
    </div>

    <div class="topbar-drag-region" data-tauri-drag-region />

    <div class="topbar-filters" aria-label="中转站筛选">
      <div class="topbar-filter-heading">
        <ListFilter :size="14" :stroke-width="1.9" aria-hidden="true" />
        <span>筛选</span>
      </div>
      <div class="topbar-filter-segment" role="group" aria-label="认证方式">
        <button
          type="button"
          class="topbar-filter-button topbar-filter-all"
          :class="{ active: authFilter === 'all' }"
          :aria-pressed="authFilter === 'all'"
          aria-label="全部认证方式"
          @click="emit('setAuthFilter', 'all')"
        >
          <UsersRound :size="14" :stroke-width="1.9" />
          <span>全部</span>
        </button>
        <button
          type="button"
          class="topbar-filter-button topbar-filter-account"
          :class="{ active: authFilter === 'account' }"
          :aria-pressed="authFilter === 'account'"
          aria-label="筛选账户认证"
          @click="emit('setAuthFilter', authFilter === 'account' ? 'all' : 'account')"
        >
          <UserRoundKey :size="14" :stroke-width="1.9" />
          <span>账号</span>
        </button>
        <button
          type="button"
          class="topbar-filter-button topbar-filter-api-key"
          :class="{ active: authFilter === 'apiKey' }"
          :aria-pressed="authFilter === 'apiKey'"
          aria-label="筛选 API Key"
          @click="emit('setAuthFilter', authFilter === 'apiKey' ? 'all' : 'apiKey')"
        >
          <KeyRound :size="14" :stroke-width="1.9" />
          <span>API Key</span>
        </button>
      </div>
      <div class="topbar-filter-segment topbar-filter-status-segment" role="group" aria-label="中转站状态">
        <button
          type="button"
          class="topbar-filter-button topbar-filter-status-all"
          :class="{ active: statusFilter === 'all' }"
          :aria-pressed="statusFilter === 'all'"
          aria-label="全部状态"
          @click="emit('setStatusFilter', 'all')"
        >
          <span>状态</span>
        </button>
        <button
          type="button"
          class="topbar-filter-button topbar-filter-warning"
          :class="{ active: statusFilter === 'warning' }"
          :aria-pressed="statusFilter === 'warning'"
          aria-label="筛选未签到"
          @click="emit('toggleStatusFilter', 'warning')"
        >
          <CalendarX2 :size="14" :stroke-width="1.9" />
          <span>未签到</span>
        </button>
        <button
          type="button"
          class="topbar-filter-button topbar-filter-error"
          :class="{ active: statusFilter === 'error' }"
          :aria-pressed="statusFilter === 'error'"
          aria-label="筛选异常"
          @click="emit('toggleStatusFilter', 'error')"
        >
          <CircleAlert :size="14" :stroke-width="1.9" />
          <span>异常</span>
        </button>
      </div>
      <button
        v-if="hasActiveFilters"
        type="button"
        class="topbar-filter-clear"
        aria-label="清空搜索和筛选"
        @click="emit('resetFilters')"
      >
        <X :size="13" :stroke-width="2" />
        <span>清空</span>
      </button>
    </div>

    <div class="topbar-actions">
      <a-tooltip content="新建中转站">
        <a-button class="topbar-add-button" type="primary" aria-label="新建中转站" @click="emit('add')">
          <template #icon><ServerPlus :size="17" :stroke-width="1.9" /></template>
          <span>添加中转站</span>
        </a-button>
      </a-tooltip>
      <span class="topbar-action-divider" aria-hidden="true" />
      <a-tooltip content="刷新全部中转站">
        <a-button
          class="topbar-icon-button topbar-icon-refresh"
          shape="circle"
          :loading="refreshInProgress"
          aria-label="刷新全部中转站"
          @click="emit('refresh')"
        >
          <template #icon><RefreshCw :size="18" :stroke-width="1.9" /></template>
        </a-button>
      </a-tooltip>
      <a-tooltip content="一键签到">
        <a-button
          class="topbar-icon-button topbar-icon-checkin"
          shape="circle"
          :loading="globalCheckInInProgress"
          aria-label="一键签到"
          @click="emit('checkIn')"
        >
          <template #icon><CalendarCheck2 :size="20" :stroke-width="1.8" /></template>
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
