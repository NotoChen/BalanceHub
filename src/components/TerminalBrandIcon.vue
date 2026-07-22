<script setup lang="ts">
import { computed } from "vue";
import { Terminal } from "@lucide/vue";
import ghosttyLogo from "../assets/logos/ghostty.svg";
import warpLogo from "../assets/logos/warp.svg";
import iterm2Logo from "../assets/logos/iterm2.svg";
import weztermLogo from "../assets/logos/wezterm.svg";
import alacrittyLogo from "../assets/logos/alacritty.svg";
import type { TemporaryCliTerminalKind } from "../stores/providers";

const props = withDefaults(
  defineProps<{
    kind: TemporaryCliTerminalKind;
    name?: string;
    size?: number;
  }>(),
  {
    name: "终端",
    size: 22,
  },
);

const sources: Partial<Record<TemporaryCliTerminalKind, string>> = {
  ghostty: ghosttyLogo,
  warp: warpLogo,
  iTerm2: iterm2Logo,
  wezTerm: weztermLogo,
  alacritty: alacrittyLogo,
};

const source = computed(() => sources[props.kind] ?? "");
const iconStyle = computed(() => ({ width: `${props.size}px`, height: `${props.size}px` }));
</script>

<template>
  <span
    class="terminal-brand-icon"
    :class="[`terminal-brand-${kind}`, { 'is-fallback': !source }]"
    :style="iconStyle"
    :title="name"
  >
    <img v-if="source" :src="source" :alt="name" draggable="false" />
    <Terminal v-else :size="Math.max(14, size - 4)" :stroke-width="1.8" aria-hidden="true" />
  </span>
</template>

<style scoped>
.terminal-brand-icon {
  display: inline-grid;
  flex: 0 0 auto;
  place-items: center;
}

.terminal-brand-icon img {
  display: block;
  width: 100%;
  height: 100%;
}

.terminal-brand-iterm2 {
  border-radius: 5px;
  overflow: hidden;
}

.terminal-brand-icon.is-fallback {
  border-radius: 6px;
  background: transparent;
  color: var(--color-text-2);
}
</style>
