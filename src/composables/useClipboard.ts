/// 复制文本到剪贴板。优先使用 navigator.clipboard；
/// WebView 剪贴板权限不稳定时回退到传统的 textarea + execCommand。
export async function copyText(value: string) {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(value);
      return;
    } catch {
      // 权限不稳定时继续走兜底方案。
    }
  }

  const active = document.activeElement instanceof HTMLElement
    ? document.activeElement : null;
  const selection = document.getSelection();
  const ranges = selection
    ? Array.from({ length: selection.rangeCount }, (_, index) => selection.getRangeAt(index).cloneRange())
    : [];
  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.setAttribute("readonly", "true");
  textarea.tabIndex = -1;
  // Keep the fallback selection inside the viewport instead of at the static
  // position after all app content, which WebKit may try to scroll into view.
  Object.assign(textarea.style, {
    position: "fixed", top: "0", left: "0", width: "1px", height: "1px",
    padding: "0", border: "0", opacity: "0", pointerEvents: "none",
  });
  document.body.appendChild(textarea);
  try {
    textarea.focus({ preventScroll: true });
    textarea.select();
    if (!document.execCommand("copy")) throw new Error("复制失败");
  } finally {
    textarea.remove();
    if (active?.isConnected) active.focus({ preventScroll: true });
    if (selection && ranges.length) {
      selection.removeAllRanges();
      for (const range of ranges)
        if (range.commonAncestorContainer.isConnected) selection.addRange(range);
    }
  }
}

export function useClipboard() {
  return { copyText };
}
