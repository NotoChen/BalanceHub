<script setup lang="ts">
import {
  CalendarCheck2,
  CalendarX2,
  CircleAlert,
  KeyRound,
  ListFilter,
  RefreshCw,
  ServerPlus,
  SlidersHorizontal,
  UserRoundKey,
} from "@lucide/vue";
import type { ProviderAuthFilter, ProviderStatusFilter } from "../utils/provider-filters";

defineProps<{
  refreshInProgress: boolean;
  globalCheckInInProgress: boolean;
  authFilter: ProviderAuthFilter;
  statusFilter: ProviderStatusFilter;
  hasActiveFilters: boolean;
}>();

const emit = defineEmits<{
  add: [];
  refresh: [];
  checkIn: [];
  settings: [];
  startDrag: [event: MouseEvent];
  setAuthFilter: [value: ProviderAuthFilter];
  toggleStatusFilter: [value: Exclude<ProviderStatusFilter, "all">];
  resetFilters: [];
}>();
</script>

<template>
  <header class="topbar" data-tauri-drag-region @mousedown="emit('startDrag', $event)">
    <div class="topbar-filters" aria-label="中转站筛选">
      <a-tooltip content="清除筛选">
        <button
          type="button"
          class="topbar-filter-button"
          :class="{ active: !hasActiveFilters, 'reset-active': hasActiveFilters }"
          :aria-pressed="!hasActiveFilters"
          aria-label="清除筛选"
          @click="emit('resetFilters')"
        >
          <ListFilter :size="17" :stroke-width="1.8" />
        </button>
      </a-tooltip>
      <span class="topbar-filter-divider" aria-hidden="true" />
      <a-tooltip content="账户认证">
        <button
          type="button"
          class="topbar-filter-button topbar-filter-account"
          :class="{ active: authFilter === 'account' }"
          :aria-pressed="authFilter === 'account'"
          aria-label="筛选账户认证"
          @click="emit('setAuthFilter', authFilter === 'account' ? 'all' : 'account')"
        >
          <UserRoundKey :size="17" :stroke-width="1.8" />
        </button>
      </a-tooltip>
      <a-tooltip content="API Key">
        <button
          type="button"
          class="topbar-filter-button topbar-filter-api-key"
          :class="{ active: authFilter === 'apiKey' }"
          :aria-pressed="authFilter === 'apiKey'"
          aria-label="筛选 API Key"
          @click="emit('setAuthFilter', authFilter === 'apiKey' ? 'all' : 'apiKey')"
        >
          <KeyRound :size="17" :stroke-width="1.8" />
        </button>
      </a-tooltip>
      <span class="topbar-filter-divider" aria-hidden="true" />
      <a-tooltip content="未签到">
        <button
          type="button"
          class="topbar-filter-button topbar-filter-warning"
          :class="{ active: statusFilter === 'warning' }"
          :aria-pressed="statusFilter === 'warning'"
          aria-label="筛选未签到"
          @click="emit('toggleStatusFilter', 'warning')"
        >
          <CalendarX2 :size="17" :stroke-width="1.8" />
        </button>
      </a-tooltip>
      <a-tooltip content="异常">
        <button
          type="button"
          class="topbar-filter-button topbar-filter-error"
          :class="{ active: statusFilter === 'error' }"
          :aria-pressed="statusFilter === 'error'"
          aria-label="筛选异常"
          @click="emit('toggleStatusFilter', 'error')"
        >
          <CircleAlert :size="17" :stroke-width="1.8" />
        </button>
      </a-tooltip>
      <span v-if="hasActiveFilters" class="topbar-filter-active-dot" aria-label="已启用筛选" />
    </div>
    <div class="topbar-drag-region" data-tauri-drag-region />

    <div class="topbar-actions">
      <a-tooltip content="新建中转站">
        <a-button
          class="topbar-icon-button topbar-icon-add"
          shape="circle"
          aria-label="新建中转站"
          @click="emit('add')"
        >
          <template #icon><ServerPlus :size="20" :stroke-width="1.8" /></template>
        </a-button>
      </a-tooltip>
      <a-tooltip content="刷新">
        <a-button
          class="topbar-icon-button topbar-icon-refresh"
          shape="circle"
          :loading="refreshInProgress"
          aria-label="刷新"
          @click="emit('refresh')"
        >
          <template #icon><RefreshCw :size="20" :stroke-width="1.8" /></template>
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
