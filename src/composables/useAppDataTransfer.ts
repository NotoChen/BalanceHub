import { onScopeDispose, ref } from "vue";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Message } from "@arco-design/web-vue";
import { exportAppDataJson, importAppDataJson, type AppDataTransferResult } from "../api/app";
import { isWebHost } from "../api/transport.ts";
import { confirmAction } from "./provider-credential-dialogs";
import { withTimeout } from "../utils/promise-timeout";
import { downloadTextFile } from "../utils/download";

interface UseAppDataTransferOptions {
  exportAppData: (path: string) => Promise<AppDataTransferResult>;
  importAppData: (path: string) => Promise<AppDataTransferResult>;
  restoreLatestBackup: () => Promise<AppDataTransferResult>;
  installDevSampleData: () => Promise<void>;
  beforeTransfer: () => Promise<boolean>;
}

export function useAppDataTransfer(options: UseAppDataTransferOptions) {
  const exportingAppData = ref(false);
  const importingAppData = ref(false);
  const restoringBackup = ref(false);
  const installingDevSample = ref(false);
  let disposed = false;
  onScopeDispose(() => { disposed = true; });

  async function exportAppData() {
    if (disposed || exportingAppData.value || importingAppData.value || restoringBackup.value) return;
    exportingAppData.value = true;
    try {
      const now = new Date();
      const date = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
      const fileName = `BalanceHub-backup-${date}.json`;
      // 浏览器版没有本机保存对话框：直接下载同一份配置文本。
      const target = isWebHost ? fileName : await save({
        title: "导出 BalanceHub 配置",
        defaultPath: fileName,
        filters: [{ name: "JSON 配置", extensions: ["json"] }],
      });
      if (!target || disposed) return;
      if (!await options.beforeTransfer()) throw new Error("应用设置尚未保存，请先在设置面板中重试保存");
      if (disposed) return;
      const result = isWebHost
        ? await withTimeout(downloadBackup(target), 30_000, "导出响应超时，请稍后重试")
        : await withTimeout(options.exportAppData(target), 30_000, "导出响应超时，请先检查所选位置是否已生成备份");
      if (!disposed) Message.success(`已导出 ${result.providerCount} 个中转站配置`);
    } catch (error) {
      if (!disposed) Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      exportingAppData.value = false;
    }
  }

  async function importAppData() {
    if (disposed || importingAppData.value || exportingAppData.value || restoringBackup.value) return;
    importingAppData.value = true;
    try {
      const source = isWebHost ? await pickBackupFile() : await open({
        title: "导入 BalanceHub 配置", multiple: false, directory: false,
        filters: [{ name: "JSON 配置", extensions: ["json"] }],
      });
      if (!source || Array.isArray(source) || disposed) return;
      const confirmed = await confirmAction("从备份恢复", "这会用备份完整替换当前中转站和应用设置。需要保留现有配置时，请先取消并导出备份。", "替换并恢复", "warning");
      if (!confirmed || disposed) return;
      if (!await options.beforeTransfer()) throw new Error("应用设置尚未保存，请先在设置面板中重试保存");
      if (disposed) return;
      const result = isWebHost
        ? await withTimeout(uploadBackup(source as string), 60_000, "恢复响应超时，请先核对当前中转站列表与设置，再决定是否重试")
        : await withTimeout(options.importAppData(source as string), 30_000, "恢复响应超时，请先核对当前中转站列表与设置，再决定是否重试");
      if (disposed) return;
      Message.success(`已恢复 ${result.providerCount} 个中转站配置`);
    } catch (error) {
      if (!disposed) Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      importingAppData.value = false;
    }
  }

  /**
   * 启动读取失败时的原地恢复：由后端挑同目录最近的自动备份。这里不要求
   * 先保存设置——存储已经处于保护态，设置保存本身就会失败；备份恢复正是
   * 解除保护的入口。损坏的原文件由后端改名保留。
   */
  async function restoreLatestBackup() {
    if (disposed || restoringBackup.value || importingAppData.value || exportingAppData.value) return;
    restoringBackup.value = true;
    try {
      const confirmed = await confirmAction(
        "自动备份恢复",
        "将用配置目录中最近的自动备份替换当前数据。损坏的原文件会保留为 data.json.corrupt-时间戳，不会直接删除。",
        "恢复备份",
        "warning",
      );
      if (!confirmed || disposed) return;
      const result = await withTimeout(
        options.restoreLatestBackup(),
        30_000,
        "恢复响应超时，请检查配置目录后重启应用",
      );
      if (!disposed) {
        Message.success(
          `已从自动备份恢复 ${result.providerCount} 个中转站配置（${result.path}）`,
        );
      }
    } catch (error) {
      if (!disposed) {
        const message = error instanceof Error ? error.message : String(error);
        // 设置保存队列拒绝在“有草稿待保存”时开始恢复；存储保护态下这份
        // 草稿本身也保存不了，直接给出唯一可行的出口。
        Message.error(
          message === "应用设置尚未保存，请先重试保存"
            ? "有设置修改还没保存，暂时无法恢复备份；请重启应用后再试"
            : message,
        );
      }
    } finally {
      restoringBackup.value = false;
    }
  }

  /**
   * 开发版专用入口：把编造的演示中转站追加进隔离的开发配置，用来检查卡片
   * 渲染；真实站点原样保留，重复点击只覆盖同 ID 的演示卡片。只在开发构建
   * 出现，单击即生效，不再弹确认框——改动只落在开发版自己的数据上。
   */
  async function installDevSampleData() {
    if (disposed || installingDevSample.value || exportingAppData.value || importingAppData.value || restoringBackup.value) return;
    installingDevSample.value = true;
    try {
      await withTimeout(options.installDevSampleData(), 30_000, "追加演示数据超时，请确认开发版进程仍在运行");
      if (!disposed) Message.success("已追加演示中转站，自动刷新、签到与测活保持关闭");
    } catch (error) {
      if (!disposed) Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      installingDevSample.value = false;
    }
  }

  async function downloadBackup(fileName: string): Promise<AppDataTransferResult> {
    const text = await exportAppDataJson();
    downloadTextFile(fileName, text);
    const parsed = JSON.parse(text) as { providers?: unknown[]; schemaVersion?: number };
    return {
      path: fileName,
      schemaVersion: parsed.schemaVersion ?? 0,
      providerCount: parsed.providers?.length ?? 0,
    };
  }

  /** 浏览器文件选择：返回文件文本，取消时返回 null。 */
  function pickBackupFile(): Promise<string | null> {
    return new Promise((resolve, reject) => {
      const input = document.createElement("input");
      input.type = "file";
      input.accept = "application/json,.json";
      input.onchange = async () => {
        const file = input.files?.[0];
        if (!file) { resolve(null); return; }
        try { resolve(await file.text()); }
        catch { reject(new Error("读取所选备份文件失败，请重新选择")); }
      };
      input.oncancel = () => resolve(null);
      input.click();
    });
  }

  async function uploadBackup(text: string): Promise<AppDataTransferResult> {
    const result = await importAppDataJson(text);
    return result.transfer;
  }

  return {
    exportingAppData,
    importingAppData,
    restoringBackup,
    installingDevSample,
    exportAppData,
    importAppData,
    restoreLatestBackup,
    installDevSampleData,
  };
}
