import { ref } from "vue";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { Message, Modal } from "@arco-design/web-vue";

function formatReleaseNotes(body?: string) {
  const notes = body?.trim();
  if (!notes) {
    return "";
  }
  return notes.length > 1200 ? `${notes.slice(0, 1200)}...` : notes;
}

export function useAppUpdater() {
  const checkingForUpdate = ref(false);

  async function checkForUpdate() {
    if (checkingForUpdate.value) return;

    checkingForUpdate.value = true;
    try {
      const update = await check();
      if (!update) {
        Message.success("当前已是最新版本");
        return;
      }

      const releaseNotes = formatReleaseNotes(update.body);
      Modal.confirm({
        title: "发现新版本",
        content: releaseNotes
          ? `发现 BalanceHub ${update.version}。\n\n更新说明：\n${releaseNotes}\n\n安装完成后应用会自动重启。`
          : `发现 BalanceHub ${update.version}，是否下载并安装？安装完成后应用会自动重启。`,
        okText: "安装并重启",
        cancelText: "稍后",
        async onOk() {
          checkingForUpdate.value = true;
          try {
            await update.downloadAndInstall();
            await relaunch();
          } catch (error) {
            Message.error(error instanceof Error ? error.message : String(error));
          } finally {
            checkingForUpdate.value = false;
          }
        },
        onCancel() {
          checkingForUpdate.value = false;
        },
      });
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      checkingForUpdate.value = false;
    }
  }

  return {
    checkingForUpdate,
    checkForUpdate,
  };
}
