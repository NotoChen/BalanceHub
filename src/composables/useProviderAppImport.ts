import { onBeforeUnmount } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import { openProviderAppImport } from "../api/provider-app-import";
import type {
  Provider,
  ProviderAppImportResult,
  ProviderAppImportTarget,
} from "../stores/provider-types";
import { withTimeout } from "../utils/promise-timeout";
import { copyText } from "./useClipboard";

export function useProviderAppImport() {
  let sequence = 0;
  let disposed = false;
  const latest = new Map<string, number>();
  const pending = new Map<string, ReturnType<typeof Message.loading>>();
  const dialogs = new Map<string, ReturnType<typeof Modal.confirm>>();

  onBeforeUnmount(() => {
    disposed = true;
    latest.clear();
    for (const message of pending.values()) message.close();
    for (const dialog of dialogs.values()) dialog.close();
    pending.clear();
    dialogs.clear();
  });

  function showLaunchFailure(
    result: ProviderAppImportResult,
    key: string,
    isCurrent: () => boolean,
  ) {
    const importUrl = result.importUrl;
    if (!importUrl) {
      Message.error(result.launchError || "无法打开应用");
      return;
    }
    const dialog = Modal.confirm({
      title: "未能打开 " + result.appLabel,
      content: result.launchError || "请安装并手动启动一次应用，再重试。",
      okText: "复制导入链接",
      cancelText: "关闭",
      closable: true,
      maskClosable: true,
      escToClose: true,
      onOk: () => {
        // Close the dialog immediately; clipboard access never locks the UI.
        void withTimeout(copyText(importUrl), 5_000, "复制超时，请重试").then(
          () => {
            if (isCurrent())
              Message.success("已复制 " + result.appLabel + " 导入链接");
          },
          () => {
            if (isCurrent()) Message.error("复制失败，请重试");
          },
        );
      },
      onClose: () => {
        if (dialogs.get(key) === dialog) dialogs.delete(key);
      },
    });
    dialogs.set(key, dialog);
  }

  async function addProviderToApp(
    provider: Provider,
    target: ProviderAppImportTarget,
  ) {
    const targetId =
      target.app === "ccSwitch" ? target.app + ":" + target.agent : target.app;
    const key = provider.identity.id + ":" + targetId;
    if (disposed || pending.has(key)) return;
    dialogs.get(key)?.close();
    dialogs.delete(key);
    const requestId = ++sequence;
    latest.set(key, requestId);
    const isCurrent = () => !disposed && latest.get(key) === requestId;
    const appLabel = target.app === "magpie" ? "Magpie" : "CC Switch";
    const loading = Message.loading({
      content: "正在打开 " + appLabel + "…",
      duration: 0,
      closable: true,
    });
    pending.set(key, loading);
    try {
      const result = await openProviderAppImport(provider.identity.id, target);
      if (!isCurrent()) return;
      if (result.launchError) {
        showLaunchFailure(result, key, isCurrent);
      } else {
        const agentLabel =
          result.targetLabel === result.appLabel
            ? ""
            : "（" + result.targetLabel + "）";
        Message.info(
          "已请求打开 " + result.appLabel + agentLabel + "，请在应用中确认导入",
        );
      }
    } catch (error) {
      if (isCurrent())
        Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      loading.close();
      if (pending.get(key) === loading) pending.delete(key);
    }
  }

  return { addProviderToApp };
}
