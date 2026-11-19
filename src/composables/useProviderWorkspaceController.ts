import { computed, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { AppSettings, Provider } from "../stores/providers";
import { providerLivenessEnabled } from "../utils/provider-liveness";
import { useProviderCardTone } from "./useProviderCardTone";
import { useCardDragSort } from "./useCardDragSort";
import { providerBoardSection, providerBoardSections } from "../utils/provider-board-sections";
import { useOAuthAccountStore } from "../stores/oauth-accounts";

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
  /// 卡片点击打开详情面；编辑是详情里的独立动作，签到也只由按钮触发。
  openDetail: (provider: Provider) => void;
}

export function useProviderWorkspaceController(options: UseProviderWorkspaceControllerOptions) {
  const oauth = useOAuthAccountStore();
  function showProviderLivenessTimeline(provider: Provider) {
    return providerLivenessEnabled(provider, options.settings.value);
  }

  const dragSort = useCardDragSort({
    items: options.providers,
    getId: (provider) => provider.identity.id,
    gridSelector: ".overview-provider-grid",
    dataId: "providerId",
    dragGroup: providerBoardSection,
    reorder: options.reorderProviders,
    onError: (error) => Message.error(error instanceof Error ? error.message : String(error)),
  });

  /// Render order follows the live drag order while keeping the fixed object
  /// type sections; the same definition drives grouping, rendering and drag.
  const providerSections = computed(() => providerBoardSections
    .map((section) => ({
      key: section.key,
      label: section.label,
      providers: dragSort.orderedGroups.value.get(section.key) ?? [],
    }))
    .filter((section) => section.providers.length > 0));

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

    if (provider.actions.oauthAccount || provider.actions.cpaGateway) {
      oauth.openAccount(provider.identity.id, provider.actions.cpaGateway);
      return;
    }

    if (!provider.runtime.enabled || cardTone.providerIntermediateLabel(provider)) {
      return;
    }

    // Card click opens the detail view; check-in stays an explicit action so
    // a navigation click never silently spends a check-in attempt.
    options.openDetail(provider);
  }

  return {
    providerDrag: computed(() => ({ providerId: dragSort.state.id, dragging: dragSort.state.dragging })),
    dragOverProviderId: dragSort.overId,
    draggedProvider: dragSort.draggedItem,
    handleProviderPointerDown: dragSort.handlePointerDown,
    providerDragStyle: dragSort.dragStyle,
    resetProviderPointerDrag: dragSort.reset,
    ...cardTone,
    providerSections,
    showProviderLivenessTimeline,
    handleProviderCardClick,
    removeProvider,
    toggleProvider,
  };
}
