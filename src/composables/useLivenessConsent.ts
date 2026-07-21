import { computed } from "vue";
import { Modal } from "@arco-design/web-vue";
import { useProviderStore } from "../stores/providers";

/// 全 App 一次性的「自动测活会消耗真实额度」授权。
///
/// 默认不开启测活；首次开启任意自动测活（全局或单站）时弹窗确认一次，记录到
/// settings.livenessConsentAcceptedAt，之后不再逐站/逐次询问。
export function useLivenessConsent() {
  const store = useProviderStore();
  const consented = computed(() => Boolean(store.settings.livenessConsentAcceptedAt));

  /// 确保已授权：已授权直接放行；未授权则弹窗，接受后记录授权并放行，取消则返回 false。
  function ensureConsent(): Promise<boolean> {
    if (consented.value) {
      return Promise.resolve(true);
    }
    return new Promise((resolve) => {
      Modal.confirm({
        title: "授权自动测活",
        content:
          "自动测活会按周期用真实 API Key 发起调用，可能消耗账号的真实额度/费用。开启后将对所有启用了自动测活的中转站生效（仅需授权一次，可在设置中重置）。确定开启吗？",
        okText: "我已知晓并授权",
        cancelText: "取消",
        onOk: async () => {
          try {
            await store.acknowledgeLivenessCost();
            resolve(true);
          } catch {
            resolve(false);
          }
        },
        onCancel: () => resolve(false),
      });
    });
  }

  return { consented, ensureConsent };
}
