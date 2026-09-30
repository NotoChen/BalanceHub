import { computed, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { AppSettings, Provider } from "../stores/providers";
import { providerNeedsCheckIn } from "../utils/provider-actions";
import { providerLivenessEnabled } from "../utils/provider-liveness";
import { useProviderCardTone } from "./useProviderCardTone";
import { useCardDragSort } from "./useCardDragSort";

interface UseProviderWorkspaceControllerOptions {
  providers: Ref<Provider[]>;
  /** 已保存的 store 配置：看板分组依据保存生效的设置，而非抽屉里未保存的草稿。 */
  settings: Ref<AppSettings>;
  checkingInProviderIds: Ref<string[]>;
  probingCapabilitiesProviderId: Ref<string | null>;
  editingProviderId: Ref<string | null>;
  probingSite: Ref<boolean>;
  testingConnection: Ref<boolean>;
  completingCredentials: Ref<boolean>;
  reorderProviders: (ids: string[]) => Promise<unknown>;
  removeProvider: (id: string) => Promise<unknown>;
  toggleProvider: (id: string, enabled: boolean) => Promise<unknown>;
  checkInProvider: (provider: Provider) => Promise<unknown>;
}

export function useProviderWorkspaceController(options: UseProviderWorkspaceControllerOptions) {
  function showProviderLivenessTimeline(provider: Provider) {
    return providerLivenessEnabled(provider, options.settings.value);
  }

  const dragSort = useCardDragSort({
    items: options.providers,
    getId: (provider) => provider.identity.id,
    gridSelector: ".overview-provider-grid",
    dataId: "providerId",
    dragGroup: (provider) => (showProviderLivenessTimeline(provider) ? "liveness" : "regular"),
    reorder: options.reorderProviders,
    onError: (error) => Message.error(error instanceof Error ? error.message : String(error)),
  });

  const livenessProviders = computed(() => dragSort.orderedGroups.value.get("liveness") ?? []);
  const regularProviders = computed(() => dragSort.orderedGroups.value.get("regular") ?? []);

  const cardTone = useProviderCardTone({
    providers: options.providers,
    checkingInProviderIds: options.checkingInProviderIds,
    probingCapabilitiesProviderId: options.probingCapabilitiesProviderId,
    editingProviderId: options.editingProviderId,
    probingSite: options.probingSite,
    testingConnection: options.testingConnection,
    completingCredentials: options.completingCredentials,
  });

  async function removeProvider(provider: Provider) {
    await options.removeProvider(provider.identity.id);
  }

  async function toggleProvider(provider: Provider, enabled: boolean) {
    await options.toggleProvider(provider.identity.id, enabled);
  }

  async function handleProviderCardClick(provider: Provider) {
    if (dragSort.clickSuppressed.value) {
      dragSort.clickSuppressed.value = false;
      return;
    }

    if (!provider.runtime.enabled || cardTone.providerIntermediateLabel(provider)) {
      return;
    }

    if (providerNeedsCheckIn(provider)) {
      await options.checkInProvider(provider);
    }
  }

  return {
    providerDrag: computed(() => ({ providerId: dragSort.state.id, dragging: dragSort.state.dragging })),
    dragOverProviderId: dragSort.overId,
    draggedProvider: dragSort.draggedItem,
    handleProviderPointerDown: dragSort.handlePointerDown,
    providerDragStyle: dragSort.dragStyle,
    resetProviderPointerDrag: dragSort.reset,
    ...cardTone,
    livenessProviders,
    regularProviders,
    showProviderLivenessTimeline,
    handleProviderCardClick,
    removeProvider,
    toggleProvider,
  };
}
