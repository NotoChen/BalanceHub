import { Message } from "@arco-design/web-vue";
import { sendAppNotification } from "../api/app";
import type { AppSettings, Provider } from "../stores/providers";

export function useSystemNotification(settings: AppSettings) {
  async function notifySystem(
    title: string,
    markdown: string,
    options: { ignoreSwitch?: boolean; provider?: Provider } = {},
  ) {
    try {
      const result = await sendAppNotification(
        settings,
        title,
        markdown,
        Boolean(options.ignoreSwitch),
        options.provider,
      );
      return result.sentCount > 0;
    } catch {
      return false;
    }
  }

  async function sendTestNotification() {
    try {
      const result = await sendAppNotification(
        settings,
        "BalanceHub 测试通知",
        "**状态**：通知渠道已正常触发。",
        true,
      );
      if (result.sentCount > 0) {
        Message.success(`已发送 ${result.sentCount} 个通知渠道`);
        return;
      }
      const failure = result.results.find((item) => !item.ok);
      Message.warning(failure?.message || "没有启用的通知渠道");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      Message.warning(message || "通知未触发，请检查配置");
    }
  }

  return {
    notifySystem,
    sendTestNotification,
  };
}
