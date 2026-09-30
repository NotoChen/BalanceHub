<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { List } from "@arco-design/web-vue";
import type { AgentCatalogAsset } from "../../stores/agent-catalog-types";

const props = defineProps<{
  assets: AgentCatalogAsset[];
  revision: string;
  resetKey: string;
  estimatedSize: number;
  focusedId?: string | null;
}>();
const emit = defineEmits<{ scroll: [] }>();
defineSlots<{
  default(props: { asset: AgentCatalogAsset; index: number }): unknown;
}>();
const space = ref<HTMLElement | null>(null);
const list = ref<InstanceType<typeof List> | null>(null);
const plainList = ref<HTMLElement | null>(null);
const height = ref(400);
const width = ref(0);
const virtualized = computed(() => props.assets.length > 40);
const virtualListProps = computed(() => virtualized.value ? {
  height: height.value,
  itemKey: "id",
  estimatedSize: props.estimatedSize,
  // Hook rows can wrap; size the buffer using the shortest possible row.
  buffer: Math.max(10, Math.ceil(height.value / 80) + 2),
} : undefined);
const layoutKey = computed(() => JSON.stringify([props.revision, width.value, props.estimatedSize]));
let observer: ResizeObserver | null = null;
let scrollContainer: HTMLElement | null = null;
let anchorId: string | null = props.focusedId ?? null;
let restoreRequest = 0;

function measure() {
  if (!space.value?.clientWidth) return;
  width.value = Math.round(space.value.clientWidth);
  height.value = Math.max(1, space.value.clientHeight);
}
function displayedRows() {
  return Array.from(space.value?.querySelectorAll<HTMLElement>("[data-global-asset-id]") ?? []);
}
function controlKey(element: Element | null) {
  if (!(element instanceof HTMLElement)) return null;
  return element.dataset.agentKind ? `agent:${element.dataset.agentKind}`
    : element.dataset.assetFeature ? `feature:${element.dataset.assetFeature}`
      : element.getAttribute("aria-label") ?? element.getAttribute("title");
}
function onScroll(event: Event) {
  const target = event.target;
  if (!(target instanceof HTMLElement) || !target.querySelector("[data-global-asset-id]")) return;
  scrollContainer = target;
  emit("scroll");
}
function currentAnchor() {
  if (!scrollContainer?.isConnected || !space.value?.contains(scrollContainer)) return anchorId;
  const top = scrollContainer.getBoundingClientRect().top;
  return displayedRows().find((row) => row.getBoundingClientRect().bottom > top + 1)?.dataset.globalAssetId ?? anchorId;
}
function scrollToAsset(id: string | null) {
  if (virtualized.value) {
    list.value?.scrollIntoView(id ? { key: id, align: "top" } : { index: 0, align: "top" });
  } else {
    const container = plainList.value;
    const row = displayedRows().find((item) => item.dataset.globalAssetId === id);
    if (container) container.scrollTop = row
      ? container.scrollTop + row.getBoundingClientRect().top - container.getBoundingClientRect().top : 0;
  }
}
watch([() => props.assets, layoutKey, () => props.resetKey, () => props.focusedId], (current, previous) => {
  const request = ++restoreRequest;
  const reset = current[2] !== previous[2];
  const active = document.activeElement;
  const focusedRow = !reset && active instanceof HTMLElement && space.value?.contains(active)
    ? active.closest<HTMLElement>("[data-global-asset-id]") : null;
  const focusedId = focusedRow?.dataset.globalAssetId;
  const focusKey = focusedRow ? controlKey(active) : null;
  const preferredId = current[3] !== previous[3] ? current[3] : reset ? null : focusedId ?? currentAnchor();
  const id = preferredId && props.assets.some((asset) => asset.id === preferredId) ? preferredId : null;
  anchorId = id;
  void nextTick(() => {
    if (request !== restoreRequest) return;
    scrollToAsset(id);
    if (focusedId === id && focusKey) void nextTick(() => {
      if (request !== restoreRequest || document.activeElement !== document.body) return;
      const row = displayedRows().find((item) => item.dataset.globalAssetId === focusedId);
      const control = Array.from(row?.querySelectorAll<HTMLElement>("button, [tabindex]") ?? [])
        .find((element) => controlKey(element) === focusKey);
      (control ?? row?.querySelector<HTMLElement>(".agent-catalog-name"))?.focus({ preventScroll: true });
    });
  });
});
onMounted(() => {
  observer = new ResizeObserver(measure);
  if (space.value) observer.observe(space.value);
  measure();
  if (props.focusedId) void nextTick(() => scrollToAsset(props.focusedId ?? null));
});
onBeforeUnmount(() => { restoreRequest += 1; observer?.disconnect(); });
defineExpose({ scrollToAsset });
</script>

<template>
  <div ref="space" class="agent-catalog-list-space" @scroll.capture="onScroll">
    <div class="agent-catalog-list" :class="{ 'is-virtual': virtualized }" :role="virtualized ? 'list' : undefined">
      <div v-if="!virtualized" ref="plainList" class="agent-catalog-rows" role="list" :style="{ maxHeight: `${height}px` }">
        <template v-for="(asset, index) in assets" :key="asset.id"><slot :asset="asset" :index="index" /></template>
      </div>
      <List v-else-if="width > 0" ref="list" :key="layoutKey" class="agent-catalog-rows" :data="assets" :bordered="false" :split="false" :scrollbar="false" :virtual-list-props="virtualListProps" :style="{ maxHeight: `${height}px` }">
        <template #item="{ item, index }"><slot :asset="item" :index="index" /></template>
      </List>
    </div>
  </div>
</template>
