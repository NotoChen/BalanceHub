import { nextTick, onBeforeUnmount, onMounted, ref, watch, type Ref } from "vue";

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

export function useBoundedChipPreview(options: {
  count: Ref<number>; total: Ref<number>; rows: Ref<number>; revision: Ref<unknown>;
  listRef: Ref<HTMLElement | null>; measureRef: Ref<HTMLElement | null>; moreRef: Ref<HTMLElement | null>;
}) {
  const { listRef, measureRef, moreRef } = options;
  const visibleCount = ref(options.count.value);
  let observer: ResizeObserver | null = null;
  let frame: number | null = null;
  let disposed = false;

  function measure() {
    const list = listRef.value, container = measureRef.value, more = moreRef.value;
    if (!list || !container || !more || !(list.clientWidth > 0)) return;
    const widths = Array.from(container.querySelectorAll<HTMLElement>("[data-preview-measure-chip]")).map((element) => element.offsetWidth);
    if (widths.length !== options.count.value) return;
    for (let count = widths.length; count >= 0; count -= 1) {
      const candidate = widths.slice(0, count);
      const hidden = Math.max(0, options.total.value - count);
      if (hidden) { more.textContent = `+${hidden}`; candidate.push(more.offsetWidth); }
      if (chipsFitRows(candidate, list.clientWidth, options.rows.value)) { visibleCount.value = count; return; }
    }
    visibleCount.value = 0;
  }
  function schedule() {
    if (disposed) return;
    if (typeof window === "undefined" || typeof window.requestAnimationFrame !== "function") { measure(); return; }
    if (frame !== null) window.cancelAnimationFrame(frame);
    frame = window.requestAnimationFrame(() => { frame = null; if (!disposed) measure(); });
  }
  async function reset() {
    visibleCount.value = options.count.value;
    await nextTick();
    if (disposed) return;
    observer?.disconnect();
    if (typeof ResizeObserver !== "undefined" && listRef.value) { observer = new ResizeObserver(schedule); observer.observe(listRef.value); }
    schedule();
  }
  watch([options.revision, options.count, options.total, options.rows], () => { void reset(); });
  onMounted(() => {
    void reset();
    if (typeof window !== "undefined") window.addEventListener?.("focus", schedule);
    if (typeof document !== "undefined") document.addEventListener("visibilitychange", schedule);
  });
  onBeforeUnmount(() => {
    disposed = true; observer?.disconnect();
    if (typeof window !== "undefined") window.removeEventListener?.("focus", schedule);
    if (typeof document !== "undefined") document.removeEventListener("visibilitychange", schedule);
    if (frame !== null) window.cancelAnimationFrame(frame);
  });
  return { visibleCount };
}
