<script setup lang="ts">
import { useActionMenu } from "../composables/useActionMenu";

withDefaults(
  defineProps<{
    label: string;
    busy?: boolean;
  }>(),
  { busy: false },
);

const { visible, menuId, select, navigate } = useActionMenu();
</script>

<template>
  <a-popover
    v-model:popup-visible="visible"
    trigger="click"
    position="br"
    content-class="topbar-menu-popover"
  >
    <button
      ref="actionMenuTrigger"
      type="button"
      class="topbar-menu-button"
      :class="{ 'is-loading': busy }"
      :aria-label="label"
      :title="label"
      :aria-busy="busy"
      aria-haspopup="menu"
      :aria-expanded="visible"
      :aria-controls="visible ? menuId : undefined"
      @keydown.down.prevent="visible = true"
    >
      <slot name="icon" />
    </button>
    <template #content>
      <div
        :id="menuId"
        ref="actionMenuPanel"
        class="topbar-menu-list"
        role="menu"
        :aria-label="label"
        @click="select"
        @keydown="navigate"
      >
        <slot />
      </div>
      <div v-if="$slots.footer" class="topbar-menu-footer">
        <slot name="footer" />
      </div>
    </template>
  </a-popover>
</template>
