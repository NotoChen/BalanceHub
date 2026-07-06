import { nextTick, reactive } from "vue";
import type { Provider } from "../stores/providers";

export function useProviderContextMenu() {
  const providerContextMenu = reactive<{
    visible: boolean;
    x: number;
    y: number;
    provider: Provider | null;
  }>({
    visible: false,
    x: 0,
    y: 0,
    provider: null,
  });

  function closeProviderContextMenu() {
    providerContextMenu.visible = false;
    providerContextMenu.provider = null;
  }

  function openProviderContextMenu(record: Provider, event: Event) {
    const mouseEvent = event as MouseEvent;
    mouseEvent.preventDefault();
    providerContextMenu.provider = record;
    providerContextMenu.x = Math.min(mouseEvent.clientX, window.innerWidth - 220);
    providerContextMenu.y = Math.min(mouseEvent.clientY, window.innerHeight - 520);
    providerContextMenu.visible = true;
    void nextTick(() => {
      const menu = document.querySelector<HTMLElement>(".provider-context-menu");
      if (!menu) {
        return;
      }

      const rect = menu.getBoundingClientRect();
      const margin = 12;
      providerContextMenu.x = Math.max(
        margin,
        Math.min(providerContextMenu.x, window.innerWidth - rect.width - margin),
      );
      providerContextMenu.y = Math.max(
        margin,
        Math.min(providerContextMenu.y, window.innerHeight - rect.height - margin),
      );
    });
  }

  return {
    providerContextMenu,
    closeProviderContextMenu,
    openProviderContextMenu,
  };
}
