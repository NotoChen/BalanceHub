import { getCurrentWindow } from "@tauri-apps/api/window";

export function useWindowDrag() {
  function startWindowDrag(event: MouseEvent) {
    if (event.button !== 0) {
      return;
    }

    const target = event.target;
    if (
      target instanceof HTMLElement &&
      target.closest("button, a, input, textarea, select, [role='button'], .topbar-actions")
    ) {
      return;
    }

    event.preventDefault();
    void getCurrentWindow().startDragging().catch(() => {
      // Dragging can fail in non-Tauri previews; the packaged app has the permission enabled.
    });
  }

  return { startWindowDrag };
}
