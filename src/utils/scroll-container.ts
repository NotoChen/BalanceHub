/** Reveal a row within a content boundary, including nested scroll areas.
 * Never continue into outer dialog wrappers or the page. */
export function revealInScrollContainer(
  boundary: HTMLElement | null | undefined,
  target: HTMLElement | null | undefined,
) {
  if (
    !boundary ||
    !target?.isConnected ||
    target === boundary ||
    !boundary.contains(target) ||
    !target.getClientRects().length
  )
    return;

  for (let container = target.parentElement; container; container = container.parentElement) {
    if (
      container.clientHeight &&
      container.scrollHeight > container.clientHeight &&
      /^(auto|scroll|overlay)$/.test(getComputedStyle(container).overflowY)
    ) {
      const viewport = container.getBoundingClientRect();
      const bounds = target.getBoundingClientRect();
      // Dialogs can still be running their scale animation when first focused.
      const scale = viewport.height / container.offsetHeight || 1;
      const top = (bounds.top - viewport.top) / scale - container.clientTop;
      const bottom = (bounds.bottom - viewport.top) / scale - container.clientTop;
      const inset = 8;
      if (top < inset) container.scrollTop += top - inset;
      else if (bottom > container.clientHeight - inset)
        container.scrollTop += Math.min(
          top - inset,
          bottom - container.clientHeight + inset,
        );
    }
    if (container === boundary) break;
  }
}
