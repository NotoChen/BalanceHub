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

  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.setAttribute("readonly", "true");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  const ok = document.execCommand("copy");
  document.body.removeChild(textarea);
  if (!ok) {
    throw new Error("复制失败");
  }
}

export function useClipboard() {
  return { copyText };
}
