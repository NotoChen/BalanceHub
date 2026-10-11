<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { useVirtualizer } from "@tanstack/vue-virtual";
import {
  buildConfigurationDiff,
  configurationDiffRows,
  parseUnifiedDiff,
  type DiffDisplayRow,
} from "../utils/configuration-text-diff";
import "../styles/modules/configuration-text-editor.css";

const props = withDefaults(
  defineProps<{
    originalText?: string;
    modifiedText?: string;
    unifiedPatch?: string | null;
    contextLines?: number;
  }>(),
  { originalText: "", modifiedText: "", unifiedPatch: null, contextLines: 3 },
);
const expanded = ref(false);
const viewport = ref<HTMLElement | null>(null);
const measuredWidth = ref(0);
let revision = 0;
const diff = computed(() =>
  props.unifiedPatch !== null
    ? parseUnifiedDiff(props.unifiedPatch)
    : buildConfigurationDiff(props.originalText, props.modifiedText),
);
const changes = computed(() => {
  let added = 0;
  let removed = 0;
  for (const line of diff.value.lines) {
    if (line.kind === "added") added += 1;
    if (line.kind === "removed") removed += 1;
  }
  return { added, removed };
});
const numberWidth = computed(() => {
  let lastLine = 0;
  for (const line of diff.value.lines) {
    lastLine = Math.max(lastLine, line.oldLine ?? 0, line.newLine ?? 0);
  }
  return Math.max(42, String(lastLine).length * 7 + 14);
});
const lineEndingNote = computed(() => {
  if (props.unifiedPatch !== null) return "";
  if (
    props.originalText !== props.modifiedText &&
    props.originalText.replace(/\r\n/g, "\n") ===
      props.modifiedText.replace(/\r\n/g, "\n")
  ) {
    return "文本行内容相同，换行符格式不同（LF / CRLF）。";
  }
  return "";
});
const finalNewlineNote = computed(() => {
  if (props.unifiedPatch !== null) return "";
  if (props.originalText.endsWith("\n") === props.modifiedText.endsWith("\n"))
    return "";
  return props.modifiedText.endsWith("\n")
    ? "+ 文件末尾新增换行"
    : "− 文件末尾的换行被移除";
});
const rows = computed<DiffDisplayRow[]>(() => {
  const lines = diff.value.lines;
  if (
    props.unifiedPatch !== null ||
    props.contextLines === undefined ||
    expanded.value
  )
    return lines;
  return configurationDiffRows(lines, props.contextLines);
});
const virtualized = computed(() => rows.value.length > 200);
const virtual = useVirtualizer(
  computed(() => ({
    enabled: virtualized.value,
    count: rows.value.length,
    getScrollElement: () => viewport.value,
    getItemKey: (index: number) => rows.value[index].id,
    estimateSize: () => 30,
    overscan: 8,
  })),
);
const displayed = computed(() =>
  virtualized.value
    ? virtual.value
        .getVirtualItems()
        .map((item) => ({
          line: rows.value[item.index],
          index: item.index,
          start: item.start,
        }))
    : rows.value.map((line, index) => ({ line, index, start: 0 })),
);
function measure(element: unknown) {
  if (!virtualized.value || !(element instanceof HTMLElement)) return;
  virtual.value.measureElement(element);
  const text = element.querySelector(".configuration-diff-text-content");
  if (text && !element.querySelector(".configuration-diff-line-hunk")) {
    // Keep previously seen long lines reachable when they leave the viewport.
    const width =
      Math.ceil(text.getBoundingClientRect().width) +
      42 +
      numberWidth.value * 2;
    measuredWidth.value = Math.max(measuredWidth.value, width);
  }
}
async function expandContext(from: number) {
  const current = revision;
  expanded.value = true;
  await nextTick();
  if (current !== revision) return;
  virtual.value.measure();
  if (virtualized.value) virtual.value.scrollToIndex(from, { align: "start" });
}
watch(diff, async (_, __, onCleanup) => {
  revision += 1;
  expanded.value = false;
  measuredWidth.value = 0;
  let cancelled = false;
  onCleanup(() => {
    cancelled = true;
  });
  await nextTick();
  if (cancelled) return;
  virtual.value.measure();
  viewport.value?.scrollTo({ top: 0, left: 0 });
});
</script>

<template>
  <div
    class="configuration-diff"
    aria-label="文本差异"
    :style="{ '--diff-number-width': `${numberWidth}px` }"
  >
    <div class="configuration-diff-summary">
      <span class="configuration-diff-added-count">+{{ changes.added }}</span
      ><span class="configuration-diff-removed-count"
        >−{{ changes.removed }}</span
      ><span>行</span>
    </div>
    <p v-if="lineEndingNote" class="configuration-diff-note">
      {{ lineEndingNote }}
    </p>
    <div
      ref="viewport"
      class="configuration-diff-scroll"
      tabindex="0"
      aria-label="差异内容"
    >
      <div
        class="configuration-diff-lines"
        :class="{ 'is-virtual': virtualized }"
        :style="
          virtualized
            ? {
                height: `${virtual.getTotalSize()}px`,
                minWidth: `${measuredWidth}px`,
              }
            : undefined
        "
      >
        <div
          v-for="{ line, index, start } in displayed"
          :key="line.id"
          :ref="measure"
          :data-index="index"
          class="configuration-diff-row"
          :style="
            virtualized ? { transform: `translateY(${start}px)` } : undefined
          "
        >
          <button
            v-if="line.kind === 'omitted'"
            type="button"
            class="configuration-diff-omitted"
            @click="expandContext(line.from)"
          >
            展开 {{ line.count }} 行相同内容
          </button>
          <div
            v-else
            class="configuration-diff-line"
            :class="'configuration-diff-line-' + line.kind"
          >
            <span class="configuration-diff-marker" aria-hidden="true">{{
              line.kind === "removed" ? "−" : line.kind === "added" ? "+" : " "
            }}</span>
            <span class="configuration-diff-number">{{
              line.oldLine ?? ""
            }}</span>
            <span class="configuration-diff-number">{{
              line.newLine ?? ""
            }}</span>
            <code class="configuration-diff-text"
              ><span class="configuration-diff-text-content">{{
                line.text || " "
              }}</span></code
            >
          </div>
        </div>
      </div>
      <p v-if="!diff.lines.length" class="configuration-diff-note">空文件</p>
      <p v-if="finalNewlineNote" class="configuration-diff-note">
        {{ finalNewlineNote }}
      </p>
    </div>
  </div>
</template>
