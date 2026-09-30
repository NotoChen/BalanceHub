import { onScopeDispose, ref } from "vue";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Message } from "@arco-design/web-vue";
import type { AppDataTransferResult } from "../api/app";
import { confirmAction } from "./provider-credential-dialogs";
import { withTimeout } from "../utils/promise-timeout";

interface UseAppDataTransferOptions {
  exportAppData: (path: string) => Promise<AppDataTransferResult>;
  importAppData: (path: string) => Promise<AppDataTransferResult>;
  beforeTransfer: () => Promise<boolean>;
}

export function useAppDataTransfer(options: UseAppDataTransferOptions) {
  const exportingAppData = ref(false);
  const importingAppData = ref(false);
  let disposed = false;
  onScopeDispose(() => { disposed = true; });

  async function exportAppData() {
    if (disposed || exportingAppData.value || importingAppData.value) return;
    exportingAppData.value = true;
    try {
      const now = new Date();
      const date = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
      const target = await save({
        title: "导出 BalanceHub 配置",
        defaultPath: `BalanceHub-backup-${date}.json`,
        filters: [{ name: "JSON 配置", extensions: ["json"] }],
      });
      if (!target || disposed) return;
      if (!await options.beforeTransfer()) throw new Error("应用设置尚未保存，请先在设置面板中重试保存");
      if (disposed) return;
      const result = await withTimeout(options.exportAppData(target), 30_000, "导出响应超时，请先检查所选位置是否已生成备份");
      if (!disposed) Message.success(`已导出 ${result.providerCount} 个中转站配置`);
    } catch (error) {
      if (!disposed) Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      exportingAppData.value = false;
    }
  }

  async function importAppData() {
    if (disposed || importingAppData.value || exportingAppData.value) return;
    importingAppData.value = true;
    try {
      const source = await open({
        title: "导入 BalanceHub 配置", multiple: false, directory: false,
        filters: [{ name: "JSON 配置", extensions: ["json"] }],
      });
      if (!source || Array.isArray(source) || disposed) return;
      const confirmed = await confirmAction("从备份恢复", "这会用备份完整替换当前中转站和应用设置。需要保留现有配置时，请先取消并导出备份。", "替换并恢复", "warning");
      if (!confirmed || disposed) return;
      if (!await options.beforeTransfer()) throw new Error("应用设置尚未保存，请先在设置面板中重试保存");
      if (disposed) return;
      const result = await withTimeout(options.importAppData(source), 30_000, "恢复响应超时，请先核对当前中转站列表与设置，再决定是否重试");
      if (disposed) return;
      Message.success(`已恢复 ${result.providerCount} 个中转站配置`);
    } catch (error) {
      if (!disposed) Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      importingAppData.value = false;
    }
  }

  return {
    exportingAppData,
    importingAppData,
    exportAppData,
    importAppData,
  };
}
