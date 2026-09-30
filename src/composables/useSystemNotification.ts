import { ref, watch } from "vue";
import { Message } from "@arco-design/web-vue";
import { sendAppNotification, type NotificationSendResult } from "../api/app";
import type { AppSettings } from "../stores/providers";
import { useLatestRequest } from "./useLatestRequest";

/**
 * 常规签到通知由后端任务服务发送；
 * 测试按钮（sendTestNotification）读设置抽屉草稿，允许先验证 webhook 再保存。
 */
export function useSystemNotification(draftSettings: AppSettings) {
  const request = useLatestRequest({ timeoutMessage: "发送测试通知超时，请核对通知渠道后重试" });
  const notificationTestResult = ref<NotificationSendResult | null>(null);
  watch(() => JSON.stringify([draftSettings.notificationChannels, draftSettings.proxyMode, draftSettings.proxyUrl]), () => {
    request.invalidate();
    notificationTestResult.value = null;
  }, { flush: "sync" });

  async function sendTestNotification() {
    if (request.loading.value) return;
    notificationTestResult.value = null;
    const snapshot = JSON.parse(JSON.stringify(draftSettings)) as AppSettings;
    await request.run(() => sendAppNotification(
        snapshot,
        "BalanceHub 测试通知",
        "**状态**：通知渠道已正常触发。",
        true,
      ), (result) => {
      notificationTestResult.value = result;
      if (result.results.length === 0) {
        Message.warning("没有启用的通知渠道");
        return;
      }
      const failures = result.results.filter((item) => !item.ok);
      if (failures.length === 0) {
        Message.success(`已发送 ${result.sentCount} 个通知渠道`);
        return;
      }
      Message.warning(`${result.sentCount} 个渠道发送成功，${failures.length} 个失败，可在通知设置中查看原因`);
    });
  }

  return {
    sendTestNotification,
    testingNotification: request.loading,
    notificationTestError: request.error,
    notificationTestResult,
  };
}
