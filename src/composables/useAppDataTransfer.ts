import { onScopeDispose, ref } from "vue";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Message } from "@arco-design/web-vue";
import type { AppDataTransferResult } from "../api/app";
import { confirmAction } from "./provider-credential-dialogs";
import { withTimeout } from "../utils/promise-timeout";

interface UseAppDataTransferOptions {
  exportAppData: (path: string) => Promise<AppDataTransferResult>;
  importAppData: (path: string) => Promise<AppDataTransferResult>;
  installDevSampleData: () => Promise<void>;
  beforeTransfer: () => Promise<boolean>;
}

export function useAppDataTransfer(options: UseAppDataTransferOptions) {
  const exportingAppData = ref(false);
  const importingAppData = ref(false);
  const installingDevSample = ref(false);
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

  /**
   * 开发版专用入口：把编造的演示中转站追加进隔离的开发配置，用来检查卡片
   * 渲染；真实站点原样保留，重复点击只覆盖同 ID 的演示卡片。只在开发构建
   * 出现，单击即生效，不再弹确认框——改动只落在开发版自己的数据上。
   */
  async function installDevSampleData() {
    if (disposed || installingDevSample.value || exportingAppData.value || importingAppData.value) return;
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

  return {
    exportingAppData,
    importingAppData,
    installingDevSample,
    exportAppData,
    importAppData,
    installDevSampleData,
  };
}
