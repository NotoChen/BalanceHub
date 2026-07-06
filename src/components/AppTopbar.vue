<script setup lang="ts">
import {
  IconPlus,
  IconRefresh,
  IconSettings,
  IconStamp,
} from "@arco-design/web-vue/es/icon";

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
      <a-tooltip content="添加">
        <a-button
          class="topbar-icon-button topbar-icon-add"
          shape="circle"
          aria-label="添加"
          @click="emit('add')"
        >
          <template #icon><icon-plus /></template>
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
          <template #icon><icon-refresh /></template>
        </a-button>
      </a-tooltip>
      <a-tooltip content="签到">
        <a-button
          class="topbar-icon-button topbar-icon-checkin"
          shape="circle"
          :loading="globalCheckInInProgress"
          aria-label="签到"
          @click="emit('checkIn')"
        >
          <template #icon><icon-stamp /></template>
        </a-button>
      </a-tooltip>
      <a-tooltip content="设置">
        <a-button
          class="topbar-icon-button topbar-icon-settings"
          shape="circle"
          aria-label="设置"
          @click="emit('settings')"
        >
          <template #icon><icon-settings /></template>
        </a-button>
      </a-tooltip>
    </div>
  </header>
</template>
