import { Message } from "@arco-design/web-vue";
import { sendAppNotification } from "../api/app";
import type { AppSettings } from "../stores/providers";

/**
 * 常规签到通知由后端任务服务发送；
 * 测试按钮（sendTestNotification）读设置抽屉草稿，允许先验证 webhook 再保存。
 */
export function useSystemNotification(draftSettings: AppSettings) {
  async function sendTestNotification() {
    try {
      const result = await sendAppNotification(
        draftSettings,
        "BalanceHub 测试通知",
        "**状态**：通知渠道已正常触发。",
        true,
      );
      if (result.results.length === 0) {
        Message.warning("没有启用的通知渠道");
        return;
      }
      const failures = result.results.filter((item) => !item.ok);
      if (failures.length === 0) {
        Message.success(`已发送 ${result.sentCount} 个通知渠道`);
        return;
      }
      // 部分失败也要逐个渠道点名，否则坏掉的 webhook 会被“已发送 N 个”掩盖。
      for (const failure of failures) {
        Message.error(`${failure.channelName}：${failure.message || "发送失败"}`);
      }
      if (result.sentCount > 0) {
        Message.warning(`其余 ${result.sentCount} 个渠道已发送`);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      Message.warning(message || "通知未触发，请检查配置");
    }
  }

  return {
    sendTestNotification,
  };
}
