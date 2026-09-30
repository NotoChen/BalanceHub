<script setup lang="ts">
import { onBeforeUnmount, watch } from "vue";
import { useActionMenu } from "../../composables/useActionMenu";
import type CardIconButton from "../workspace-card/CardIconButton.vue";

const props = withDefaults(
  defineProps<{
    label: string;
    tone: InstanceType<typeof CardIconButton>["$props"]["tone"];
    panelClass?: string;
    disabled?: boolean;
  }>(),
  { panelClass: "", disabled: false },
);

const emit = defineEmits<{ interaction: [active: boolean] }>();
const { visible, menuId, viewportStyle, select, navigate } = useActionMenu({
  fitViewport: true,
});

watch(visible, (value) => emit("interaction", value));
watch(
  () => props.disabled,
  (disabled) => {
    if (disabled) visible.value = false;
  },
);
onBeforeUnmount(() => emit("interaction", false));
</script>

<template>
  <a-popover
    v-model:popup-visible="visible"
    :disabled="disabled"
    trigger="click"
    position="rt"
    content-class="workspace-card-action-popover provider-card-action-popover"
  >
    <button
      ref="actionMenuTrigger"
      type="button"
      class="card-icon-action"
      :class="`card-action-${tone}`"
      :disabled="disabled"
      :title="label"
      :aria-label="label"
      aria-haspopup="menu"
      :aria-expanded="visible"
      :aria-controls="visible ? menuId : undefined"
      @click.stop
      @pointerdown.stop
      @keydown.down.prevent="visible = true"
    >
      <slot name="icon" />
    </button>
    <template #content>
      <div
        :id="menuId"
        ref="actionMenuPanel"
        class="provider-card-action-panel"
        :class="panelClass"
        :style="viewportStyle"
        role="menu"
        :aria-label="label"
        @click.capture="select"
        @click.stop
        @pointerdown.stop
        @keydown.stop="navigate"
      >
        <slot />
      </div>
    </template>
  </a-popover>
</template>
