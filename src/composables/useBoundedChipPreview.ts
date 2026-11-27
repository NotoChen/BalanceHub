import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch, type Ref } from "vue";

/** Geometry for fitting Provider model chips within the visible rows. */
export function chipsFitRows(widths: readonly number[], availableWidth: number, rows: number, gap = 5) {
  let row = 1;
  let used = 0;
  for (const rawWidth of widths) {
    const width = Math.min(rawWidth, availableWidth);
    if (used === 0) used = width;
    else if (used + gap + width <= availableWidth + 0.5) used += gap + width;
    else { row += 1; if (row > rows) return false; used = width; }
  }
  return true;
}

// 每张卡都有测量层；窗口重新聚焦或可见性变化时把所有卡片合并到同一帧里
// 测量，而不是每张卡各自注册监听、各自排一个 rAF。
const activeMeasures = new Set<() => void>();
const pendingMeasures = new Set<() => void>();
let measureFrame: number | null = null;
let hostListenersBound = false;

function flushMeasures() {
  measureFrame = null;
  const tasks = [...pendingMeasures];
  pendingMeasures.clear();
  for (const task of tasks) task();
}

function ensureMeasureFrame() {
  if (measureFrame !== null) return;
  if (typeof window === "undefined" || typeof window.requestAnimationFrame !== "function") {
    flushMeasures();
    return;
  }
  measureFrame = window.requestAnimationFrame(flushMeasures);
}

function scheduleMeasure(task: () => void) {
  pendingMeasures.add(task);
  ensureMeasureFrame();
}

function scheduleAllMeasures() {
  for (const task of activeMeasures) pendingMeasures.add(task);
  ensureMeasureFrame();
}

function bindHostListeners() {
  if (hostListenersBound) return;
  hostListenersBound = true;
  if (typeof window !== "undefined") window.addEventListener?.("focus", scheduleAllMeasures);
  if (typeof document !== "undefined") document.addEventListener("visibilitychange", scheduleAllMeasures);
}

function unbindHostListeners() {
  if (!hostListenersBound) return;
  hostListenersBound = false;
  if (typeof window !== "undefined") window.removeEventListener?.("focus", scheduleAllMeasures);
  if (typeof document !== "undefined") document.removeEventListener("visibilitychange", scheduleAllMeasures);
}

export function useBoundedChipPreview(options: {
  count: Ref<number>; total: Ref<number>; rows: Ref<number> | number; revision: Ref<unknown>;
  listRef: Ref<HTMLElement | null>; measureRef: Ref<HTMLElement | null>; moreRef: Ref<HTMLElement | null>;
  /// 可见区是否还留着「+N」芯片。列表布局把总数写在标题里，不再为它预留宽度。
  reserveMore?: Ref<boolean> | boolean;
}) {
  const { listRef, measureRef, moreRef } = options;
  const rowLimit = computed(() => typeof options.rows === "number" ? options.rows : options.rows.value);
  const reserveMore = computed(() =>
    typeof options.reserveMore === "boolean" || options.reserveMore === undefined
      ? options.reserveMore ?? true
      : options.reserveMore.value,
  );
  const visibleCount = ref(options.count.value);
  let observer: ResizeObserver | null = null;
  let disposed = false;

  function measure() {
    const list = listRef.value, container = measureRef.value, more = moreRef.value;
    if (!list || !container || !more || !(list.clientWidth > 0)) return;
    const widths = Array.from(container.querySelectorAll<HTMLElement>("[data-preview-measure-chip]")).map((element) => element.offsetWidth);
    if (widths.length !== options.count.value) return;
    for (let count = widths.length; count >= 0; count -= 1) {
      const candidate = widths.slice(0, count);
      const hidden = Math.max(0, options.total.value - count);
      if (hidden && reserveMore.value) { more.textContent = `+${hidden}`; candidate.push(more.offsetWidth); }
      if (chipsFitRows(candidate, list.clientWidth, rowLimit.value)) { visibleCount.value = count; return; }
    }
    visibleCount.value = 0;
  }
  function schedule() {
    if (disposed) return;
    if (typeof window === "undefined" || typeof window.requestAnimationFrame !== "function") { measure(); return; }
    scheduleMeasure(measure);
  }
  async function reset() {
    visibleCount.value = options.count.value;
    await nextTick();
    if (disposed) return;
    observer?.disconnect();
    if (typeof ResizeObserver !== "undefined" && listRef.value) { observer = new ResizeObserver(schedule); observer.observe(listRef.value); }
    schedule();
  }
  watch([options.revision, options.count, options.total, rowLimit, reserveMore], () => { void reset(); });
  onMounted(() => {
    activeMeasures.add(measure);
    bindHostListeners();
    void reset();
  });
  onBeforeUnmount(() => {
    disposed = true;
    observer?.disconnect();
    activeMeasures.delete(measure);
    pendingMeasures.delete(measure);
    if (activeMeasures.size === 0) unbindHostListeners();
    if (measureFrame !== null && activeMeasures.size === 0) {
      window.cancelAnimationFrame(measureFrame);
      measureFrame = null;
    }
  });
  return { visibleCount };
}
