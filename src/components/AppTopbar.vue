<script setup lang="ts">
import {
  CalendarCheck2,
  RefreshCw,
  ServerPlus,
  SlidersHorizontal,
} from "@lucide/vue";

defineProps<{
  refreshInProgress: boolean;
  globalCheckInInProgress: boolean;
}>();

const emit = defineEmits<{
  add: [];
  refresh: [];
  checkIn: [];
  settings: [];
  startDrag: [event: MouseEvent];
}>();
</script>

<template>
  <header class="topbar" data-tauri-drag-region @mousedown="emit('startDrag', $event)">
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
