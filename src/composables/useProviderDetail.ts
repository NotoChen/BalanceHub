import { computed, ref, type Ref } from "vue";
import type { Provider } from "../stores/providers";

/// 卡片点击打开的只读详情面。编辑仍是独立动作，签到也只由按钮触发；
/// 详情按 id 跟随 store，刷新或后台更新后不需要重新打开。
export function useProviderDetail(providers: Ref<Provider[]>) {
  const visible = ref(false);
  const providerId = ref<string | null>(null);
  const provider = computed(
    () =>
      providers.value.find((item) => item.identity.id === providerId.value) ??
      null,
  );

  function openProviderDetail(target: Provider) {
    providerId.value = target.identity.id;
    visible.value = true;
  }

  function closeProviderDetail() {
    visible.value = false;
  }

  return {
    providerDetailVisible: visible,
    providerDetailProvider: provider,
    openProviderDetail,
    closeProviderDetail,
  };
}
