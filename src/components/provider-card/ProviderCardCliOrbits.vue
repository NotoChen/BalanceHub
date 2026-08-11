<script setup lang="ts">
import {
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
  type CSSProperties,
} from "vue";
import { Bot } from "@lucide/vue";
import BrandIcon from "../BrandIcon.vue";
import type {
  ProviderCardCliOrbitMotion,
  ProviderCardCliOrbitSpec,
} from "../../utils/provider-card-cli-orbit";
import {
  advanceProviderCardCliOrbitMotion,
  createProviderCardCliOrbitMotion,
  layoutProviderCardCliOrbits,
  subscribeProviderCardCliOrbitFrames,
} from "../../utils/provider-card-cli-orbit";

const props = withDefaults(
  defineProps<{
    orbits?: readonly ProviderCardCliOrbitSpec[];
  }>(),
  {
    orbits: () => [],
  },
);

const renderedOrbits = ref(layoutProviderCardCliOrbits(props.orbits));
const host = ref<HTMLElement | null>(null);
const orbitPath = ref("path(\"M 8 0 H 92 Q 100 0 100 8 V 92 Q 100 100 92 100 H 8 Q 0 100 0 92 V 8 Q 0 0 8 0 Z\")");
const orbitMotions = new Map<string, ProviderCardCliOrbitMotion>();
const orbitNodes = new Map<string, HTMLElement>();
let resizeObserver: ResizeObserver | null = null;
let unsubscribeOrbitFrames: (() => void) | null = null;
let lastFrameAt: number | null = null;
let mounted = false;

function roundedRectPath(width: number, height: number) {
  const radius = Math.min(8, width / 2, height / 2);
  const right = width;
  const bottom = height;

  return [
    `M ${radius} 0`,
    `H ${right - radius}`,
    `Q ${right} 0 ${right} ${radius}`,
    `V ${bottom - radius}`,
    `Q ${right} ${bottom} ${right - radius} ${bottom}`,
    `H ${radius}`,
    `Q 0 ${bottom} 0 ${bottom - radius}`,
    `V ${radius}`,
    `Q 0 0 ${radius} 0`,
    "Z",
  ].join(" ");
}

function syncOrbitPath() {
  const rect = host.value?.getBoundingClientRect();
  if (!rect || rect.width <= 0 || rect.height <= 0) return;

  orbitPath.value = `path("${roundedRectPath(rect.width, rect.height)}")`;
}

function setOrbitNode(id: string, node: unknown) {
  const element =
    node && typeof node === "object" && "style" in node
      ? (node as HTMLElement)
      : null;
  if (element) {
    orbitNodes.set(id, element);
    const motion = orbitMotions.get(id);
    if (motion) {
      orbitNodes.get(id)?.style.setProperty(
        "--provider-card-cli-orbit-progress",
        `${motion.progress}%`,
      );
    }
    return;
  }
  orbitNodes.delete(id);
}

function syncOrbitMotions() {
  const activeIds = new Set(renderedOrbits.value.map((orbit) => orbit.id));
  for (const orbit of renderedOrbits.value) {
    if (!orbitMotions.has(orbit.id)) {
      orbitMotions.set(orbit.id, createProviderCardCliOrbitMotion(orbit.phaseProgress));
    }
    const motion = orbitMotions.get(orbit.id);
    if (motion) {
      orbitNodes.get(orbit.id)?.style.setProperty(
        "--provider-card-cli-orbit-progress",
        `${motion.progress}%`,
      );
    }
  }
  for (const id of orbitMotions.keys()) {
    if (!activeIds.has(id)) {
      orbitMotions.delete(id);
      orbitNodes.delete(id);
    }
  }
}

function renderOrbitFrame(now: number) {
  if (lastFrameAt === null) {
    lastFrameAt = now;
    return;
  }

  // Avoid a large catch-up jump after the window was backgrounded.
  const deltaMs = Math.min(Math.max(now - lastFrameAt, 0), 200);
  lastFrameAt = now;
  for (const [id, motion] of orbitMotions) {
    advanceProviderCardCliOrbitMotion(motion, deltaMs);
    orbitNodes.get(id)?.style.setProperty(
      "--provider-card-cli-orbit-progress",
      `${motion.progress}%`,
    );
  }
}

function prefersReducedMotion() {
  return (
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

function updateOrbitFrameSubscription() {
  if (!mounted) return;
  const shouldAnimate = renderedOrbits.value.length > 0 && !prefersReducedMotion();
  if (shouldAnimate && !unsubscribeOrbitFrames) {
    lastFrameAt = null;
    unsubscribeOrbitFrames = subscribeProviderCardCliOrbitFrames(renderOrbitFrame);
    return;
  }
  if (!shouldAnimate && unsubscribeOrbitFrames) {
    unsubscribeOrbitFrames();
    unsubscribeOrbitFrames = null;
    lastFrameAt = null;
  }
}

function orbitStyle(orbit: (typeof renderedOrbits.value)[number]): CSSProperties {
  const motion = orbitMotions.get(orbit.id);
  return {
    ...orbit.style,
    "--provider-card-cli-orbit-progress": `${motion?.progress ?? orbit.phaseProgress}%`,
    "--provider-card-cli-orbit-path": orbitPath.value,
  } as CSSProperties;
}

async function observeHost() {
  await nextTick();
  if (!mounted) return;
  syncOrbitMotions();
  syncOrbitPath();

  if (!resizeObserver || !host.value) return;
  resizeObserver.disconnect();
  resizeObserver.observe(host.value);
}

watch(
  () => props.orbits,
  () => {
    renderedOrbits.value = layoutProviderCardCliOrbits(props.orbits);
    syncOrbitMotions();
    updateOrbitFrameSubscription();
    void observeHost();
  },
  { deep: true, immediate: true },
);

onMounted(() => {
  mounted = true;
  if (typeof ResizeObserver !== "undefined") {
    resizeObserver = new ResizeObserver(syncOrbitPath);
  }
  updateOrbitFrameSubscription();
  void observeHost();
});

onBeforeUnmount(() => {
  mounted = false;
  unsubscribeOrbitFrames?.();
  unsubscribeOrbitFrames = null;
  resizeObserver?.disconnect();
  orbitMotions.clear();
  orbitNodes.clear();
});
</script>

<template>
  <div
    v-if="renderedOrbits.length"
    ref="host"
    class="provider-card-cli-orbits"
    aria-hidden="true"
  >
    <span
      v-for="orbit in renderedOrbits"
      :key="orbit.id"
      :ref="(node) => setOrbitNode(orbit.id, node)"
      class="provider-card-cli-orbit-icon"
      :data-cli-kind="orbit.id"
      :style="orbitStyle(orbit)"
    >
      <BrandIcon v-if="orbit.brand" :brand="orbit.brand" :size="16" />
      <Bot v-else :size="16" :stroke-width="1.8" />
    </span>
  </div>
</template>
