import { ref } from "vue";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { Message } from "@arco-design/web-vue";

export function useAppUpdater() {
  const checkingForUpdate = ref(false);
  const updateDialogVisible = ref(false);
  const availableUpdateCurrentVersion = ref("");
  const availableUpdateVersion = ref("");
  const availableUpdateReleaseNotes = ref("");
  const installingUpdate = ref(false);
  const updateDownloadProgress = ref<number | null>(null);
  const updateInstallStatus = ref("");
  let pendingUpdate: Update | null = null;

  function resetInstallProgress() {
    updateDownloadProgress.value = null;
    updateInstallStatus.value = "";
  }

  async function closePendingUpdate() {
    const update = pendingUpdate;
    pendingUpdate = null;
    if (update) {
      await update.close().catch(() => {});
    }
  }

  function releaseNotesFromUpdate(update: Update) {
    const body = update.body?.trim();
    if (body) {
      return body;
    }

    const rawNotes = update.rawJson?.notes;
    if (typeof rawNotes === "string") {
      return rawNotes.trim();
    }

    const rawBody = update.rawJson?.body;
    return typeof rawBody === "string" ? rawBody.trim() : "";
  }

  async function checkForUpdate() {
    if (checkingForUpdate.value || installingUpdate.value || updateDialogVisible.value) return;

    checkingForUpdate.value = true;
    try {
      const update = await check();
      if (!update) {
        Message.success("当前已是最新版本");
        return;
      }

      pendingUpdate = update;
      availableUpdateCurrentVersion.value = update.currentVersion;
      availableUpdateVersion.value = update.version;
      availableUpdateReleaseNotes.value = releaseNotesFromUpdate(update);
      resetInstallProgress();
      updateDialogVisible.value = true;
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      checkingForUpdate.value = false;
    }
  }

  async function dismissUpdate() {
    if (installingUpdate.value) return;
    updateDialogVisible.value = false;
    resetInstallProgress();
    await closePendingUpdate();
  }

  async function installUpdate() {
    const update = pendingUpdate;
    if (!update || installingUpdate.value) return;

    installingUpdate.value = true;
    updateInstallStatus.value = "正在准备下载";
    updateDownloadProgress.value = null;
    let downloadedBytes = 0;
    let contentLength: number | undefined;
    let installed = false;

    function handleDownloadEvent(event: DownloadEvent) {
      if (event.event === "Started") {
        contentLength = event.data.contentLength;
        updateDownloadProgress.value = contentLength ? 0 : null;
        updateInstallStatus.value = "正在下载更新";
        return;
      }
      if (event.event === "Progress") {
        downloadedBytes += event.data.chunkLength;
        if (contentLength) {
          updateDownloadProgress.value = Math.min(
            99,
            Math.round((downloadedBytes / contentLength) * 100),
          );
        }
        return;
      }

      updateDownloadProgress.value = 100;
      updateInstallStatus.value = "正在安装更新";
    }

    try {
      await update.downloadAndInstall(handleDownloadEvent);
      installed = true;
      pendingUpdate = null;
      updateInstallStatus.value = "正在重启应用";
      await update.close().catch(() => {});
      await relaunch();
    } catch (error) {
      if (installed) {
        updateDialogVisible.value = false;
        Message.error("更新已安装，但应用未能自动重启，请手动重启应用");
      } else {
        resetInstallProgress();
        Message.error(`更新安装失败：${error instanceof Error ? error.message : String(error)}`);
      }
    } finally {
      installingUpdate.value = false;
    }
  }

  return {
    checkingForUpdate,
    updateDialogVisible,
    availableUpdateCurrentVersion,
    availableUpdateVersion,
    availableUpdateReleaseNotes,
    installingUpdate,
    updateDownloadProgress,
    updateInstallStatus,
    checkForUpdate,
    dismissUpdate,
    installUpdate,
  };
}
