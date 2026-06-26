import { ref } from "vue";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Message, Modal } from "@arco-design/web-vue";
import type { AppDataTransferResult } from "../api/app";

interface UseAppDataTransferOptions {
  exportAppData: (path: string) => Promise<AppDataTransferResult>;
  importAppData: (path: string) => Promise<AppDataTransferResult>;
  afterImport: () => void;
}

export function useAppDataTransfer(options: UseAppDataTransferOptions) {
  const exportingAppData = ref(false);
  const importingAppData = ref(false);

  async function exportAppData() {
    if (exportingAppData.value) return;

    const target = await save({
      title: "导出 BalanceHub 配置",
      defaultPath: `BalanceHub-backup-${new Date().toISOString().slice(0, 10)}.json`,
      filters: [{ name: "JSON 配置", extensions: ["json"] }],
    });
    if (!target) return;

    exportingAppData.value = true;
    try {
      const result = await options.exportAppData(target);
      Message.success(`已导出 ${result.providerCount} 个中转站配置`);
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      exportingAppData.value = false;
    }
  }

  async function importAppData() {
    if (importingAppData.value) return;

    const source = await open({
      title: "导入 BalanceHub 配置",
      multiple: false,
      directory: false,
      filters: [{ name: "JSON 配置", extensions: ["json"] }],
    });
    if (!source || Array.isArray(source)) return;

    Modal.confirm({
      title: "导入配置",
      content: "导入会用所选文件完整替换当前中转站和应用设置。",
      okText: "导入",
      cancelText: "取消",
      async onOk() {
        importingAppData.value = true;
        try {
          const result = await options.importAppData(source);
          options.afterImport();
          Message.success(`已导入 ${result.providerCount} 个中转站配置`);
        } catch (error) {
          Message.error(error instanceof Error ? error.message : String(error));
        } finally {
          importingAppData.value = false;
        }
      },
    });
  }

  return {
    exportingAppData,
    importingAppData,
    exportAppData,
    importAppData,
  };
}
