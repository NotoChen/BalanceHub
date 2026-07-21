<script setup lang="ts">
import { computed } from "vue";
import codexLogo from "../assets/logos/codex.svg";
import claudeLogo from "../assets/logos/claude.svg";
import opencodeLogo from "../assets/logos/opencode.svg";
import openclawLogo from "../assets/logos/openclaw.svg";
import hermesLogo from "../assets/logos/hermes.png";

export type BrandIconName = "codex" | "claude" | "opencode" | "openclaw" | "hermes";

const props = withDefaults(
  defineProps<{
    brand: BrandIconName;
    size?: number;
    decorative?: boolean;
    label?: string;
  }>(),
  {
    size: 16,
    decorative: true,
    label: "",
  },
);

const sources: Record<BrandIconName, string> = {
  codex: codexLogo,
  claude: claudeLogo,
  opencode: opencodeLogo,
  openclaw: openclawLogo,
  hermes: hermesLogo,
};

const labels: Record<BrandIconName, string> = {
  codex: "Codex",
  claude: "Claude Code",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
};

const alt = computed(() => props.label || labels[props.brand]);
</script>

<template>
  <img
    class="brand-icon"
    :class="`brand-icon-${brand}`"
    :src="sources[brand]"
    :width="size"
    :height="size"
    :alt="decorative ? '' : alt"
    :aria-hidden="decorative || undefined"
    :title="decorative ? undefined : alt"
    draggable="false"
  />
</template>
