import { ref } from "vue";

/// 本地会话对账不绑定具体中转站：它按 Agent 账号和项目汇总本地会话用量，
/// 再从站点读取同一窗口的请求量，用来核对“本地觉得用了多少、站点记了多少”。
/// 请求日志和 Agent 会话页都打开同一份入口，避免两处各维护一套可见状态。
export function useUsageReconciliation() {
  const visible = ref(false);

  function openUsageReconciliation() {
    visible.value = true;
  }

  function closeUsageReconciliation() {
    visible.value = false;
  }

  return {
    usageReconciliationVisible: visible,
    openUsageReconciliation,
    closeUsageReconciliation,
  };
}
