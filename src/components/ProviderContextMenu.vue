<script setup lang="ts">
import {
  IconCheckCircleFill,
  IconDelete,
  IconEdit,
  IconPauseCircleFill,
  IconPlayCircleFill,
  IconRefresh,
  IconSync,
} from "@arco-design/web-vue/es/icon";
import type { Provider } from "../stores/providers";
import {
  providerCheckedInToday,
  supportsCheckIn,
} from "../utils/provider-display";

defineProps<{
  provider: Provider;
  x: number;
  y: number;
  checkingIn: boolean;
  testingLiveness: boolean;
}>();

const emit = defineEmits<{
  toggle: [provider: Provider];
  refresh: [provider: Provider];
  testLiveness: [provider: Provider];
  edit: [provider: Provider];
  checkIn: [provider: Provider];
  remove: [provider: Provider];
}>();
</script>

<template>
  <div
    class="provider-context-menu"
    :style="{ left: `${x}px`, top: `${y}px` }"
    @click.stop
    @contextmenu.prevent
  >
    <div class="provider-context-menu-group">
      <button
        v-if="provider.runtime.enabled && supportsCheckIn(provider)"
        type="button"
        :disabled="providerCheckedInToday(provider) || checkingIn"
        @click="emit('checkIn', provider)"
      >
        <icon-check-circle-fill />
        <span>{{ providerCheckedInToday(provider) ? "已签到" : "签到" }}</span>
      </button>
      <button type="button" :disabled="!provider.runtime.enabled" @click="emit('refresh', provider)">
        <icon-refresh />
        <span>刷新额度</span>
      </button>
      <button
        type="button"
        :disabled="!provider.runtime.enabled || !provider.auth.apiKey.trim() || testingLiveness"
        @click="emit('testLiveness', provider)"
      >
        <icon-sync />
        <span>{{ testingLiveness ? "测活中" : "测活" }}</span>
      </button>
    </div>

    <div class="provider-context-menu-group">
      <button type="button" @click="emit('edit', provider)">
        <icon-edit />
        <span>编辑</span>
      </button>
      <button type="button" @click="emit('toggle', provider)">
        <icon-pause-circle-fill v-if="provider.runtime.enabled" />
        <icon-play-circle-fill v-else />
        <span>{{ provider.runtime.enabled ? "停用" : "启用" }}</span>
      </button>
      <button type="button" class="danger" @click="emit('remove', provider)">
        <icon-delete />
        <span>删除</span>
      </button>
    </div>
  </div>
</template>
